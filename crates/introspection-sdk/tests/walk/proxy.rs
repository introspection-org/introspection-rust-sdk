//! A forward HTTP proxy every client in the walk talks through, so the SDK is
//! exercised exactly as shipped: `IntrospectionClient`, the token mint and the
//! Data Plane URL the token response names all build their own `reqwest`
//! client, and every one of them honours `HTTP_PROXY` (the crate enables
//! reqwest's `system-proxy`). Absolute-form requests carry the original
//! origin, so the two planes (CP `localhost:8000`, DP `localhost:8002`) stay
//! apart without rewriting a URL.
//!
//! Replay answers from the HAR; record forwards to the real target and
//! writes what it saw, scrubbed, into a HAR of the same shape.

use std::collections::HashMap;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use base64::Engine as _;
use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper_util::rt::TokioIo;
use serde::Serialize;

use crate::har::{self, Entry, Har, Identity, NameValue};

/// Requests carrying this header are forwarded and not recorded: the polls of
/// `settled()`, which the recording must never see (design doc §5).
pub const QUIET_HEADER: &str = "x-walk-quiet";

#[derive(Debug, Clone)]
pub struct Exchange {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct Reply {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Exchange {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    fn identity(&self) -> Identity {
        Identity::new(
            &self.method,
            &self.url,
            self.header("content-type"),
            &self.body,
        )
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Unmatched {
    pub method: String,
    pub url: String,
    pub detail: String,
}

/// The replay side: the recorded entries, which of them have answered, and
/// the requests that matched none.
pub struct Replayer {
    entries: Vec<Entry>,
    identities: Vec<Identity>,
    consumed: Vec<bool>,
    project: String,
    pub unmatched: Vec<Unmatched>,
}

impl Replayer {
    pub fn new(har: Har, project: &str) -> Self {
        let identities = har.log.entries.iter().map(Identity::of_entry).collect();
        let consumed = vec![false; har.log.entries.len()];
        Self {
            entries: har.log.entries,
            identities,
            consumed,
            project: project.to_string(),
            unmatched: Vec::new(),
        }
    }

    /// The first unconsumed entry with this identity answers, so identical
    /// requests (the same resource read as it changes) get their responses in
    /// the order they were recorded.
    fn answer(&mut self, exchange: &Exchange) -> Reply {
        let sent = exchange.identity();
        let hit = (0..self.entries.len()).find(|&i| {
            !self.consumed[i] && sent.matches_recorded(&self.identities[i], &self.project)
        });
        if let Some(i) = hit {
            self.consumed[i] = true;
            let response = &self.entries[i].response;
            let headers = response
                .headers
                .iter()
                .filter(|h| !is_framing(&h.name))
                .map(|h| (h.name.clone(), h.value.clone()))
                .collect();
            return Reply {
                status: response.status,
                headers,
                body: response.body(),
            };
        }
        let detail = self.explain(&sent);
        let message = format!(
            "walk replay: no recorded {} {} matches what the SDK sent{detail}",
            sent.method,
            sent.url()
        );
        self.unmatched.push(Unmatched {
            method: sent.method.clone(),
            url: sent.url(),
            detail: detail.trim().to_string(),
        });
        Reply {
            // Not a status the SDK retries, and not one any step expects.
            status: 599,
            headers: vec![("content-type".into(), "text/plain".into())],
            body: message.into_bytes(),
        }
    }

    fn explain(&self, sent: &Identity) -> String {
        // The unconsumed candidates first: they are the ones it could have been.
        let mut same_route: Vec<usize> = (0..self.entries.len())
            .filter(|&i| sent.same_route(&self.identities[i]))
            .collect();
        same_route.sort_by_key(|&i| self.consumed[i]);
        if same_route.is_empty() {
            return "\n    no recorded request on this method and path".into();
        }
        let mut out = String::new();
        for i in same_route.into_iter().take(4) {
            let state = if self.consumed[i] {
                "consumed"
            } else {
                "unconsumed"
            };
            out.push_str(&format!(
                "\n  vs entry {i} ({state}): {}{}",
                self.identities[i].url(),
                sent.diff(&self.identities[i])
            ));
        }
        out
    }

    pub fn total(&self) -> usize {
        self.entries.len()
    }

    pub fn consumed(&self) -> usize {
        self.consumed.iter().filter(|c| **c).count()
    }

