#!/usr/bin/env bash
# Refresh the SDK end-to-end walk's recording from the JS SDK's main, so the
# JS, Python, Rust and Swift walks replay the same server responses.
#
#   scripts/sync-walk-recording.sh                 # from GitHub main
#   scripts/sync-walk-recording.sh ../introspection-js-sdk   # from a checkout
#
# Copies tests/recordings/walk_<hash>/recording.har and
# tests/recordings/walk/bootstrap.json into
# crates/introspection-sdk/tests/recordings/walk/.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
dest="$root/crates/introspection-sdk/tests/recordings/walk"
source="${1:-}"
tmp=""
cleanup() { [ -n "$tmp" ] && rm -rf "$tmp"; }
trap cleanup EXIT

if [ -z "$source" ]; then
  tmp="$(mktemp -d)"
  git clone --quiet --depth 1 --filter=blob:none --sparse \
    https://github.com/introspection-org/introspection-js-sdk "$tmp/js"
  git -C "$tmp/js" sparse-checkout set --no-cone '/tests/recordings/walk*'
  source="$tmp/js"
fi

recordings="$source/tests/recordings"
shopt -s nullglob
hars=("$recordings"/walk_*/recording.har)
if [ "${#hars[@]}" -ne 1 ]; then
  echo "expected exactly one $recordings/walk_*/recording.har, found ${#hars[@]}" >&2
  exit 1
fi

mkdir -p "$dest"
cp "${hars[0]}" "$dest/recording.har"
cp "$recordings/walk/bootstrap.json" "$dest/bootstrap.json"
revision="$(git -C "$source" rev-parse --short HEAD 2>/dev/null || echo unknown)"
echo "synced the walk recording from introspection-js-sdk@$revision ($(basename "$(dirname "${hars[0]}")"))"
