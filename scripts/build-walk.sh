#!/usr/bin/env bash
# Build the end-to-end walk's test binary and copy it to $1, so a caller runs
# the walk without compiling it (introspection-cloud's scripts/sdk-coverage.sh).
# Its compile-time paths are defaults the walk's environment overrides
# (WALK_RECORDINGS_DIR, WALK_REPORT, RECIPE_DIR).
set -euo pipefail

out=${1:?usage: scripts/build-walk.sh <output path>}
bin=$(cargo test --all-features --test walk --no-run --message-format=json |
  jq -r 'select(.reason == "compiler-artifact" and .target.name == "walk" and .executable != null) | .executable')
[ -n "$bin" ] || { echo "cargo produced no walk executable" >&2; exit 1; }
install -m 0755 "$bin" "$out"
