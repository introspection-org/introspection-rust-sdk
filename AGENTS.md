# AGENTS.md

Notes for contributors and coding agents. For what the SDK does and how to use it, read [README.md](README.md).

## Layout and releases

- The repo is a Cargo workspace (since #124). The SDK lives in `crates/introspection-sdk`; `crates/aauth-protocol` sits beside it.
- release-please manages two packages: `introspection-sdk` and `aauth-protocol`. See `release-please-config.json`.
- SDK tags stay `vX.Y.Z` (`include-component-in-tag: false`). `aauth-protocol` gets component-prefixed tags.
- Don't set `component` on `crates/introspection-sdk`. A release PR that bumps only the SDK has one bare `<summary>X.Y.Z</summary>` entry, and release-please tags it only when the SDK's branch component is empty. Its tag never uses the component, so the key would only stop the tagging.

## CI

- The last job in `.github/workflows/ci.yml`, `All green` (id `all-green`), fails when any job in its `needs` failed, was cancelled or was skipped.
- Add every new `ci.yml` job to `all-green`'s `needs`, or `All green` passes without it.
- List a job in `allowed-skips` only when it skips by design, like `semver-checks`, which runs on pull requests only. A job skipped because a job it needs failed must still fail `All green`.
- CI runs `cargo clippy --all-targets --all-features -- -D warnings` at the workspace root. That compiles every member's examples, so a broken example fails CI.
- CI never runs the examples. They need a live server.

## Examples

- Cargo auto-discovers examples only at `crates/introspection-sdk/examples/*.rs`. A file under `examples/api/` needs an `[[example]]` entry in `crates/introspection-sdk/Cargo.toml`.
- An example that reads a TaskRuns stream (`run.stream()`) must match `Ok(AgUiEvent::RunError(_))` explicitly. The stream yields a run error as `Ok` and then ends, so `event?` alone misses it. Codex flags this in review.
- A docs region (`// #region docs:...`) must show every helper it calls that the example file defines. Put SDK-work helpers (stream parsing, snapshot recovery, text accumulation) inside the region, before the code that uses them. Stand in for the reader's own code (rendering, sending events to the browser, storing records) with a minimal stub inside the region, with a one-line comment saying it is your app's code.

## Decoding never fails on what the server adds

The API grows without asking the SDK first, so a response that gains something still decodes for every user:

- **A new field is ignored.** Never `#[serde(deny_unknown_fields)]` on a response type; keep extras in a `#[serde(flatten)]` map only where callers read them.
- **A new enum value or union variant decodes.** An enum in a response, a plain string enum or a tagged union alike, ends in `#[serde(untagged)] Unknown(serde_json::Value)`, as `Event` does: what the server sent is kept and serialises back unchanged. It is a plain derived enum; there is no macro for it. A string enum callers read or build by its wire value also gets `impl WireEnum for X {}`, which gives `wire()` and `from_wire()` through serde.
- `strict-schema` (the walk and the tests) is the only place an unmodelled field fails, so drift is found before a release; model it rather than loosen that check.

## The end-to-end walk

- `crates/introspection-sdk/tests/walk` is the SDK end-to-end walk (introspection-cloud `docs/design/sdk-end-to-end-walk.md`): the design doc's steps, in its order, replaying `tests/recordings/walk/recording.har` through a local forward proxy every SDK client reaches via `HTTP_PROXY`. It needs the `arrow` feature, so it runs under CI's `cargo test --all-features`.
- `--features walk` also builds the walk as the `introspection-walk` binary (`tests/walk/bin.rs`). Each release tag pushes it (debug, stripped) to `ghcr.io/introspection-org/introspection-rust-walk` as `<version>` and `latest` (`walk.yml`), which introspection-cloud's SDK walk job pulls instead of compiling the walk. The test and the binary both call `scenario::run`, so a crate the walk starts using goes in the `walk` feature's optional dependencies as well as in the dev-dependencies.
- The recording is this SDK's own, recorded by `scripts/record-walk.sh` (below); don't edit the HAR by hand, and take nothing from another SDK's.
- A request the recording does not hold fails the step with the method, URL and a diff against the closest recorded requests. Fix the SDK or the step; widen the matcher only for a documented equivalence (the lists in `tests/walk/har.rs`).
- Snapshots are insta inline snapshots through `assert_stable!`, whose one redaction mirrors the JS walk's `stable()`. Update them with `cargo insta test -p introspection-sdk --all-features --test walk --accept`.
- `--all-features` turns on `strict-schema`, so a response field the SDK does not model fails the step with its JSON path. Model the field; don't drop it from the recording. `tests/round_trip_test.rs` fails on it too.
- The walk calls every route through an SDK method. It has no raw HTTP client, so a raw call does not compile, and the report's `raw_calls` (`target/tmp/walk-report.json`) is always empty. A route the SDK has no method for needs a typed method first. A quiet poll goes through `Creds::quiet()`, the SDK client that marks its requests quiet.
- Re-record with `scripts/record-walk.sh [--cloud-ref <ref, default main>] [--sdk-ref <branch, default the current branch>]`. It dispatches introspection-cloud's `sdk-walks.yml` (CI: SDK Walks, `sdk_refs=rust=<sdk-ref>`, its only input), which walks the SDK branch against a Compose stack built from the cloud branch in CI, and copies `recording.har` and `bootstrap.json` from the run's `sdk-coverage` artifact into `tests/recordings/walk/`. Both branches are pinned, so don't record against a local Compose stack with `introspection login`: a local stack and login can point anywhere, and never record against staging.
- Run the script from a checkout of this repository. It uses the checkout around the current directory, and otherwise the one that holds the script, so a copy of the script works from inside any checkout.
- The run's `sdk-coverage` artifact stays in `target/record-walk/<run id>/`, and the script prints that path. For a failed walk, read `walk-rust.log` there (a snapshot diff follows its `Snapshot Summary` line) and `walk-report-rust.json`.
- The SDK branch must be pushed and current, since the workflow checks it out from GitHub. The script waits until no SDK Walks run is queued or running (a dispatch cancels a running one), and with `WALK_RECORD_LOCK` set takes that directory as a lock. It judges the walk by `walk-report-rust.json`, not by the run's conclusion, and fails with the walk log when a step failed: the run walks every SDK and gates coverage floors, so a red run can hold a green rust walk, and a walk named in the cloud repository variable `SDK_WALKS_ADVISORY` does not fail the run, so a green run can hide a failed walk.
- Then replay with `cargo test -p introspection-sdk --all-features --test walk` and commit. Commit a re-recording only when it changes more than volatile values (ids, timestamps, tokens).
- The Python and Swift SDKs copy the script; only the settings at its top (language, artifact and recording paths, replay command) change.
