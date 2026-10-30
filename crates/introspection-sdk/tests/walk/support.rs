//! Support for the SDK end-to-end walk (introspection-cloud
//! `docs/design/sdk-end-to-end-walk.md`), ported from introspection-js-sdk
//! `tests/walk/support.ts`: the fixed names, the target's settings, the
//! normaliser every inline snapshot goes through, the step runner, the one
//! place a route the SDK has no method for is called by hand, and the
//! per-run report the cross-SDK matrix reads.

use std::collections::HashMap;
use std::future::Future;
use std::panic::AssertUnwindSafe;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::{Duration, Instant};

use futures::FutureExt as _;
use introspection_sdk::api::http::{HttpClient, HttpConfig};
use introspection_sdk::api::ApiResult;
use introspection_sdk::{AdvancedOptions, ClientConfig, IntrospectionClient};
use regex::Regex;
use serde::Serialize;
use serde_json::{json, Value};

use crate::proxy::{Replayer, QUIET_HEADER};

/// Fixed names, so a recording matches whatever run replays it. The same
/// values as the JS walk's `WALK`, since every SDK replays its recording.
pub mod walk {
    pub const SIBLING_SUFFIX: &str = "-walk";
    pub const PROJECT_NAME: &str = "SDK end-to-end walk";
    /// `sdk-walk-rust-<unix time, hex>-<random>`: the SDK, so walks of
    /// different SDKs never sweep each other's project, and the time, which
    /// the projects list does not carry, so a sweep spares a run still going.
    pub const PROJECT_SLUG_PREFIX: &str = "sdk-walk-rust-";
    /// A walk takes about a minute; one this old crashed before its teardown.
    pub const STALE_AFTER_SECONDS: u64 = 3600;
    pub const APPLICATION_NAME: &str = "sdk-coverage-walk";
    pub const TAG_NAME: &str = "walk";
    pub const API_KEY_NAME: &str = "walk";
    pub const AUTOMATION_NAME: &str = "The walk's automation";
    pub const EXPERIMENT_NAMES: [&str; 2] =
        ["The walk's experiment", "The walk's cancelled experiment"];
    pub const CREDENTIAL_NAME: &str = "WALK_SECRET";
    pub const CREDENTIAL_SECRET: &str = "walk-secret-value-0001";
    pub const SECOND_TURN: &str = "Reply with the single word done.";
    pub const VARIABLE_NAME: &str = "WALK_MARKER";
    pub const VARIABLE_VALUE: &str = "walk-marker-7f3a";
    pub const CONNECTOR_NAME: &str = "hello-mcp";
    pub const MCP_SERVER_ID: &str = "hello-mcp";
    pub const MCP_ISSUER: &str = "https://example-server.modelcontextprotocol.io/";
    pub const MCP_ENDPOINT: &str = "https://example-server.modelcontextprotocol.io/mcp";
    pub const MCP_HOST: &str = "example-server.modelcontextprotocol.io";
    /// An identity provider a federated application attaches and a service
    /// account refuses (step 2). Checked by its issuer alone.
    pub const IDP_ISSUER: &str = "https://idp.walk.invalid/";
    /// The token a registered connection stores (step 6); no provider issued it.
    pub const REGISTERED_TOKEN: &str = "walk-registered-token";
    /// An image mirror (step 14), digest-pinned as a mirror must be. The copy
    /// may fail on a stack with no egress; the walk does not wait for it.
    pub const IMAGE_NAME: &str = "walk/busybox";
    pub const IMAGE_REFERENCE: &str =
        "docker.io/library/busybox@sha256:9ae97d36d26566ff84e8893c64a6dc4fe8ca6d1144bf5b87b2b85a32def253c7";
    pub const API_ENDPOINT_NAME: &str = "walk-api";
    pub const API_ENDPOINT_HOST: &str = "walk-api.example.com";
    /// Not a secret: a literal header value the binding stores encrypted.
    pub const API_ENDPOINT_HEADER: &str = "walk-header-value-0001";
    pub const RECIPE_SLUG: &str = "hello-mcp";
    pub const RUNTIME_SLUG: &str = "hello-mcp";
    pub const INPUT_FILE_NAME: &str = "walk-input.txt";
    pub const INPUT_FILE_TEXT: &str = "the walk's input file";
    pub const OUTPUT_FILE_NAME: &str = "greeting.txt";
    /// The access step's people, customer members keyed `user:walk-<name>`,
    /// with the tags each holds.
    pub const PERSONAS: [(&str, &[&str]); 4] = [
        ("avery", &["group:platform"]),
        ("blake", &[]),
        ("casey", &["group:platform"]),
        ("emery", &[]),
    ];
    pub const GROUP_TAG: &str = "group:platform";
    pub const ADMIN_NAME: &str = "Drew";
    pub const PERSONA_FILE_NAME: &str = "walk-avery-notes.txt";
    pub const ECHO_MESSAGE: &str = "hello from the public MCP";
    pub const ECHO_RESULT: &str = "Echo: hello from the public MCP";
    pub const INSTRUCTION: &str =
        "Call the `echo` tool from the declared `hello-mcp` MCP server with the message \
`hello from the public MCP`. Write the echoed value exactly, with no quotes or \
additional formatting, to `/workspace/outputs/greeting.txt`. Then run `echo $WALK_MARKER` \
in the shell and reply in one short sentence.";
}

