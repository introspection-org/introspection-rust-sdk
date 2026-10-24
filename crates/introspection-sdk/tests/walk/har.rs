//! The walk's cassette: HAR 1.2 as Polly writes it, read and written here so
//! every SDK's recording goes through one parser (design doc §9), plus the
//! request identity replay matches on and the scrubbing record applies.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;
use std::sync::LazyLock;

use base64::Engine as _;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Har {
    pub log: Log,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Log {
    pub version: String,
    pub creator: Value,
    #[serde(default)]
    pub pages: Vec<Value>,
    pub entries: Vec<Entry>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub started_date_time: String,
    pub time: f64,
    pub request: Request,
    pub response: Response,
    #[serde(default)]
    pub cache: Value,
    #[serde(default)]
    pub timings: Value,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NameValue {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Request {
    pub method: String,
    pub url: String,
    #[serde(default)]
    pub http_version: String,
    #[serde(default)]
    pub headers: Vec<NameValue>,
    #[serde(default)]
    pub query_string: Vec<NameValue>,
    #[serde(default)]
    pub cookies: Vec<Value>,
    #[serde(default)]
    pub headers_size: i64,
    #[serde(default)]
    pub body_size: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub post_data: Option<PostData>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PostData {
    #[serde(default)]
    pub mime_type: String,
    #[serde(default)]
    pub params: Vec<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Response {
    pub status: u16,
    #[serde(default)]
    pub status_text: String,
    #[serde(default)]
    pub http_version: String,
    #[serde(default)]
    pub headers: Vec<NameValue>,
    #[serde(default)]
    pub cookies: Vec<Value>,
    pub content: Content,
    #[serde(default, rename = "redirectURL")]
    pub redirect_url: String,
    #[serde(default)]
    pub headers_size: i64,
    #[serde(default)]
    pub body_size: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Content {
    #[serde(default)]
    pub mime_type: String,
    #[serde(default)]
    pub size: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encoding: Option<String>,
}

impl Har {
    pub fn read(path: &Path) -> Self {
        let text = std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
        serde_json::from_str(&text)
            .unwrap_or_else(|e| panic!("{} is not a HAR 1.2 log: {e}", path.display()))
    }

    pub fn new(entries: Vec<Entry>) -> Self {
        Self {
            log: Log {
                version: "1.2".into(),
                creator: serde_json::json!({
                    "name": "introspection-rust-sdk walk",
                    "version": env!("CARGO_PKG_VERSION"),
                }),
                pages: Vec::new(),
                entries,
                extra: Map::new(),
            },
        }
    }

    /// Whether the log reaches the walk's end: the teardown reads the
    /// application it removed (`404`), then the login's project. A record run
    /// that stopped short leaves a partial log, which replay must not take for
    /// the contract.
    pub fn is_complete(&self) -> bool {
        let entries = &self.log.entries;
        let [.., app, last] = entries.as_slice() else {
            return false;
        };
        app.response.status == 404
            && app.request.url.contains("/v1/applications/")
            && last.request.method == "GET"
            && PROJECT_URL.is_match(&last.request.url)
    }
}

impl Response {
    /// The body as the server sent it: HAR keeps binary content base64'd.
    pub fn body(&self) -> Vec<u8> {
        let text = self.content.text.clone().unwrap_or_default();
        if self.content.encoding.as_deref() == Some("base64") {
            return base64::engine::general_purpose::STANDARD
                .decode(text.as_bytes())
                .expect("a base64 HAR body decodes");
        }
        text.into_bytes()
    }
}

static PROJECT_URL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"/v1/projects/[^/?]+$").expect("valid regex"));

// ----- scrubbing ------------------------------------------------------------

/// The same list, in the same order, as introspection-js-sdk
/// `tests/polly-setup.ts` `SECRET_PATTERNS`, with the same placeholders, so a
/// secret reads the same in every SDK's recording.
static SECRET_PATTERNS: LazyLock<Vec<(Regex, &'static str)>> = LazyLock::new(|| {
    [
        (r"sk-proj-[A-Za-z0-9_-]{20,}", "REDACTED_OPENAI_KEY"),
        (r"AIza[A-Za-z0-9_-]{35}", "REDACTED_GOOGLE_KEY"),
        (
            r"sk-ant-api\d+-[A-Za-z0-9_-]{20,}",
            "REDACTED_ANTHROPIC_KEY",
        ),
        (
            r"intro_dev_[A-Za-z0-9_-]{20,}",
            "REDACTED_INTROSPECTION_TOKEN",
        ),
        (
            r"eyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}",
            "REDACTED_JWT",
        ),
        (
            r"intro_[a-z]+_[A-Za-z0-9_-]{20,}",
            "REDACTED_INTROSPECTION_KEY",
        ),
        (
            r#"("(?:client_secret|secret|api_key)"\s*:\s*")[^"]+(")"#,
            "${1}REDACTED_SECRET${2}",
        ),
        (r"(client_secret=)[^&\s]+", "${1}REDACTED_SECRET"),
    ]
    .into_iter()
    .map(|(re, to)| (Regex::new(re).expect("valid secret pattern"), to))
    .collect()
});

pub fn scrub_secrets(text: &str) -> String {
    let mut out = text.to_string();
    for (re, to) in SECRET_PATTERNS.iter() {
        out = re.replace_all(&out, *to).into_owned();
    }
    out
}

const SENSITIVE_HEADERS: &[&str] = &[
    "authorization",
    "api-key",
    "api_key",
    "x-api-key",
    "cookie",
    "set-cookie",
];

static SENSITIVE_HEADER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)(authorization|api[-_]?key|access[-_]?token|refresh[-_]?token|\btoken\b|secret|password|credential|cookie|x-goog-user-project|session)",
    )
    .expect("valid regex")
});

