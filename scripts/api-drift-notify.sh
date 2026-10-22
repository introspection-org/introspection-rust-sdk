#!/usr/bin/env bash
# Tell #main-build-alerts when the nightly API drift state changes: clean to
# drift, or drift back to clean.
#
# scripts/api-drift-report.sh builds the report; this script owns the decision
# and the post, which follow introspection-sre's change/gates/main-status.sh: a
# {text} JSON curled to the webhook, and an empty webhook prints the message
# instead. A run in the same state as the last post stays quiet, so the channel
# hears each drift once rather than every night until someone fixes it.
#
# The last posted state lives in STATE_DIR/last-posted, and the workflow
# carries that directory between runs. It is written only after a real post, so
# a rehearsal never swallows the edge the first real post needs.
#
# env:
#   REPORT_DIR         the OUT_DIR of api-drift-report.sh
#   STATE_DIR          holds last-posted between runs
#   SLACK_WEBHOOK_URL  empty prints the message instead of posting it
#   GITHUB_OUTPUT      optional; receives posted=true|false
set -euo pipefail

: "${REPORT_DIR:?}" "${STATE_DIR:?}"
output="${GITHUB_OUTPUT:-/dev/null}"

state="$(cat "${REPORT_DIR}/state")"
body="$(cat "${REPORT_DIR}/body.txt")"
previous=""
if [ -f "${STATE_DIR}/last-posted" ]; then
  previous="$(cat "${STATE_DIR}/last-posted")"
fi
echo "state=${state} last posted=${previous:-none}"

edge=""
if [ "${state}" = "red" ] && [ "${previous}" != "red" ]; then
  edge="🚨 API drift: introspection-rust-sdk is out of step with the staging API reference"
elif [ "${state}" = "green" ] && [ "${previous}" = "red" ]; then
  edge="✅ API drift cleared: introspection-rust-sdk matches the staging API reference again"
fi

post() {
  if [ -z "${SLACK_WEBHOOK_URL:-}" ]; then
    printf 'Slack post withheld (no webhook) — would have sent:\n%s\n' "$1"
    return 0
  fi
  local code
  code="$(curl -sS -o /dev/null -w '%{http_code}' --max-time 20 \
    -X POST -H 'Content-Type: application/json' \
    -d "$(jq -n --arg t "$1" '{text: $t}')" "${SLACK_WEBHOOK_URL}" || true)"
  case "${code}" in
    2*) echo "Posted to Slack." ;;
    *)
      echo "::warning::Slack refused this alert (HTTP ${code}), so the channel did not hear it."
      return 1
      ;;
  esac
}

if [ -z "${edge}" ]; then
  echo "No state change (${state}, last posted ${previous:-nothing}); staying quiet."
  echo "posted=false" >>"${output}"
  if [ -z "${SLACK_WEBHOOK_URL:-}" ] && [ "${state}" = "red" ]; then
    # A rehearsal shows the message even off an edge, since showing it is the
    # point of rehearsing.
    printf 'Rehearsal of the red message:\n%s\n%s\n' "🚨 API drift (rehearsal, not a state change)" "${body}"
  fi
  exit 0
fi

post "$(printf '%s\n%s' "${edge}" "${body}")"
if [ -z "${SLACK_WEBHOOK_URL:-}" ]; then
  echo "posted=false" >>"${output}"
  exit 0
fi
mkdir -p "${STATE_DIR}"
echo "${state}" >"${STATE_DIR}/last-posted"
echo "posted=true" >>"${output}"