// ----- mode and settings ----------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Record,
    Replay,
    Passthrough,
}

impl Mode {
    /// `WALK_MODE`, or `POLLY_MODE` as the JS walk spells it; replay unless
    /// either says otherwise.
    pub fn from_env() -> Self {
        let raw = std::env::var("WALK_MODE")
            .or_else(|_| std::env::var("POLLY_MODE"))
            .unwrap_or_else(|_| "replay".into());
        match raw.as_str() {
            "record" => Self::Record,
            "replay" => Self::Replay,
            "passthrough" => Self::Passthrough,
            other => panic!("WALK_MODE={other}: must be record, replay or passthrough"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Settings {
    pub base_api_url: String,
    pub cp_session: String,
    /// The login's project: read only for the deployment the walk's own
    /// project is created on.
    pub bootstrap_project_id: String,
    /// The project the walk creates, runs in and deletes: new on every live
    /// run, so each starts empty; replay sends the recorded one.
    pub project_slug: String,
    pub dp_url: Option<String>,
    /// The recipe agent: `scripted` on Compose, `agent` with a real model.
    pub agent: String,
    pub target: String,
    /// Where the recipe is cloned from for the push: a public URL, or a
    /// checkout on disk (`RECIPE_DIR`). Live only.
    pub recipe_repository_url: String,
    pub recipe_git_ref: String,
}

const RECIPE_REPOSITORY_URL: &str = "https://github.com/introspection-org/recipe-harbor-mcp-agent";

/// A `recipe-harbor-mcp-agent` checkout beside this repository: the recipe is
/// private, so cloning it needs GitHub credentials a local checkout does not.
fn sibling_recipe() -> Option<String> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../recipe-harbor-mcp-agent");
    dir.join(".git")
        .exists()
        .then(|| dir.canonicalize().ok())
        .flatten()
        .map(|d| d.display().to_string())
}

pub fn recordings_dir() -> PathBuf {
    std::env::var_os("WALK_RECORDINGS_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/recordings/walk"))
}

pub const BOOTSTRAP_FILE: &str = "bootstrap.json";

impl Settings {
    /// Replay needs nothing but the recorded project id; a live run reads the
    /// `introspection login` credentials file, with the environment over it.
    pub fn for_mode(mode: Mode) -> Self {
        if mode == Mode::Replay {
            let bootstrap: Value = serde_json::from_str(
                &std::fs::read_to_string(recordings_dir().join(BOOTSTRAP_FILE))
                    .expect("the recording's bootstrap.json"),
            )
            .expect("bootstrap.json is JSON");
            return Self {
                base_api_url: "http://localhost:8000".into(),
                cp_session: "replay-session".into(),
                bootstrap_project_id: bootstrap["project_id"]
                    .as_str()
                    .expect("bootstrap.json names the project")
                    .into(),
                project_slug: bootstrap["project_slug"]
                    .as_str()
                    .expect("bootstrap.json names the walk's project")
                    .into(),
                dp_url: Some("http://localhost:8002".into()),
                agent: "scripted".into(),
                target: "compose".into(),
                recipe_repository_url: RECIPE_REPOSITORY_URL.into(),
                recipe_git_ref: "main".into(),
            };
        }
        let file = credentials_file();
        let pick = |env: &str, key: &str| {
            std::env::var(env)
                .ok()
                .or_else(|| file.get(key).and_then(Value::as_str).map(str::to_string))
        };
        let missing =
            "the walk is live and has no Control Plane session: run `introspection login` \
                       or set INTROSPECTION_BASE_API_URL and INTROSPECTION_CP_SESSION";
        Self {
            base_api_url: pick("INTROSPECTION_BASE_API_URL", "cp_url").expect(missing),
            cp_session: pick("INTROSPECTION_CP_SESSION", "cp_session").expect(missing),
            bootstrap_project_id: pick("INTROSPECTION_PROJECT_ID", "project_id").expect(
                "the walk creates its project on the login's deployment: pick a project with \
                 `introspection login` or set INTROSPECTION_PROJECT_ID",
            ),
            project_slug: format!(
                "{}{:x}-{:04x}",
                walk::PROJECT_SLUG_PREFIX,
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .expect("the clock is past the epoch")
                    .as_secs(),
                fastrand::u16(..)
            ),
            dp_url: pick("INTROSPECTION_DP_URL", "dp_url"),
            agent: std::env::var("WALK_AGENT").unwrap_or_else(|_| "scripted".into()),
            target: std::env::var("WALK_TARGET").unwrap_or_else(|_| "compose".into()),
            recipe_repository_url: std::env::var("RECIPE_DIR")
                .or_else(|_| std::env::var("RECIPE_REPOSITORY_URL"))
                .unwrap_or_else(|_| {
                    sibling_recipe().unwrap_or_else(|| RECIPE_REPOSITORY_URL.into())
                }),
            recipe_git_ref: std::env::var("RECIPE_GIT_REF").unwrap_or_else(|_| "main".into()),
        }
    }

    /// The prompt for the target's agent: the scripted model is told to do
    /// the same three things a real model does, so the judge grades both.
    pub fn prompt(&self) -> String {
        if self.agent != "scripted" {
            return walk::INSTRUCTION.into();
        }
        let directive = json!({
            "reply": "Echoed the message through hello-mcp and wrote it to the outputs file.",
            "tools": [
                { "name": "hello-mcp_echo", "arguments": { "message": walk::ECHO_MESSAGE } },
                {
                    "name": "write",
                    "arguments": {
                        "path": format!("/workspace/outputs/{}", walk::OUTPUT_FILE_NAME),
                        "content": walk::ECHO_RESULT,
                    },
                },
                { "name": "bash", "arguments": { "command": "echo $WALK_MARKER" } },
            ],
        });
        format!(
            "{}\n\n<<SCRIPT {}>>",
            walk::INSTRUCTION,
            js_json(&directive)
        )
    }

    /// A second turn that needs no tool: the scripted agent is told its reply.
    pub fn second_turn(&self) -> String {
        if self.agent != "scripted" {
            return walk::SECOND_TURN.into();
        }
        format!(
            "{} <<SCRIPT {}>>",
            walk::SECOND_TURN,
            js_json(&json!({ "reply": "done" }))
        )
    }
}

/// `JSON.stringify` key order for the directives above: insertion order,
/// which `serde_json` without `preserve_order` does not keep.
fn js_json(value: &Value) -> String {
    match value {
        Value::Object(map) => {
            let order = [
                "reply",
                "tools",
                "name",
                "arguments",
                "message",
                "path",
                "content",
                "command",
            ];
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort_by_key(|k| order.iter().position(|o| o == k).unwrap_or(usize::MAX));
            let fields: Vec<String> = keys
                .into_iter()
                .map(|k| format!("{}:{}", Value::String(k.clone()), js_json(&map[k])))
                .collect();
            format!("{{{}}}", fields.join(","))
        }
        Value::Array(items) => {
            let items: Vec<String> = items.iter().map(js_json).collect();
            format!("[{}]", items.join(","))
        }
        other => other.to_string(),
    }
}

fn credentials_file() -> serde_json::Map<String, Value> {
    let path = std::env::var_os("INTROSPECTION_CREDENTIALS")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .map(|home| PathBuf::from(home).join(".introspection/credentials.json"))
        });
    path.and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

// ----- clients --------------------------------------------------------------

/// A credential and the planes it talks to, from which the walk builds the
/// SDK client and its quiet twin (the polls a recording never sees). There
/// is no raw HTTP client: a route the SDK has no method for gets one first.
#[derive(Debug, Clone)]
pub struct Creds {
    pub base_api_url: String,
    pub dp_url: Option<String>,
    pub token: String,
    pub cp_session: Option<String>,
}

impl Creds {
    fn advanced(&self, quiet: bool) -> AdvancedOptions {
        let mut headers = HashMap::new();
        if quiet {
            headers.insert(QUIET_HEADER.to_string(), "1".to_string());
        }
        AdvancedOptions {
            base_api_url: Some(self.base_api_url.clone()),
            dp_url: self.dp_url.clone(),
            cp_session: self.cp_session.clone(),
            additional_headers: Some(headers),
        }
    }

