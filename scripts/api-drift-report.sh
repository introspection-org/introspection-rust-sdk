#!/usr/bin/env bash
# Runs the API contract check (crates/introspection-sdk/tests/api_contract_test.rs)
# against staging for .github/workflows/api-contract.yml.
#
# Writes to OUT_DIR the files the workflow and the notify step read:
#
#   state        red | green
#   body.txt     the Slack message below its headline: the run link and the
#                report, trimmed to fit
#   summary.md   the report in full, for the job summary
#
# Exits 0 when the check ran, drift or not, and then writes references=read
# to GITHUB_OUTPUT: both documents were fetched and parsed. Exits 2 when it
# could not run: a document that cannot be read (any HTTP error, 404
# included, a network error or a body that is not JSON) or a base URL that is
# unset. That is not drift, and must not post or change the notify state.
#
# usage: scripts/api-drift-report.sh OUT_DIR [RUN_URL]
# env:   INTROSPECTION_API_CONTRACT_UNCALLED  report: operations not called
#                                             are listed but do not fail
#        INTROSPECTION_CP_URL                 the Control Plane's base URL
#        INTROSPECTION_DP_URL                 the Data Plane's base URL
#        INTROSPECTION_CONTROLPLANE_SPEC      a path or URL instead of the CP's
#        INTROSPECTION_DATAPLANE_SPEC         a path or URL instead of the DP's
#        GITHUB_OUTPUT                        optional; receives references=read
set -euo pipefail

out="${1:?usage: api-drift-report.sh OUT_DIR [RUN_URL]}"
run_url="${2:-(local run)}"
root="$(cd "$(dirname "$0")/.." && pwd)"
mkdir -p "${out}"
out="$(cd "${out}" && pwd)"
report="${out}/report.txt"
# A report left by an earlier run must not pass for this one's.
rm -f "${report}"

# The check exits non-zero on drift, which is a result here, not a failure; a
# missing report means it never got to compare.
cd "${root}"
status=0
INTROSPECTION_API_CONTRACT_REPORT="${report}" \
  cargo test -p introspection-sdk --test api_contract_test -- --ignored --nocapture \
  >"${out}/check.log" 2>&1 || status=$?
if [ ! -s "${report}" ]; then
  echo "::error::The API contract check did not produce a report (exit ${status}); see below."
  grep -E "could not read the API reference|error(\[|:)" "${out}/check.log" || tail -n 40 "${out}/check.log"
  exit 2
fi

if [ "${status}" -ne 0 ]; then
  state=red
else
  state=green
fi
echo "${state}" >"${out}/state"
echo "references=read" >>"${GITHUB_OUTPUT:-/dev/null}"
headline="$(head -n 1 "${report}")"

# Slack cuts a message at 40,000 characters, but one much past 10,000 is
# unreadable in the channel; the job summary carries the rest.
trimmed="$(awk -v budget=6000 '
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
  }' "${report}")"

# Slack reads & < > as control characters, so they travel as entities.
# shellcheck disable=SC2016 # the backticks are a Slack code block
printf '%s\n\n```\n%s\n```\n' "${run_url}" "${trimmed}" |
  sed -e 's/&/\&amp;/g' -e 's/</\&lt;/g' -e 's/>/\&gt;/g' >"${out}/body.txt"

{
  echo "## API contract: ${state}"
  echo
  echo '```'
  cat "${report}"
  echo '```'
} >"${out}/summary.md"

echo "API contract: ${state}"
echo "${headline}"
