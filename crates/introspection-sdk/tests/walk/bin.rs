//! `introspection-walk`: the SDK end-to-end walk as a binary, so a caller can
//! run it without compiling it. Built with `--features walk`; reads the same
//! environment as the `walk` test, runs the walk once, and exits non-zero
//! when it fails or, on replay, finds no complete recording.

#![allow(clippy::result_large_err)]

mod cleanup;
mod har;
mod proxy;
mod scenario;
mod steps;
mod support;

use std::process::ExitCode;

pub use scenario::{set, Walk};

fn main() -> ExitCode {
    // The inline snapshots are compiled in, but on a mismatch insta would
    // write a pending snapshot beside the source it was built from, and it
    // runs `cargo metadata` there to find the workspace. Neither exists where
    // the binary runs, so a mismatch only fails its step, and no setting
    // lets one pass.
    std::env::set_var("INSTA_UPDATE", "no");
    std::env::set_var("INSTA_WORKSPACE_ROOT", std::env::temp_dir());
    std::env::remove_var("INSTA_FORCE_PASS");

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()
        .expect("a tokio runtime");
    let report = std::env::temp_dir().join("walk-report.json");
    match runtime.block_on(scenario::run(&report)) {
        Ok(scenario::Outcome::Passed) => ExitCode::SUCCESS,
        Ok(scenario::Outcome::Skipped(why)) => {
            eprintln!("walk: {why}");
            ExitCode::FAILURE
        }
        Err(failure) => {
            eprintln!("walk: {failure}");
            ExitCode::FAILURE
        }
    }
}