pub fn scrub_header(name: &str, value: &str) -> String {
    let lower = name.to_ascii_lowercase();
    if SENSITIVE_HEADERS.contains(&lower.as_str()) || SENSITIVE_HEADER.is_match(&lower) {
        "REDACTED".into()
    } else {
        value.into()
    }
}

// ----- matching -------------------------------------------------------------

/// Body fields the Rust SDK always sends where the JS SDK leaves the server's
/// default, with the default as their value:
///
/// - `MemberCreateParams` is tagged on `member_type`, so an invite carries
///   `member_type: business`, which is what the CP assumes without one.
static DEFAULTED_FIELDS: LazyLock<Vec<(&'static str, Regex, &'static str, &'static str)>> =
    LazyLock::new(|| {
        vec![(
            "POST",
            Regex::new(r"^/v1/members$").expect("valid route pattern"),
            "member_type",
            "business",
        )]
    });

/// Routes where the Rust SDK and the JS SDK disagree only on naming the
/// project in `?project=`, which a project-scoped token makes redundant: the
/// value the one side sends is the walk's own project, the one the token is
/// scoped to. Each is an SDK difference the PR lists, kept to the route.
///
/// - `ExperimentHandle::{start, end, cancel, run}`, `Experiments::get` and
///   `Experiments::update` always send it; the JS SDK's `*ById` calls never do.
/// - `Connectors::delete` takes no project; the JS SDK sends it.
static PROJECT_OPTIONAL: LazyLock<Vec<(&'static str, Regex)>> = LazyLock::new(|| {
    [
        ("GET", r"^/v1/experiments/[^/]+$"),
        ("PATCH", r"^/v1/experiments/[^/]+$"),
        ("POST", r"^/v1/experiments/[^/]+/(start|end|cancel|run)$"),
        ("DELETE", r"^/v1/connectors/[^/]+$"),
    ]
    .into_iter()
    .map(|(method, path)| (method, Regex::new(path).expect("valid route pattern")))
    .collect()
});

