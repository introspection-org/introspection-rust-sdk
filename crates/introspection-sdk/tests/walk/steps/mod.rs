//! The walk's steps, numbered and named as in the JS walk.

mod access;
mod config;
mod eval;
mod finish;
mod setup;
mod work;

use std::process::Command;

use futures::future::{FutureExt as _, LocalBoxFuture};
use introspection_sdk::api::ApiResult;

use crate::support::{Settings, Steps};
use crate::Walk;

type Step = for<'a> fn(&'a mut Walk) -> LocalBoxFuture<'a, ()>;

macro_rules! step {
    ($n:literal, $name:literal, $f:path) => {
        ($n, $name, |w| $f(w).boxed_local())
    };
}

/// The JS walk's steps, in its order and under its names.
const STEPS: [(u32, &str, Step); 19] = [
    step!(0, "clean slate", setup::clean_slate),
    step!(1, "project", setup::project),
    step!(2, "application", setup::application),
    step!(3, "repository", setup::repository),
    step!(4, "recipe", setup::recipe),
    step!(
        5,
        "credential and variable",
        config::credential_and_variable
    ),
    step!(6, "connector", config::connector),
    step!(7, "runtime", config::runtime),
    step!(
        8,
        "connection and endpoint",
        config::connection_and_endpoint
    ),
    step!(9, "files in", work::files_in),
    step!(10, "task", work::task),
    step!(11, "evidence", work::evidence),
    step!(13, "telemetry reads", work::telemetry_reads),
    step!(14, "eval", eval::eval),
    step!(15, "commands", eval::commands),
    step!(16, "automations", eval::automations),
    step!(17, "access", access::access),
    step!(18, "members and keys", finish::members_and_keys),
    step!(19, "teardown", finish::teardown),
];

pub async fn run(w: &mut Walk, steps: &mut Steps) {
    for (n, name, step) in STEPS {
        if n == 13 {
            // The dev lane drives `introspection dev` through the CLI, which
            // the recording cannot see (design doc §5); the JS walk skips it.
            steps.skip(12, "the dev lane", "CLI-only, not recorded");
        }
        steps.run(n, name, step(w)).await;
    }
}

/// The read answers `404` through the SDK's typed error.
pub fn expect_not_found<T: std::fmt::Debug>(result: ApiResult<T>) {
    match result {
        Err(err) => assert_eq!(err.status(), Some(404), "expected a 404, got {err}"),
        Ok(value) => panic!("expected a 404, read {value:?}"),
    }
}

/// Push the recipe to the walk's hosted `main`. Live only: the git transport
/// is a subprocess the recording cannot see. The bearer rides the
/// environment as git config, never argv.
pub fn push_recipe(settings: &Settings, url: &str, bearer: &str) {
    let dir = std::env::temp_dir().join(format!("sdk-walk-recipe-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let git = |args: &[&str], auth: bool| {
        let mut cmd = Command::new("git");
        cmd.args(args).env("GIT_TERMINAL_PROMPT", "0");
        if auth {
            cmd.env("GIT_CONFIG_COUNT", "2")
                .env("GIT_CONFIG_KEY_0", "http.extraHeader")
                .env(
                    "GIT_CONFIG_VALUE_0",
                    format!("Authorization: Bearer {bearer}"),
                )
                .env("GIT_CONFIG_KEY_1", "credential.helper")
                .env("GIT_CONFIG_VALUE_1", "");
        }
        let out = cmd.output().expect("run git");
        let hint = if args.first() == Some(&"clone") {
            "; set RECIPE_DIR to a recipe-harbor-mcp-agent checkout, or keep one beside this repository"
        } else {
            ""
        };
        assert!(
            out.status.success(),
            "git {args:?} failed: {}{hint}",
            String::from_utf8_lossy(&out.stderr)
        );
    };
    let dir_str = dir.to_string_lossy().to_string();
    git(
        &[
            "clone",
            "--quiet",
            "--depth",
            "1",
            "--branch",
            &settings.recipe_git_ref,
            &settings.recipe_repository_url,
            &dir_str,
        ],
        false,
    );
    // Provisioning seeds the hosted copy with a first commit; the walk's
    // recipe replaces it, so the push is forced on this fresh repository.
    git(
        &[
            "-C",
            &dir_str,
            "push",
            "--quiet",
            "--force",
            url,
            "HEAD:refs/heads/main",
        ],
        true,
    );
    let _ = std::fs::remove_dir_all(&dir);
}