    pub fn client(&self) -> IntrospectionClient {
        IntrospectionClient::new(
            ClientConfig::with_token(&self.token).advanced(self.advanced(false)),
        )
        .expect("the walk's client builds")
    }

    pub fn quiet(&self) -> IntrospectionClient {
        IntrospectionClient::new(
            ClientConfig::with_token(&self.token).advanced(self.advanced(true)),
        )
        .expect("the walk's quiet client builds")
    }

    /// A bare Data Plane client on this token, the kind
    /// `EmailCodeAuth::data_plane` hands out: no Control Plane behind it.
    pub fn dp_http(&self) -> std::sync::Arc<HttpClient> {
        std::sync::Arc::new(
            HttpClient::new(HttpConfig {
                api_url: self
                    .dp_url
                    .clone()
                    .unwrap_or_else(|| self.base_api_url.clone()),
                token: self.token.clone(),
                additional_headers: HashMap::new(),
                timeout: Duration::from_secs(30),
                max_retries: 0,
                retry_base: Duration::from_millis(500),
            })
            .expect("the walk's Data Plane client builds"),
        )
    }
}

// ----- polling ----------------------------------------------------------------

/// Poll `poll` (a quiet client) until `done`, then `read` once through the
/// recorded client: that one read, in the resource's terminal state, is what
/// the recording carries (design doc §5). In replay the read is all there is.
pub async fn settled<T, R, RF, P, PF, D>(
    mode: Mode,
    label: &str,
    timeout: Duration,
    read: R,
    poll: P,
    done: D,
) -> T
where
    R: Fn() -> RF,
    RF: Future<Output = ApiResult<T>>,
    P: Fn() -> PF,
    PF: Future<Output = ApiResult<T>>,
    D: Fn(&T) -> bool,
{
    if mode != Mode::Replay {
        let deadline = Instant::now() + timeout;
        loop {
            // A conversation reads 404 until its first spans are ingested: not
            // yet, rather than gone.
            match poll().await {
                Ok(value) if done(&value) => break,
                Ok(_) => {}
                Err(e) if e.status() == Some(404) => {}
                Err(e) => panic!("{label}: poll failed: {e}"),
            }
            assert!(
                Instant::now() < deadline,
                "{label}: not done after {timeout:?}"
            );
            tokio::time::sleep(Duration::from_secs(3)).await;
        }
    }
    let value = read().await.unwrap_or_else(|e| panic!("{label}: {e}"));
    assert!(
        done(&value),
        "{label}: the recorded read is not in its terminal state"
    );
    value
}

// ----- normalisation ----------------------------------------------------------

fn re(pattern: &str) -> Regex {
    Regex::new(pattern).expect("valid regex")
}

static UUID: LazyLock<Regex> =
    LazyLock::new(|| re(r"(?i)^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$"));
static TIMESTAMP: LazyLock<Regex> =
    LazyLock::new(|| re(r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(\.\d+)?(Z|[+-]\d{2}:\d{2})$"));
static SHA: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)^[0-9a-f]{40}$"));
static UUID_ANYWHERE: LazyLock<Regex> =
    LazyLock::new(|| re(r"(?i)[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}"));
static SHA_ANYWHERE: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)\b[0-9a-f]{40}\b"));
static DIGEST_ANYWHERE: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)\b[0-9a-f]{64}\b"));
static JWT: LazyLock<Regex> =
    LazyLock::new(|| re(r"^eyJ[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+$"));
