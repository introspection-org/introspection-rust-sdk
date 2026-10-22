#!/usr/bin/env bash
# Builds the nightly API drift report that .github/workflows/api-contract.yml
# posts to #main-build-alerts.
#
# Two reports, because they answer different questions:
#
# - SDK drift: where this SDK disagrees with the staging API reference, from
#   the API contract check (crates/introspection-sdk/tests/api_contract_test.rs).
# - Reference changelog: what changed in the API reference since the saved
#   copies in api-reference/ were taken, from oasdiff.
#
# Either one being non-empty makes the run red: SDK drift needs fixing, and a
# changelog means the saved copies are behind even when the SDK happens to
# match.
#
# Writes to OUT_DIR the files the notify step reads:
#
#   state        red | green
#   body.txt     the Slack message below its headline: summary, run link, and
#                the agent prompt carrying both reports trimmed to fit
#   summary.md   both reports in full, for the job summary
#
# Exits 0 when the report was built (red or green) and 2 when it could not be,
# for example when the staging reference did not answer: an outage is not
# drift, and must not post or change the notify state.
#
# usage: scripts/api-drift-report.sh OUT_DIR [RUN_URL]
# env:   OASDIFF                          the oasdiff binary (default: oasdiff)
#        INTROSPECTION_CONTROLPLANE_SPEC  a path or URL instead of staging's
#        INTROSPECTION_DATAPLANE_SPEC     a path or URL instead of staging's
set -euo pipefail

out="${1:?usage: api-drift-report.sh OUT_DIR [RUN_URL]}"
run_url="${2:-(local run)}"
root="$(cd "$(dirname "$0")/.." && pwd)"
oasdiff="${OASDIFF:-oasdiff}"
mkdir -p "${out}"
out="$(cd "${out}" && pwd)"

reference() {
  local plane="$1" env_var="$2"
  echo "${!env_var:-https://api.staging.introspection.dev/openapi/${plane}.json}"
}

# Characters the SDK drift, and each plane's changelog, may take in the Slack
# message. Slack cuts a message at 40,000 characters, but one much past 10,000
# is unreadable in the channel; the job summary carries the rest.
drift_budget=3500
plane_budget=2500

# The first lines of stdin that fit in $1 characters.
trim() {
  awk -v budget="$1" '
    { lines[NR] = $0 }
    END {
      used = 0
      for (i = 1; i <= NR; i++) {
        if (used + length(lines[i]) + 1 > budget) {
          print "… " NR - i + 1 " more lines, in the job summary"
          exit
        }
        print lines[i]
        used += length(lines[i]) + 1
      }
    }'
}

# 1. SDK drift. The check exits non-zero on drift, which is a result here, not
#    a failure; a missing report means it never got to compare.
cd "${root}"
status=0
INTROSPECTION_API_CONTRACT_REPORT="${out}/sdk-drift.txt" \
  cargo test -p introspection-sdk --test api_contract_test -- --ignored --nocapture \
  >"${out}/check.log" 2>&1 || status=$?
if [ ! -s "${out}/sdk-drift.txt" ]; then
  echo "::error::The API contract check did not produce a report (exit ${status}); see below."
  grep -E "could not read the API reference|error(\[|:)" "${out}/check.log" || tail -n 40 "${out}/check.log"
  exit 2
fi
sdk_red=false
if [ "${status}" -ne 0 ]; then
  sdk_red=true
fi

# 2. Reference changelog: saved copy → staging, one plane at a time.
changed=false
: >"${out}/changelog.txt"
for plane in controlplane dataplane; do
  case "${plane}" in
    controlplane) live="$(reference controlplane INTROSPECTION_CONTROLPLANE_SPEC)" ;;
    dataplane) live="$(reference dataplane INTROSPECTION_DATAPLANE_SPEC)" ;;
  esac
  if ! "${oasdiff}" changelog "api-reference/${plane}.json" "${live}" --format singleline \
    >"${out}/${plane}.raw" 2>"${out}/${plane}.err"; then
    echo "::error::oasdiff could not compare the saved ${plane} reference with ${live}"
    cat "${out}/${plane}.err"
    exit 2
  fi
  # "error at api-reference/x.json, in API GET /v1/y added ..." → "BREAKING: GET /v1/y added ..."
  {
    if grep -q "^No changes" "${out}/${plane}.raw"; then
      echo "${plane}: no changes"
    else
      changed=true
      echo "${plane}: $(head -n 1 "${out}/${plane}.raw")"
      tail -n +2 "${out}/${plane}.raw" | grep -v '^\s*$' | sed -E \
        -e 's/^error at [^,]*, in API /  BREAKING: /' \
        -e 's/^warning at [^,]*, in API /  MAYBE BREAKING: /' \
        -e 's/^info at [^,]*, in API /  /' \
        -e 's/^(error|warning|info) at [^,]*, /  /'
    fi
    echo
  } >"${out}/${plane}.txt"
  cat "${out}/${plane}.txt" >>"${out}/changelog.txt"
done

state=green
if [ "${sdk_red}" = true ] || [ "${changed}" = true ]; then
  state=red
fi
echo "${state}" >"${out}/state"

summary_line="SDK drift: $(head -n 1 "${out}/sdk-drift.txt")"
plane_lines="$(grep -h -m1 '^[a-z]*plane:' "${out}/controlplane.txt" "${out}/dataplane.txt" | sed 's/^/Reference /')"

# The prompt, with the placeholders filled and its leading comment dropped.
prompt="$(
  # The report keeps its first line; the rest is the findings.
  drift="$(trim "${drift_budget}" <"${out}/sdk-drift.txt")"
  planes="$(for plane in controlplane dataplane; do trim "${plane_budget}" <"${out}/${plane}.txt"; done)"
  awk 'BEGIN { skip = 0 } /^<!--/ { skip = 1 } !skip { print } /-->/ { skip = 0 }' \
    "${root}/.github/api-drift-prompt.md" |
    sed -e '/./,$!d' |
    RUN_URL="${run_url}" DRIFT="${drift}" PLANES="${planes}" awk '
      { gsub(/\{\{RUN_URL\}\}/, ENVIRON["RUN_URL"]) }
      /^\{\{SDK_DRIFT\}\}$/ { print ENVIRON["DRIFT"]; next }
      /^\{\{CHANGELOG\}\}$/ { print ENVIRON["PLANES"]; next }
      { print }'
)"

# Slack reads & < > as control characters, so they travel as entities.
slack_escape() {
  sed -e 's/&/\&amp;/g' -e 's/</\&lt;/g' -e 's/>/\&gt;/g'
}

if [ "${state}" = red ]; then
  # shellcheck disable=SC2016 # the backticks are a Slack code block
  printf '%s\n%s\n%s\n\nAgent prompt, ready to paste:\n```\n%s\n```\n' \
    "${summary_line}" "${plane_lines}" "${run_url}" "${prompt}" | slack_escape >"${out}/body.txt"
else
  printf '%s\n' "${run_url}" >"${out}/body.txt"
fi

{
  echo "## API drift: ${state}"
  echo
  echo "- ${summary_line}"
  echo "- ${plane_lines//$'\n'/$'\n'- }"
  echo
  echo "### SDK drift"
  echo
  echo '```'
  cat "${out}/sdk-drift.txt"
  echo '```'
  echo
  echo "### Reference changelog"
  echo
  echo '```'
  cat "${out}/changelog.txt"
  echo '```'
} >"${out}/summary.md"

echo "API drift: ${state}"
echo "${summary_line}"
echo "${plane_lines}"
