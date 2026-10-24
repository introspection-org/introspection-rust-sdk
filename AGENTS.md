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

## The end-to-end walk

- `crates/introspection-sdk/tests/walk` is the SDK end-to-end walk (introspection-cloud `docs/design/sdk-end-to-end-walk.md`): the design doc's steps, in its order, replaying `tests/recordings/walk/recording.har` through a local forward proxy every SDK client reaches via `HTTP_PROXY`. It needs the `arrow` feature, so it runs under CI's `cargo test --all-features`.
- The recording is this SDK's own, recorded against Compose (below); don't edit the HAR by hand, and take nothing from another SDK's.
- A request the recording does not hold fails the step with the method, URL and a diff against the closest recorded requests. Fix the SDK or the step; widen the matcher only for a documented equivalence (the lists in `tests/walk/har.rs`).
- Snapshots are insta inline snapshots through `assert_stable!`, whose one redaction mirrors the JS walk's `stable()`. Update them with `cargo insta test -p introspection-sdk --all-features --test walk --accept`.
- A route the SDK has no method for is called through `support::Raw` with a `// todo:` and shows up in the run's report (`target/tmp/walk-report.json`). Don't add an SDK method from a walk PR.
- Re-record against Compose with `WALK_MODE=record cargo test -p introspection-sdk --all-features --test walk` and a signed-in `introspection login`; record mode proxies plain `http` only, which is what Compose serves.