    pub fn unconsumed(&self) -> Vec<(String, String)> {
        self.entries
            .iter()
            .zip(&self.consumed)
            .filter(|(_, c)| !**c)
            .map(|(e, _)| (e.request.method.clone(), e.request.url.clone()))
            .collect()
    }
}

/// The record side: forwards to the real target and keeps what it saw.
pub struct Recorder {
    client: reqwest::Client,
    entries: Mutex<Vec<Entry>>,
    pub path: PathBuf,
}

impl Recorder {
    pub fn new(path: PathBuf) -> Self {
        Self {
            client: reqwest::Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .expect("a plain reqwest client builds"),
            entries: Mutex::new(Vec::new()),
            path,
        }
    }

    async fn forward(&self, exchange: &Exchange) -> Reply {
        let started = Instant::now();
        let started_at = httpdate::fmt_http_date(std::time::SystemTime::now());
        let method = reqwest::Method::from_bytes(exchange.method.as_bytes()).expect("a method");
        let mut request = self.client.request(method, &exchange.url);
        for (k, v) in &exchange.headers {
            let lower = k.to_ascii_lowercase();
            if !matches!(
                lower.as_str(),
                "host"
                    | "proxy-connection"
                    | "connection"
                    | "content-length"
                    | "accept-encoding"
                    | QUIET_HEADER
            ) {
                request = request.header(k, v);
            }
        }
        let response = request
            .body(exchange.body.clone())
            .send()
            .await
            .unwrap_or_else(|e| {
                panic!(
                    "walk record: {} {} failed: {e}",
                    exchange.method, exchange.url
                )
            });
        let status = response.status().as_u16();
        // reqwest decompressed the body, so the encoding headers no longer
        // describe it.
        let headers: Vec<(String, String)> = response
            .headers()
            .iter()
            .filter(|(k, _)| !is_framing(k.as_str()))
            .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or_default().to_string()))
            .collect();
        let body = response
            .bytes()
            .await
            .map(|b| b.to_vec())
            .unwrap_or_default();
        let reply = Reply {
            status,
            headers,
            body,
        };
        // The step 3 `git push` crosses the proxy too; replay never pushes,
        // so the pack stays out of the recording.
        if exchange.header(QUIET_HEADER).is_none() && !exchange.url.contains("/v1/git/") {
            let entry = to_entry(
                exchange,
                &reply,
                started_at,
                started.elapsed().as_secs_f64(),
            );
            self.entries.lock().expect("recorder lock").push(entry);
        }
        reply
    }

    pub fn write(&self) {
        let entries = self.entries.lock().expect("recorder lock").clone();
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir).expect("create the recordings directory");
        }
        let text = serde_json::to_string_pretty(&Har::new(entries)).expect("a HAR serialises");
        std::fs::write(&self.path, text + "\n").expect("write the recording");
        eprintln!("walk record: wrote {}", self.path.display());
    }
}

fn to_entry(exchange: &Exchange, reply: &Reply, started: String, seconds: f64) -> Entry {
    let headers = |pairs: &[(String, String)]| -> Vec<NameValue> {
        pairs
            .iter()
            .filter(|(k, _)| !k.eq_ignore_ascii_case(QUIET_HEADER))
            .map(|(k, v)| NameValue {
                name: k.to_ascii_lowercase(),
                value: har::scrub_header(k, v),
            })
            .collect()
    };
    let url = har::scrub_secrets(&exchange.url);
    let query_string: Vec<NameValue> = url::Url::parse(&url)
        .map(|u| {
            u.query_pairs()
                .map(|(k, v)| NameValue {
                    name: k.into_owned(),
                    value: v.into_owned(),
                })
                .collect()
        })
        .unwrap_or_default();
    let request_type = exchange
        .header("content-type")
        .unwrap_or_default()
        .to_string();
    let response_type = reply
        .headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("content-type"))
        .map(|(_, v)| v.clone())
        .unwrap_or_default();
    let (text, encoding) = match std::str::from_utf8(&reply.body) {
        Ok(text) => (har::scrub_secrets(text), None),
        Err(_) => (
            base64::engine::general_purpose::STANDARD.encode(&reply.body),
            Some("base64".to_string()),
        ),
    };
    let entry = serde_json::json!({
        "startedDateTime": started,
        "time": seconds * 1000.0,
        "request": {
            "method": exchange.method,
            "url": url,
            "httpVersion": "HTTP/1.1",
            "headers": headers(&exchange.headers),
            "queryString": query_string,
            "cookies": [],
            "headersSize": -1,
            "bodySize": exchange.body.len(),
            "postData": (!exchange.body.is_empty()).then(|| serde_json::json!({
                "mimeType": request_type,
                "params": [],
                "text": har::scrub_secrets(&String::from_utf8_lossy(&exchange.body)),
            })),
        },
        "response": {
            "status": reply.status,
            "statusText": "",
            "httpVersion": "HTTP/1.1",
            "headers": headers(&reply.headers),
            "cookies": [],
            "content": {
                "mimeType": response_type,
                "size": reply.body.len(),
                "text": text,
                "encoding": encoding,
            },
            "redirectURL": "",
            "headersSize": -1,
            "bodySize": reply.body.len(),
        },
        "cache": {},
        "timings": { "send": 0, "wait": seconds * 1000.0, "receive": 0 },
    });
    let mut entry: Entry = serde_json::from_value(entry).expect("a HAR entry");
    if let Some(post) = entry.request.post_data.as_mut() {
        post.text = post.text.take().filter(|t| !t.is_empty());
    }
    entry
}