static URL: LazyLock<Regex> = LazyLock::new(|| re(r"^https?://"));
static DIGITS: LazyLock<Regex> = LazyLock::new(|| re(r"\d"));
static MEASURED_KEY: LazyLock<Regex> =
    LazyLock::new(|| re(r"(_ms$|_count$|_tokens$|^max_turn$|^display_index$)"));
static VOLATILE_KEY: LazyLock<Regex> = LazyLock::new(|| {
    re(
        r"(^id$|_at$|_sha$|token$|secret$|^key$|_key$|cookie$|^url$|_url$|^expires_in$|^client_id$|_prefix$|_suffix$|^prefix$|^key_display$|image_tag$|_image_name$|_bytes$)",
    )
});

/// Everything stable, nothing volatile: ids, timestamps, hashes, tokens,
/// secrets and URLs become a placeholder naming their kind, by key and by
/// shape, so an inline snapshot pins every other field. A faithful port of
/// the JS walk's `stable()`; the placeholders are the cross-SDK convention.
pub fn stable(value: &Value, key: &str) -> Value {
    match value {
        Value::Array(items) => Value::Array(items.iter().map(|v| stable(v, "")).collect()),
        Value::Object(map) => {
            Value::Object(map.iter().map(|(k, v)| (k.clone(), stable(v, k))).collect())
        }
        Value::String(s) => Value::String(stable_string(s, key)),
        // A measured number (a duration, a span or call count, a token
        // total, a task's per-project index) moves between two runs; its bounds are
        // asserted beside the snapshot instead.
        Value::Number(_) if VOLATILE_KEY.is_match(key) || MEASURED_KEY.is_match(key) => {
            Value::String("[number]".into())
        }
        other => other.clone(),
    }
}