/// What replay matches a request on: method, origin (scheme, host and port,
/// so the two planes stay apart), the decoded path, the query as decoded
/// pairs (a key's values in order, keys in any order), and the body as its
/// meaning rather than its bytes. Headers are not part of it.
#[derive(Debug, Clone, PartialEq)]
pub struct Identity {
    pub method: String,
    pub origin: String,
    pub path: String,
    pub query: Vec<(String, String)>,
    pub body: Body,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Body {
    None,
    Json(Value),
    Form(Vec<(String, String)>),
    Multipart(Vec<Part>),
    Text(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Part {
    pub name: String,
    pub filename: Option<String>,
    pub content_type: Option<String>,
    pub content: String,
}

impl Identity {
    pub fn new(method: &str, url: &str, content_type: Option<&str>, body: &[u8]) -> Self {
        let parsed = url::Url::parse(&scrub_secrets(url))
            .unwrap_or_else(|e| panic!("walk replay: unparsable URL {url}: {e}"));
        let origin = format!(
            "{}://{}:{}",
            parsed.scheme(),
            parsed.host_str().unwrap_or_default(),
            parsed.port_or_known_default().unwrap_or_default()
        );
        let path = percent_encoding::percent_decode_str(parsed.path())
            .decode_utf8_lossy()
            .into_owned();
        let mut query: Vec<(String, String)> = parsed
            .query_pairs()
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect();
        // A repeated filter (`metadata`, `tag`, `contains`) is ANDed, so its
        // values compare as a set: the Rust SDK sends a metadata filter in key
        // order, the JS SDK in insertion order.
        query.sort();
        Self {
            method: method.to_ascii_uppercase(),
            origin,
            path,
            query,
            body: Body::parse(content_type, body),
        }
    }

    pub fn of_entry(entry: &Entry) -> Self {
        let post = entry.request.post_data.as_ref();
        let text = post.and_then(|p| p.text.clone()).unwrap_or_default();
        Self::new(
            &entry.request.method,
            &entry.request.url,
            post.map(|p| p.mime_type.as_str()),
            text.as_bytes(),
        )
    }

    pub fn same_route(&self, other: &Self) -> bool {
        self.method == other.method && self.origin == other.origin && self.path == other.path
    }

    /// `project` is the walk's own project, the one value
    /// [`PROJECT_OPTIONAL`] lets one side name and the other leave out.
    pub fn matches_recorded(&self, recorded: &Self, project: &str) -> bool {
        self.same_route(recorded)
            && self.query_matches(recorded, project)
            && self.body_matches(recorded)
    }

    fn body_matches(&self, recorded: &Self) -> bool {
        if self.body.matches(&recorded.body) {
            return true;
        }
        let (Body::Json(Value::Object(sent)), Body::Json(Value::Object(seen))) =
            (&self.body, &recorded.body)
        else {
            return false;
        };
        let mut sent = sent.clone();
        for (method, path, key, value) in DEFAULTED_FIELDS.iter() {
            if *method == self.method
                && path.is_match(&self.path)
                && !seen.contains_key(*key)
                && sent.get(*key) == Some(&Value::String((*value).into()))
            {
                sent.remove(*key);
            }
        }
        Body::Json(Value::Object(sent)).matches(&recorded.body)
    }

    fn query_matches(&self, recorded: &Self, project: &str) -> bool {
        if self.query == recorded.query {
            return true;
        }
        if !PROJECT_OPTIONAL
            .iter()
            .any(|(method, path)| *method == self.method && path.is_match(&self.path))
        {
            return false;
        }
        let without = |query: &[(String, String)]| -> Vec<(String, String)> {
            query
                .iter()
                .filter(|(k, v)| !(k == "project" && v == project))
                .cloned()
                .collect()
        };
        without(&self.query) == without(&recorded.query)
    }

    pub fn url(&self) -> String {
        let mut out = format!("{}{}", self.origin, self.path);
        for (i, (k, v)) in self.query.iter().enumerate() {
            let _ = write!(out, "{}{k}={v}", if i == 0 { '?' } else { '&' });
        }
        out
    }

    /// What differs from `recorded`, for the failure a request with no
    /// recorded match raises.
    pub fn diff(&self, recorded: &Self) -> String {
        let mut out = String::new();
        if self.query != recorded.query {
            let _ = write!(
                out,
                "\n    query: sent {:?}, recorded {:?}",
                self.query, recorded.query
            );
        }
        if !self.body.matches(&recorded.body) {
            let _ = write!(
                out,
                "\n    body: sent {}\n          recorded {}",
                self.body.describe(),
                recorded.body.describe()
            );
        }
        out
    }
}

impl Body {
    fn parse(content_type: Option<&str>, bytes: &[u8]) -> Self {
        if bytes.is_empty() {
            return Self::None;
        }
        let text = scrub_secrets(&String::from_utf8_lossy(bytes));
        let content_type = content_type.unwrap_or_default().to_ascii_lowercase();
        if content_type.contains("multipart/form-data") {
            return Self::Multipart(parse_multipart(&content_type, &text));
        }
        if content_type.contains("application/x-www-form-urlencoded") {
            let mut pairs: Vec<(String, String)> = url::form_urlencoded::parse(text.as_bytes())
                .map(|(k, v)| (k.into_owned(), scrub_secrets(&v)))
                .collect();
            pairs.sort_by(|a, b| a.0.cmp(&b.0));
            return Self::Form(pairs);
        }
        match serde_json::from_str::<Value>(&text) {
            Ok(value) => Self::Json(value),
            Err(_) => Self::Text(text),
        }
    }

    /// Equality of meaning (a JSON number by its value), with two documented
    /// equivalences and nothing looser:
    ///
    /// - An empty JSON object matches a recorded empty body. The Rust SDK
    ///   posts `{}` to three bodiless actions (a run's `/cancel`, an
    ///   automation's `/trigger`, an API key's `/acknowledge`) where the JS
    ///   SDK sends nothing.
    /// - A multipart body matches a recorded empty one. Polly in Node
    ///   records a `FormData` body as an empty `text/plain` entry, so the JS
    ///   recording carries no fields to compare; a recording this SDK writes
    ///   carries them and is compared field by field.
    fn matches(&self, recorded: &Self) -> bool {
        match (self, recorded) {
            (Self::Json(Value::Object(map)), Self::None) => map.is_empty(),
            (Self::Multipart(_), Self::None) => true,
            (Self::Json(sent), Self::Json(recorded)) => json_eq(sent, recorded),
            (sent, recorded) => sent == recorded,
        }
    }

    fn describe(&self) -> String {
        match self {
            Self::None => "(none)".into(),
            Self::Json(value) => value.to_string(),
            Self::Form(pairs) => format!("form {pairs:?}"),
            Self::Multipart(parts) => format!("multipart {parts:?}"),
            Self::Text(text) => format!("text {text:?}"),
        }
    }
}

/// JSON equality, where a number is its value: `1` and `1.0` are the same
/// JSON number, which `serde_json` keeps apart (an `f64` field such as
/// `sample_rate` serialises as `1.0`).
fn json_eq(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x.as_f64() == y.as_f64(),
        (Value::Array(x), Value::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(x, y)| json_eq(x, y))
        }
        (Value::Object(x), Value::Object(y)) => {
            x.len() == y.len()
                && x.iter()
                    .all(|(k, v)| y.get(k).is_some_and(|w| json_eq(v, w)))
        }
        (x, y) => x == y,
    }
}

fn parse_multipart(content_type: &str, text: &str) -> Vec<Part> {
    let Some(boundary) = content_type
        .split(';')
        .find_map(|p| p.trim().strip_prefix("boundary="))
    else {
        return Vec::new();
    };
    let delimiter = format!("--{}", boundary.trim_matches('"'));
    text.split(delimiter.as_str())
        .filter_map(|chunk| {
            let chunk = chunk.strip_prefix("\r\n")?;
            let (head, content) = chunk.split_once("\r\n\r\n")?;
            let headers: BTreeMap<String, String> = head
                .lines()
                .filter_map(|line| line.split_once(':'))
                .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string()))
                .collect();
            let disposition = headers.get("content-disposition")?;
            Some(Part {
                name: disposition_param(disposition, "name")?,
                filename: disposition_param(disposition, "filename"),
                content_type: headers.get("content-type").cloned(),
                content: content.trim_end_matches("\r\n").to_string(),
            })
        })
        .collect()
}

