//! The SDK end-to-end walk: one ordered scenario from a clean project to a
//! graded eval, every step an SDK call, replaying this SDK's own recording
//! against a local stack (introspection-cloud
//! `docs/design/sdk-end-to-end-walk.md`). The design doc's steps, in its
//! order, under its names, so the four SDKs' walks read side by side.
//!
//! Replay (the default) needs no stack, no keys and no network:
//!
//!   cargo test --all-features --test walk
//!
//! A live run talks to a signed-in `introspection login` (or
//! `INTROSPECTION_BASE_API_URL` + `INTROSPECTION_CP_SESSION` +
//! `INTROSPECTION_PROJECT_ID`) on a Compose stack:
//!
//!   WALK_MODE=record cargo test --all-features --test walk        # re-record
//!   WALK_MODE=passthrough cargo test --all-features --test walk   # live, records nothing

// The walk's helpers return the SDK's own `ApiResult` so a step sees the
// public error contract; boxing it to shrink them would change what they see.
#![allow(clippy::result_large_err)]

mod cleanup;
mod har;
mod proxy;
mod scenario;
mod steps;
mod support;

pub use scenario::{set, Walk};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sdk_end_to_end_walk() {
    let report = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("walk-report.json");
    match scenario::run(&report).await {
        Ok(scenario::Outcome::Passed) => {}
        Ok(scenario::Outcome::Skipped(why)) => eprintln!("walk: {why}; skipped"),
        Err(failure) => panic!("{failure}"),
    }
}
