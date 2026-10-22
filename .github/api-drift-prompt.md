<!--
The agent prompt that ends each API drift alert in #main-build-alerts.
scripts/api-drift-report.sh fills the {{...}} placeholders and drops this
comment; the rest is pasted verbatim into a coding agent, so write it as
instructions to that agent.
-->

Fix the API drift that last night's API Contract run found in introspection-rust-sdk ({{RUN_URL}}).

Where: the introspection-rust-sdk repository. Fetch origin, create a new branch from origin/main, and work only there.

What: make the SDK match the staging API reference. Fix exactly the failures in the SDK drift report below, and nothing else. The `x-introspection` flags in the reference decide what the SDK must cover: an operation whose `clients` include `sdk` must be called. An operation not called needs an SDK method that calls it, with its types and a test in crates/introspection-sdk/tests that follows AGENTS.md. The reference changelog is context: it says what changed in the API since the saved copies in api-reference/ were taken.

Stale code: when the check reports something the SDK has and the reference no longer does (a route in neither reference), delete it from the SDK. List each deletion in the PR body. If a deletion breaks the SDK's public API, declare it as breaking the way the "Semver checks" section of README.md says.

Finish: when the drift is fixed, refresh both saved copies from staging:

curl -fsS https://api.staging.introspection.dev/openapi/controlplane.json -o api-reference/controlplane.json
curl -fsS https://api.staging.introspection.dev/openapi/dataplane.json -o api-reference/dataplane.json

Stop when the check passes against staging (cargo test -p introspection-sdk --test api_contract_test -- --ignored --nocapture), together with cargo test --all-features and cargo clippy --all-targets --all-features -- -D warnings.

Limits:

- Never edit the reference URLs or the environment variables that override them in crates/introspection-sdk/tests/api_contract_test.rs.
- Never add an exemption, an allowlist, a skip or an exclusion of any kind to the check, and never change the check to make a finding go away. The `x-introspection` flags are the only classification; if one looks wrong, say so in the PR body and leave the finding.
- Never weaken, skip or delete a test to make it pass.

Output: one pull request against main, with conventional commit messages. Its body lists the drift fixed, the stale code deleted, and any flag that looks wrong.

If a report below ends with a trim marker, run the check, or oasdiff changelog against the saved copy, for the full list.

SDK drift (cargo test -p introspection-sdk --test api_contract_test -- --ignored):

{{SDK_DRIFT}}

Reference changelog (oasdiff, api-reference/ saved copy vs staging):

{{CHANGELOG}}
