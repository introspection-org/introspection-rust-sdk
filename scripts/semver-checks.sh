#!/usr/bin/env bash
# Checks the public API of every published crate against its latest crates.io
# release with cargo-semver-checks, once with default features and once with
# all features.
#
# release-please only bumps the version when a release PR merges, so between
# a breaking merge and its release the version in Cargo.toml still says the
# old one. A crate may therefore break its API when a breaking change is
# declared the way release-please reads it: a `type!:` subject or a
# `BREAKING CHANGE:` footer, either in the commits on the base branch since
# the crate's last release tag, or in PR_TITLE / PR_BODY (the squash commit
# release-please will see). Such a crate is not checked until it is released.
#
# Usage: scripts/semver-checks.sh [crate...]
# Env:   PR_TITLE, PR_BODY  the pull request's title and body (CI sets them)
#        BASE_REV           the commit whose history is scanned (default HEAD)

set -euo pipefail

if [ "$#" -gt 0 ]; then
  crates=("$@")
else
  crates=(introspection-sdk aauth-protocol)
fi
base="${BASE_REV:-HEAD}"

# Takes the messages as an argument: piping into grep -q would let an early
# match SIGPIPE the writer and, under pipefail, read as "not breaking".
declares_breaking() {
  grep -qE '^[a-z]+(\([^)]*\))?!:|^BREAKING[ -]CHANGE:' <<<"$1"
}

pr_breaking=false
if declares_breaking "${PR_TITLE:-}"$'\n'"${PR_BODY:-}"; then
  pr_breaking=true
fi

for crate in "${crates[@]}"; do
  # introspection-sdk is tagged vX.Y.Z, the AAuth crates <crate>-vX.Y.Z.
  if [ "$crate" = introspection-sdk ]; then
    pattern='v[0-9]*'
  else
    pattern="$crate-v[0-9]*"
  fi
  tag="$(git tag --list "$pattern" --merged "$base" --sort=-v:refname | head -n 1)"
  if [ -z "$tag" ]; then
    echo "::error::no release tag matching $pattern for $crate; fetch tags (fetch-depth: 0)"
    exit 1
  fi

  if [ "$pr_breaking" = true ]; then
    echo "$crate: the PR declares a breaking change; skipping the semver check"
    continue
  fi
  if declares_breaking "$(git log --format=%B "$tag..$base" -- "crates/$crate")"; then
    echo "$crate: a breaking change has merged since $tag; skipping the semver check"
    continue
  fi

  for features in --default-features --all-features; do
    echo "::group::$crate ($features) against crates.io, unreleased changes since $tag"
    cargo semver-checks --package "$crate" "$features"
    echo "::endgroup::"
  done
done