fn disposition_param(disposition: &str, key: &str) -> Option<String> {
    disposition.split(';').find_map(|p| {
        let (k, v) = p.trim().split_once('=')?;
        (k == key).then(|| v.trim_matches('"').to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROJECT: &str = "01a16a8d-5e8e-76ca-88f7-401338c97e70";

    fn json(method: &str, url: &str, body: &str) -> Identity {
        Identity::new(method, url, Some("application/json"), body.as_bytes())
    }

    fn matches(sent: &Identity, recorded: &Identity) -> bool {
        sent.matches_recorded(recorded, PROJECT)
    }

    #[test]
    fn a_json_body_matches_on_meaning_not_bytes() {
        let recorded = json(
            "POST",
            "http://localhost:8002/v1/files",
            r#"{"a":1,"b":{"c":[1,2]}}"#,
        );
        let sent = json(
            "POST",
            "http://localhost:8002/v1/files",
            r#"{ "b": {"c": [1, 2]}, "a": 1.0 }"#,
        );
        assert!(matches(&sent, &recorded));
        let changed = json(
            "POST",
            "http://localhost:8002/v1/files",
            r#"{"a":1,"b":{"c":[2,1]}}"#,
        );
        assert!(!matches(&changed, &recorded), "array order is meaning");
        let extra = json(
            "POST",
            "http://localhost:8002/v1/files",
            r#"{"a":1,"b":{"c":[1,2]},"d":null}"#,
        );
        assert!(
            !matches(&extra, &recorded),
            "an added field is a different request"
        );
    }

    #[test]
    fn the_planes_are_told_apart_by_port() {
        let cp = json("GET", "http://localhost:8000/v1/repositories/x", "");
        let dp = json("GET", "http://localhost:8002/v1/repositories/x", "");
        assert!(!matches(&cp, &dp));
    }

    #[test]
    fn a_query_matches_as_decoded_pairs_with_repeated_keys_as_a_set() {
        let recorded = json(
            "GET",
            "http://localhost:8002/v1/files?metadata=suite%3Awalk&metadata=stage%3Aattached&x=1",
            "",
        );
        let sent = json(
            "GET",
            "http://localhost:8002/v1/files?x=1&metadata=stage:attached&metadata=suite:walk",
            "",
        );
        assert!(matches(&sent, &recorded));
        let missing = json(
            "GET",
            "http://localhost:8002/v1/files?metadata=suite:walk&x=1",
            "",
        );
        assert!(!matches(&missing, &recorded));
    }

    #[test]
    fn the_project_query_is_optional_only_where_listed() {
        let recorded = json("POST", "http://localhost:8000/v1/experiments/e/start", "{}");
        let sent = json(
            "POST",
            &format!("http://localhost:8000/v1/experiments/e/start?project={PROJECT}"),
            "{}",
        );
        assert!(matches(&sent, &recorded));
        let other = json(
            "POST",
            "http://localhost:8000/v1/experiments/e/start?project=another",
            "{}",
        );
        assert!(!matches(&other, &recorded), "only the walk's own project");
        let unlisted = json(
            "GET",
            &format!("http://localhost:8000/v1/recipes/r?project={PROJECT}"),
            "",
        );
        assert!(!matches(
            &unlisted,
            &json("GET", "http://localhost:8000/v1/recipes/r", "")
        ));
    }

    #[test]
    fn an_empty_object_matches_a_bodiless_recording_and_nothing_else() {
        let recorded = json("POST", "http://localhost:8002/v1/automations/a/trigger", "");
        assert!(matches(
            &json(
                "POST",
                "http://localhost:8002/v1/automations/a/trigger",
                "{}"
            ),
            &recorded
        ));
        assert!(!matches(
            &json(
                "POST",
                "http://localhost:8002/v1/automations/a/trigger",
                r#"{"x":1}"#
            ),
            &recorded
        ));
    }

    #[test]
    fn a_secret_is_compared_scrubbed() {
        let recorded = json(
            "POST",
            "http://localhost:8000/v1/credentials",
            r#"{"name":"N","secret":"REDACTED_SECRET"}"#,
        );
        let sent = json(
            "POST",
            "http://localhost:8000/v1/credentials",
            r#"{"name":"N","secret":"walk-secret-value-0001"}"#,
        );
        assert!(matches(&sent, &recorded));
        let form = |secret: &str| {
            Identity::new(
                "POST",
                "http://localhost:8000/v1/oauth/token",
                Some("application/x-www-form-urlencoded"),
                format!("grant_type=client_credentials&client_secret={secret}").as_bytes(),
            )
        };
        assert!(matches(&form("intro_sk_abc"), &form("REDACTED_SECRET")));
    }

    #[test]
    fn a_defaulted_field_is_dropped_only_where_listed() {
        let recorded = json(
            "POST",
            "http://localhost:8000/v1/members",
            r#"{"email":"e","role":"member"}"#,
        );
        let business = json(
            "POST",
            "http://localhost:8000/v1/members",
            r#"{"email":"e","role":"member","member_type":"business"}"#,
        );
        assert!(matches(&business, &recorded));
        let customer = json(
            "POST",
            "http://localhost:8000/v1/members",
            r#"{"email":"e","role":"member","member_type":"customer"}"#,
        );
        assert!(!matches(&customer, &recorded));
    }

    #[test]
    fn a_multipart_body_is_compared_by_field() {
        let body = |content: &str| {
            format!(
                "--b\r\nContent-Disposition: form-data; name=\"file\"; filename=\"f.txt\"\r\nContent-Type: text/plain\r\n\r\n{content}\r\n--b\r\nContent-Disposition: form-data; name=\"name\"\r\n\r\nf.txt\r\n--b--\r\n"
            )
        };
        let part = |content: &str| {
            Identity::new(
                "POST",
                "http://localhost:8002/v1/files",
                Some("multipart/form-data; boundary=b"),
                body(content).as_bytes(),
            )
        };
        assert!(matches(&part("v2"), &part("v2")));
        assert!(!matches(&part("v3"), &part("v2")));
    }
}
