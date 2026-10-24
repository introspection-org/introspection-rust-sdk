#!/usr/bin/env bash
# Run the tests with coverage (cargo-llvm-cov) and fail when line coverage drops
# below the floor. The floor is a do-not-regress gate: raise it as coverage
# grows, and never lower it without saying why in the pull request.
#
#   scripts/coverage.sh                      # every test, gated on the floor
#   COVERAGE_THRESHOLD=0 scripts/coverage.sh -p introspection-sdk --test walk
#                                            # what the end-to-end walk alone reaches
#   COVERAGE_HTML=1 scripts/coverage.sh      # also write target/llvm-cov/html
#
# Needs `cargo install cargo-llvm-cov` and `rustup component add llvm-tools-preview`.
set -euo pipefail
cd "$(dirname "$0")/.."
THRESHOLD="${COVERAGE_THRESHOLD:-86}"

if [ "$#" -eq 0 ]; then set -- --workspace; fi
cargo llvm-cov --all-features "$@" --summary-only --fail-under-lines "$THRESHOLD"
if [ -n "${COVERAGE_HTML:-}" ]; then
  cargo llvm-cov report --html
  echo "HTML report: target/llvm-cov/html/index.html"
fi
