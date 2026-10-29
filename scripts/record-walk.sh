#!/usr/bin/env bash
# Re-record this SDK's end-to-end walk on introspection-cloud's Scenarios
# workflow (suite `sdk`), which runs every SDK's walk against a Compose stack
# built from the cloud ref, with this SDK checked out at the SDK ref. The
# recording is then copied into the walk's recording directory.
#
#   scripts/record-walk.sh                                 # cloud main, the current branch
#   scripts/record-walk.sh --cloud-ref my-cloud-branch --sdk-ref my-sdk-branch
#
# The SDK ref must be pushed and not behind its remote: the workflow checks it
# out from GitHub, not from this checkout. WALK_RECORD_LOCK, when set, names a
# directory used as a lock so only one recording runs at a time on this machine.
# Needs gh (signed in) and jq.
#
# Another SDK copies this script and changes only the four settings below.
set -euo pipefail

SDK=rust
SDK_REPO=introspection-org/introspection-rust-sdk
# Where the recording sits in the workflow's sdk-coverage artifact, and here.
ARTIFACT_RECORDING_DIR=recordings/rust
RECORDING_DIR=crates/introspection-sdk/tests/recordings/walk
REPLAY="cargo test -p introspection-sdk --all-features --test walk"

CLOUD_REPO=introspection-org/introspection-cloud
WORKFLOW=scenarios.yml
RECORDING_FILES=(recording.har bootstrap.json)
POLL=30

die() { echo "record-walk: $*" >&2; exit 1; }
say() { echo "record-walk: $*" >&2; }

cd "$(dirname "$0")/.."

cloud_ref=main
sdk_ref=$(git branch --show-current)
while [ "$#" -gt 0 ]; do
  case "$1" in
    --cloud-ref) cloud_ref=${2:?--cloud-ref needs a value}; shift 2 ;;
    --sdk-ref) sdk_ref=${2:?--sdk-ref needs a value}; shift 2 ;;
    -h | --help) sed -n '2,15p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) die "unknown argument: $1" ;;
  esac
done
[ -n "$sdk_ref" ] || die "detached HEAD: pass --sdk-ref <branch>"
command -v gh >/dev/null || die "gh is not installed"
command -v jq >/dev/null || die "jq is not installed"

# 1. The workflow records what GitHub has, so the SDK ref must be pushed and current.
remote=$(git remote -v | awk -v r="$SDK_REPO" '$2 ~ r"(\\.git)?$" && $3 == "(fetch)" { print $1; exit }')
[ -n "$remote" ] || die "no git remote points at $SDK_REPO"
git fetch --quiet "$remote" "+refs/heads/$sdk_ref:refs/remotes/$remote/$sdk_ref" 2>/dev/null ||
  die "$sdk_ref is not pushed to $SDK_REPO; push it first"
sdk_sha=$(git rev-parse "refs/remotes/$remote/$sdk_ref")
if local_sha=$(git rev-parse --verify --quiet "refs/heads/$sdk_ref"); then
  [ -z "$(git rev-list "$local_sha..$sdk_sha")" ] || die "$sdk_ref is behind $remote/$sdk_ref; pull first"
  [ -z "$(git rev-list "$sdk_sha..$local_sha")" ] || die "$sdk_ref has unpushed commits; push them first"
fi
say "recording $SDK_REPO@$sdk_ref ($sdk_sha) against $CLOUD_REPO@$cloud_ref"

tmp=$(mktemp -d)
lock_held=
cleanup() {
  rm -rf "$tmp"
  if [ -n "$lock_held" ]; then rm -rf "$WALK_RECORD_LOCK"; fi
}
trap cleanup EXIT
trap 'exit 130' INT TERM

# 2. One recording at a time on this machine.
if [ -n "${WALK_RECORD_LOCK:-}" ]; then
  until mkdir "$WALK_RECORD_LOCK" 2>/dev/null; do
    say "waiting for $WALK_RECORD_LOCK (held by $(cat "$WALK_RECORD_LOCK/branch" 2>/dev/null || echo unknown))"
    sleep "$POLL"
  done
  lock_held=1
  echo "$SDK_REPO@$sdk_ref" >"$WALK_RECORD_LOCK/branch"
