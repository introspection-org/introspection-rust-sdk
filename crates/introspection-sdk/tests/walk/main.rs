//! The SDK end-to-end walk: one ordered scenario from a clean project to a
//! graded eval, every step an SDK call, replaying the recording the JS walk
//! made against a local stack (introspection-cloud
//! `docs/design/sdk-end-to-end-walk.md`, introspection-js-sdk
//! `tests/walk/walk.test.ts`). The same steps, in the same order, under the
//! same names, so the four SDKs' walks read side by side.
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
mod steps;
mod support;

use std::sync::{Arc, Mutex};

use introspection_sdk::{
    Application, Connector, File, IntrospectionClient, Judge, Project, Recipe, Repository, Runner,
    Runtime, Task,
};

use crate::har::Har;
use crate::proxy::{Backend, Recorder, Replayer};
use crate::support::{recordings_dir, Creds, Mode, Settings, Steps};

/// What the steps hand each other. Each is set by the step that makes it and
/// read by the ones after; a step after a failed one is skipped, so a read
/// of an unset value only happens when the walk itself is wrong.
pub struct Walk {
    pub mode: Mode,
    pub settings: Settings,
    pub backend: Arc<Backend>,
    /// The signed-in owner's Control Plane session.
    pub owner: Creds,
    /// The owner's Data Plane token for the project (`POST /v1/tokens`).
    pub member: Option<Creds>,
    /// The walk's service account, from step 2 on.
    pub sa: Option<Creds>,
    pub project: Option<Project>,
    pub original_description: Option<String>,
    pub application: Option<Application>,
    pub repository: Option<Repository>,
    pub recipe: Option<Recipe>,
    pub pinned_recipe: Option<Recipe>,
    pub connector: Option<Connector>,
    pub runtime: Option<Runtime>,
    pub runtime_v2: Option<Runtime>,
    pub runner: Option<Runner>,
    pub input_file: Option<File>,
    pub task: Option<Task>,
    pub conversation_id: Option<String>,
    pub output_file: Option<File>,
    pub eval_task: Option<Task>,
    pub judge: Option<Judge>,
}

/// An unset step output: the step that sets it did not run.
pub fn set<'a, T>(value: &'a Option<T>, what: &str) -> &'a T {
    value
        .as_ref()
        .unwrap_or_else(|| panic!("{what} is set by an earlier step"))
}

impl Walk {
    pub fn owner(&self) -> IntrospectionClient {
        self.owner.client()
    }

    pub fn member(&self) -> IntrospectionClient {
        set(&self.member, "the member token").client()
    }

    pub fn sa(&self) -> IntrospectionClient {
        set(&self.sa, "the service account").client()
    }

    pub fn sa_quiet(&self) -> IntrospectionClient {
        set(&self.sa, "the service account").quiet()
    }

    pub fn project_id(&self) -> uuid::Uuid {
        set(&self.project, "the project").id
    }
}

/// Every client the SDK builds reaches the walk's proxy: `system-proxy`
/// reads these when a client is built, and the test binary runs this one
/// test, so nothing else shares the environment.
fn route_through(addr: std::net::SocketAddr) {
    let proxy = format!("http://{addr}");
    for key in ["HTTP_PROXY", "http_proxy"] {
        std::env::set_var(key, &proxy);
    }
    for key in ["NO_PROXY", "no_proxy", "ALL_PROXY", "all_proxy"] {
        std::env::remove_var(key);
    }
}

fn backend(mode: Mode, settings: &Settings) -> Option<Backend> {
    let path = recordings_dir().join("recording.har");
    match mode {
        Mode::Replay => {
            if !path.exists() {
                eprintln!("walk: no recording at {}; skipped", path.display());
                return None;
            }
            let har = Har::read(&path);
            // Replay runs only against a recording that reaches the
            // teardown, never a partial one a stopped record run left.
            if !har.is_complete() {
                eprintln!(
                    "walk: {} stops short of the teardown; skipped",
                    path.display()
                );
                return None;
            }
            Some(Backend::Replay(Mutex::new(Replayer::new(
                har,
                &settings.bootstrap_project_id,
            ))))
        }
        Mode::Record => Some(Backend::Record(Recorder::new(path))),
        Mode::Passthrough => Some(Backend::Passthrough(Recorder::new(path))),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sdk_end_to_end_walk() {
    let mode = Mode::from_env();
    if mode == Mode::Replay && !recordings_dir().join("recording.har").exists() {
        eprintln!("walk: no recording; skipped");
        return;
    }
    let settings = Settings::for_mode(mode);
    let Some(backend) = backend(mode, &settings) else {
        return;
    };
    let backend = Arc::new(backend);
    route_through(proxy::serve(backend.clone()).await);
    let owner = Creds {
        base_api_url: settings.base_api_url.clone(),
        dp_url: None,
        // `IntrospectionClient::new` demands a bearer token even when the
        // credential is a CP session cookie, which replaces it on the wire.
        token: "cp-session".into(),
        cp_session: Some(settings.cp_session.clone()),
    };
    let mut walk = Walk {
        mode,
        settings,
        backend: backend.clone(),
        owner,
        member: None,
        sa: None,
        project: None,
        original_description: None,
        application: None,
        repository: None,
        recipe: None,
        pinned_recipe: None,
        connector: None,
        runtime: None,
        runtime_v2: None,
        runner: None,
        input_file: None,
        task: None,
        conversation_id: None,
        output_file: None,
        eval_task: None,
        judge: None,
    };
    let mut steps = Steps::default();
    steps::run(&mut walk, &mut steps).await;
    backend.finish();

    let replayer = match backend.as_ref() {
        Backend::Replay(replayer) => Some(replayer.lock().expect("replayer lock")),
        _ => None,
    };
    let report = support::report(&steps, replayer.as_deref());
    let path = std::env::var_os("WALK_REPORT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("walk-report.json")
        });
    std::fs::write(
        &path,
        serde_json::to_string_pretty(&report).expect("the report") + "\n",
    )
    .expect("write the walk report");
    eprintln!("walk report ({}):\n{report:#}", path.display());

    if let Some(replayer) = &replayer {
        assert!(
            replayer.unmatched.is_empty(),
            "the SDK sent {} request(s) the recording does not hold: {:#?}",
            replayer.unmatched.len(),
            replayer.unmatched
        );
    }
    assert!(
        steps.failed().is_none(),
        "walk step failed: {:?}",
        steps.failed()
    );
}
