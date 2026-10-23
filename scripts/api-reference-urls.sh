#!/usr/bin/env bash
# Picks the staging API reference URLs for .github/workflows/api-contract.yml:
# the Control Plane and Data Plane base URLs whose /openapi.json the check
# reads.
#
# introspection-cloud's "API Reference Deployed" workflow sends both in the
# api-reference-deployed dispatch, and the workflow keeps the newest pair in
# the Actions cache (CACHE_DIR/urls.json) for the runs that carry none. A run
# takes the first of these that names both:
#
# 1. the dispatch payload, which is also written to CACHE_DIR; the workflow
#    saves it once the check has read both documents (drift or not);
# 2. a manual run's cp_url / dp_url inputs;
# 3. the pair the newest dispatch saved.
#
# A source naming only one of the two, or a value that is not an http(s) URL,
# is an error, not a fall-through, so a half-filled manual run never mixes its
# URL with a cached one. The pair goes to GITHUB_ENV as INTROSPECTION_CP_URL
# and INTROSPECTION_DP_URL, which the check reads, and its source to
# GITHUB_OUTPUT as source=payload|input|cache.
#
# Exits 2, writing nothing, when no source names both URLs.
#
# usage: scripts/api-reference-urls.sh CACHE_DIR
# env:   PAYLOAD_CP_URL, PAYLOAD_DP_URL  client_payload.cp_url, .dp_url
#        INPUT_CP_URL, INPUT_DP_URL      a manual run's inputs
#        GITHUB_ENV, GITHUB_OUTPUT       optional
set -euo pipefail

cache_dir="${1:?usage: api-reference-urls.sh CACHE_DIR}"
cache_file="${cache_dir}/urls.json"

fail() {
  echo "::error::could not pick the API reference URLs: $1" >&2
  exit 2
}

# An http(s) base URL with nothing that could break out of a line.
checked() {
  local name="$1" value="$2"
  if ! [[ "${value}" =~ ^https?://[^/[:space:]]+[^[:space:]]*$ ]]; then
    fail "${name} is not an http(s) URL: '${value}'"
  fi
  printf '%s' "${value%/}"
}

src="" cp_url="" dp_url=""
trim() {
  local value="${1#"${1%%[![:space:]]*}"}"
  printf '%s' "${value%"${value##*[![:space:]]}"}"
}

# Sets the pair from its first argument, a source, when it names both; one
# alone is an error.
pick() {
  local from="$1" cp dp
  cp="$(trim "$2")"
  dp="$(trim "$3")"
  [ -n "${src}" ] && return 0
  [ -z "${cp}" ] && [ -z "${dp}" ] && return 0
  [ -z "${cp}" ] && fail "the ${from} names no cp_url: pass both or none"
  [ -z "${dp}" ] && fail "the ${from} names no dp_url: pass both or none"
  cp_url="$(checked "${from} cp_url" "${cp}")"
  dp_url="$(checked "${from} dp_url" "${dp}")"
  src="${from}"
}

pick payload "${PAYLOAD_CP_URL:-}" "${PAYLOAD_DP_URL:-}"
pick input "${INPUT_CP_URL:-}" "${INPUT_DP_URL:-}"
if [ -z "${src}" ] && [ -f "${cache_file}" ]; then
  pick cache "$(jq -r '.cp_url // ""' "${cache_file}")" "$(jq -r '.dp_url // ""' "${cache_file}")"
fi
if [ -z "${src}" ]; then
  fail "no staging reference URLs recorded yet: run introspection-cloud's \"API Reference Deployed\" workflow by hand (it dispatches here and refills the cache), or run this workflow by hand with its cp_url and dp_url inputs"
fi

echo "API references from the ${src}: ${cp_url}, ${dp_url}"
if [ "${src}" = payload ]; then
  mkdir -p "${cache_dir}"
  jq -n --arg cp "${cp_url}" --arg dp "${dp_url}" '{cp_url: $cp, dp_url: $dp}' >"${cache_file}"
fi
printf 'INTROSPECTION_CP_URL=%s\nINTROSPECTION_DP_URL=%s\n' "${cp_url}" "${dp_url}" >>"${GITHUB_ENV:-/dev/null}"
echo "source=${src}" >>"${GITHUB_OUTPUT:-/dev/null}"