fi

# 3. A dispatch cancels a running Scenarios run on the same ref, so wait for none to run.
busy() {
  gh run list --repo "$CLOUD_REPO" --workflow "$WORKFLOW" --limit 50 \
    --json databaseId,status,url --jq '[.[] | select(.status != "completed") | .url] | join(" ")'
}
while :; do
  runs=$(busy) || die "cannot list $CLOUD_REPO's Scenarios runs"
  [ -n "$runs" ] || break
  say "waiting for running Scenarios runs: $runs"
  sleep "$POLL"
done

# 4. Dispatch, then find that run: gh prints its URL; failing that, the newest
# dispatch on the cloud ref by this user created after the dispatch.
me=$(gh api user --jq .login)
since=$(date -u +%Y-%m-%dT%H:%M:%SZ)
out=$(gh workflow run "$WORKFLOW" --repo "$CLOUD_REPO" --ref "$cloud_ref" \
  -f suite=sdk -f sdk_refs="$SDK=$sdk_ref" 2>&1) || die "dispatch failed: $out"
run_id=$(grep -oE "/actions/runs/[0-9]+" <<<"$out" | head -1 | cut -d/ -f4 || true)
for _ in $(seq 20); do
  [ -z "$run_id" ] || break
  sleep 5
  run_id=$(gh run list --repo "$CLOUD_REPO" --workflow "$WORKFLOW" --event workflow_dispatch \
    --branch "$cloud_ref" --user "$me" --limit 10 --json databaseId,createdAt \
    --jq "[.[] | select(.createdAt >= \"$since\")] | last | .databaseId // empty")
done
[ -n "$run_id" ] || die "dispatched, but no run appeared: $out"
run_url="https://github.com/$CLOUD_REPO/actions/runs/$run_id"
say "run $run_url"

while :; do
  # A failed poll (a network blip in a long wait) is retried, not fatal.
  state=$(gh run view "$run_id" --repo "$CLOUD_REPO" --json status,conclusion \
    --jq '"\(.status) \(.conclusion)"' 2>/dev/null) || state="unknown -"
  read -r status conclusion <<<"$state"
  [ "$status" = completed ] && break
  say "run is $status"
  sleep "$POLL"
done
say "run $conclusion"

# 5. The walk is advisory in the workflow, so read its own report, not the run's conclusion.
art="$tmp/sdk-coverage"
gh run download "$run_id" --repo "$CLOUD_REPO" --name sdk-coverage --dir "$art" ||
  die "the run ($conclusion) left no sdk-coverage artifact: $run_url"
report="$art/walk-report-$SDK.json"
log="$art/walk-$SDK.log"
failed=$(jq -r 'if (.steps | length) == 0 then "no steps in the walk report" else
  .steps[] | select(.status != "passed" and .status != "skipped") | "step \(.n) \(.name): \(.status)" end' \
  "$report" 2>/dev/null) || failed="no readable walk report ($(basename "$report"))"
if [ -n "$failed" ]; then
  echo "$failed" >&2
  # The panic, the line after it (its message), and which steps failed.
  if [ -f "$log" ]; then
    grep -E -A1 'panicked at|^test result|step failed' "$log" | grep -v '^--$' | tail -40 >&2 || tail -40 "$log" >&2
  fi
  die "the $SDK walk failed in $run_url; nothing copied"
fi
for f in "${RECORDING_FILES[@]}"; do
  [ -s "$art/$ARTIFACT_RECORDING_DIR/$f" ] || die "the artifact has no $ARTIFACT_RECORDING_DIR/$f: $run_url"
done
mkdir -p "$RECORDING_DIR"
for f in "${RECORDING_FILES[@]}"; do cp "$art/$ARTIFACT_RECORDING_DIR/$f" "$RECORDING_DIR/$f"; done

# 6. Report.
echo
echo "Run: $run_url ($conclusion; $CLOUD_REPO@$cloud_ref, $SDK=$sdk_ref $sdk_sha)"
grep -F "| \`$SDK\`" "$art/summary.md" || true
echo
git status --short -- "$RECORDING_DIR"
echo "Copied the recording into $RECORDING_DIR. Now replay it and commit:"
echo "  $REPLAY"