/// Headers describing how a body was framed on the wire, which the proxy
/// re-frames itself.
fn is_framing(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "content-length" | "transfer-encoding" | "content-encoding" | "connection"
    )
}

pub enum Backend {
    Replay(Mutex<Replayer>),
    Record(Recorder),
    /// Forwards without writing anything (`WALK_MODE=passthrough`).
    Passthrough(Recorder),
}

impl Backend {
    /// One exchange, whether it arrived through the proxy or from the walk's
    /// own code (the OAuth grant's redirects to an `https` provider, which a
    /// plain HTTP proxy cannot see into).
    pub async fn exchange(&self, exchange: Exchange) -> Reply {
        match self {
            Self::Replay(replayer) => replayer.lock().expect("replayer lock").answer(&exchange),
            Self::Record(recorder) | Self::Passthrough(recorder) => {
                recorder.forward(&exchange).await
            }
        }
    }

    pub fn finish(&self) {
        if let Self::Record(recorder) = self {
            recorder.write();
        }
    }
}

/// Serve `backend` as a forward proxy on an ephemeral local port.
pub async fn serve(backend: Arc<Backend>) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind the walk's proxy");
    let addr = listener.local_addr().expect("the proxy's address");
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                continue;
            };
            let backend = backend.clone();
            tokio::spawn(async move {
                let service = service_fn(move |req| handle(req, backend.clone()));
                let _ = http1::Builder::new()
                    .serve_connection(TokioIo::new(stream), service)
                    .await;
            });
        }
    });
    addr
}

async fn handle(
    req: hyper::Request<hyper::body::Incoming>,
    backend: Arc<Backend>,
) -> Result<hyper::Response<Full<Bytes>>, Infallible> {
    let (parts, body) = req.into_parts();
    let headers: Vec<(String, String)> = parts
        .headers
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or_default().to_string()))
        .collect();
    let url = if parts.uri.authority().is_some() {
        parts.uri.to_string()
    } else {
        let host: HashMap<_, _> = headers.iter().cloned().collect();
        format!(
            "http://{}{}",
            host.get("host").cloned().unwrap_or_default(),
            parts.uri
        )
    };
    let body = body
        .collect()
        .await
        .map(|b| b.to_bytes().to_vec())
        .unwrap_or_default();
    let reply = backend
        .exchange(Exchange {
            method: parts.method.to_string(),
            url,
            headers,
            body,
        })
        .await;
    let mut response = hyper::Response::builder().status(reply.status);
    for (k, v) in &reply.headers {
        response = response.header(k, v);
    }
    Ok(response
        .body(Full::new(Bytes::from(reply.body)))
        .expect("a proxied response builds"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(url: &str, status: u16) -> Entry {
        serde_json::from_value(serde_json::json!({
            "startedDateTime": "2026-10-23T20:56:52Z",
            "time": 0,
            "request": { "method": "GET", "url": url },
            "response": { "status": status, "content": { "mimeType": "application/json", "text": "{}" } },
        }))
        .expect("an entry")
    }

    fn get(url: &str) -> Exchange {
        Exchange {
            method: "GET".into(),
            url: url.into(),
            headers: Vec::new(),
            body: Vec::new(),
        }
    }

    #[test]
    fn identical_requests_answer_in_recorded_order_and_then_not_at_all() {
        let url = "http://localhost:8002/v1/tasks/t";
        let har = Har::new(vec![
            entry(url, 200),
            entry("http://localhost:8002/v1/files", 200),
            entry(url, 404),
        ]);
        let mut replayer = Replayer::new(har, "p");
        assert_eq!(replayer.answer(&get(url)).status, 200);
        assert_eq!(replayer.answer(&get(url)).status, 404);
        assert_eq!(
            replayer.answer(&get(url)).status,
            599,
            "a third read was never recorded"
        );
        assert_eq!(replayer.unmatched.len(), 1);
        assert_eq!(replayer.consumed(), 2);
        assert_eq!(
            replayer.unconsumed(),
            vec![(
                "GET".to_string(),
                "http://localhost:8002/v1/files".to_string()
            )]
        );
    }
}