fn stable_string(s: &str, key: &str) -> String {
    let placeholder = if UUID.is_match(s) {
        "[uuid]"
    } else if TIMESTAMP.is_match(s) {
        "[timestamp]"
    } else if SHA.is_match(s) {
        "[sha]"
    } else if JWT.is_match(s) {
        "[jwt]"
    } else if URL.is_match(s) {
        "[url]"
    } else if VOLATILE_KEY.is_match(key) {
        "[redacted]"
    } else if key.ends_with("_id") && DIGITS.is_match(s) {
        // An identifier under an `_id` key that is not a UUID (a Restate
        // invocation, a provider response id) is volatile when it carries
        // digits; a declared name such as `mcp_server_id: hello-mcp` stays.
        "[id]"
    } else {
        // An id or a hash inside a longer value (a storage path, an image
        // tag, a digest) is volatile too; the text around it stays.
        let s = UUID_ANYWHERE.replace_all(s, "[uuid]");
        let s = DIGEST_ANYWHERE.replace_all(&s, "[digest]");
        return SHA_ANYWHERE.replace_all(&s, "[sha]").into_owned();
    };
    placeholder.into()
}

/// [`stable`] as an insta redaction. Every snapshot in the walk applies it
/// at the selector `.**` (each top-level field, and through it everything
/// beneath), so the normalisation rule lives here and nowhere else.
pub fn stable_redaction(
    content: insta::internals::Content,
    path: insta::internals::ContentPath<'_>,
) -> insta::internals::Content {
    let path = path.to_string();
    let key = path.rsplit('.').next().unwrap_or_default();
    let value = serde_json::to_value(&content).expect("snapshot content is JSON");
    to_content(stable(&value, key))
}

fn to_content(value: Value) -> insta::internals::Content {
    use insta::internals::Content;
    match value {
        Value::Null => Content::None,
        Value::Bool(b) => Content::Bool(b),
        Value::Number(n) => n
            .as_u64()
            .map(Content::U64)
            .or_else(|| n.as_i64().map(Content::I64))
            .unwrap_or_else(|| Content::F64(n.as_f64().unwrap_or_default())),
        Value::String(s) => Content::String(s),
        Value::Array(items) => Content::Seq(items.into_iter().map(to_content).collect()),
        Value::Object(map) => Content::Map(
            map.into_iter()
                .map(|(k, v)| (Content::String(k), to_content(v)))
                .collect(),
        ),
    }
}

/// `assert_json_snapshot!` through the walk's one normalisation rule.
/// Extra selectors (a field the walk leaves out, such as a task's platform
/// `metadata`) are redacted first.
macro_rules! assert_stable {
    ($value:expr, @$snapshot:literal) => {
        insta::assert_json_snapshot!(serde_json::to_value(&$value).expect("snapshot JSON"), {
            ".**" => insta::dynamic_redaction($crate::support::stable_redaction),
        }, @$snapshot)
    };
    ($value:expr, { $($selector:expr => $replacement:expr),+ $(,)? }, @$snapshot:literal) => {
        insta::assert_json_snapshot!(serde_json::to_value(&$value).expect("snapshot JSON"), {
            $($selector => $replacement,)+
            ".**" => insta::dynamic_redaction($crate::support::stable_redaction),
        }, @$snapshot)
    };
}
pub(crate) use assert_stable;

// ----- steps and the report -----------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct StepResult {
    pub n: u32,
    pub name: String,
    pub status: &'static str,
}

/// The walk's steps share state in order; a step after a failed one skips
/// rather than failing on its missing input, so one failure reads as one.
#[derive(Default)]
pub struct Steps {
    pub results: Vec<StepResult>,
    failed: Option<String>,
}

impl Steps {
    pub async fn run<F: Future<Output = ()>>(&mut self, n: u32, name: &str, step: F) {
        if let Some(failed) = &self.failed {
            eprintln!("walk step {n} ({name}): skipped after {failed} failed");
            self.push(n, name, "skipped");
            return;
        }
        match AssertUnwindSafe(step).catch_unwind().await {
            Ok(()) => self.push(n, name, "passed"),
            Err(_) => {
                self.failed = Some(name.to_string());
                self.push(n, name, "failed");
            }
        }
    }

    pub fn skip(&mut self, n: u32, name: &str, why: &str) {
        eprintln!("walk step {n} ({name}): skipped: {why}");
        self.push(n, name, "skipped");
    }

    fn push(&mut self, n: u32, name: &str, status: &'static str) {
        self.results.push(StepResult {
            n,
            name: name.to_string(),
            status,
        });
    }

    pub fn failed(&self) -> Option<&str> {
        self.failed.as_deref()
    }
}

/// The per-run report, in the shape every SDK's walk writes.
pub fn report(steps: &Steps, replayer: Option<&Replayer>) -> Value {
    let (total, consumed, unconsumed) = match replayer {
        Some(r) => (
            r.total(),
            r.consumed(),
            r.unconsumed()
                .into_iter()
                .map(|(method, url)| json!({ "method": method, "url": url }))
                .collect(),
        ),
        None => (0, 0, Vec::new()),
    };
    json!({
        "sdk": "rust",
        "steps": steps.results,
        "entries": { "total": total, "consumed": consumed },
        "unconsumed": unconsumed,
        // The walk has no raw HTTP client, so every route goes through an
        // SDK method; the field stays for the cross-SDK report shape.
        "raw_calls": [],
    })
}

/// A UUIDv7-shaped id derived from the span an event annotates: the same on
/// every replay (the span is recorded), new on every live run, where a fixed
/// id would collide with the receipt a previous run left in the project.
pub fn span_event_id(trace_id: &str, span_id: &str, label: &str) -> uuid::Uuid {
    let digest = ring::digest::digest(
        &ring::digest::SHA256,
        format!("{trace_id}:{span_id}:{label}").as_bytes(),
    );
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest.as_ref()[..16]);
    bytes[6] = (bytes[6] & 0x0F) | 0x70;
    bytes[8] = (bytes[8] & 0x3F) | 0x80;
    uuid::Uuid::from_bytes(bytes)
}

/// When a walk project was created, from its slug (see
/// `walk::PROJECT_SLUG_PREFIX`); `None` for anyone else's project.
pub fn walk_project_born(slug: &str) -> Option<u64> {
    let rest = slug.strip_prefix(walk::PROJECT_SLUG_PREFIX)?;
    u64::from_str_radix(rest.split('-').next()?, 16).ok()
}
