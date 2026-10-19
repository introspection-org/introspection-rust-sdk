//! The agent's calls to its Person Server (AAuth §Person Server), each
//! signed as the agent with its agent token.

use http::{HeaderMap, Method, StatusCode};
use serde_json::{json, Value};

use super::connect::ConnectTo;
use super::guard::admit_host;
use crate::discovery::{read_limited, MAX_DOCUMENT_BYTES};
use crate::error::{Error, Result};
use crate::httpsig::{header_map, sign_jwt, Outgoing};
use crate::identifier::same_origin;
use crate::keys::SigningKey;
use crate::metadata::{dwk, metadata_url, PersonServerMetadata};

/// What a Person Server answered.
#[derive(Debug)]
#[non_exhaustive]
pub enum Answer {
    /// `200`: what was asked for, such as `{"auth_token": …}`.
    Granted(Value),
    /// `202`: a human is deciding. Poll `location` after `retry_after` seconds.
    Pending {
        /// The pending URL, absolute, on the Person Server's own origin.
        location: String,
        /// Seconds to wait before polling.
        retry_after: u64,
    },
    /// A refusal: a `4xx`, or a `200` saying `"permission": "denied"`.
    Denied {
        /// The HTTP status.
        status: u16,
        /// Why, from whichever field the Person Server used.
        reason: String,
    },
}

/// Who the agent acts for, as the Person Server knows them: a `login_hint`,
/// and the `tenant` a multi-tenant Person Server is asked under.
#[derive(Clone, Copy, Debug, Default)]
pub struct Person<'a> {
    /// The hint the Person Server's directory recognises, such as `user:sam@acme.example`.
    pub login_hint: Option<&'a str>,
    /// The person's organisation, at a multi-tenant Person Server.
    pub tenant: Option<&'a str>,
}

impl Person<'_> {
    /// Set a request body's `login_hint` and `tenant` to these, removing any
    /// it already carried. A proxy signing for code it does not trust uses
    /// this so that code cannot name someone else.
    pub fn name_in(&self, body: &mut Value) {
        let Some(fields) = body.as_object_mut() else {
            return;
        };
        fields.remove("login_hint");
        fields.remove("tenant");
        if let Some(hint) = self.login_hint {
            fields.insert("login_hint".into(), json!(hint));
        }
        if let Some(tenant) = self.tenant {
            fields.insert("tenant".into(), json!(tenant));
        }
    }
}

/// The agent as it signs: its agent token, and the key that token binds.
#[derive(Clone, Copy, Debug)]
pub struct Agent<'a> {
    /// The agent token its provider issued.
    pub agent_token: &'a str,
    /// The key the token's `cnf.jwk` names.
    pub key: &'a SigningKey,
}

/// Where a [`PersonServerClient`] may send requests. The default is
/// production's: `https` only, and never an IP literal or `localhost`.
#[derive(Clone, Debug, Default)]
pub struct Reach {
    /// Allow `http://` URLs. Local development only.
    pub allow_http: bool,
    /// Allow IP literals and `localhost`. Local development only; with a
    /// guarded client, its resolver still refuses private answers.
    pub allow_private: bool,
    /// Send requests for some authorities to other addresses.
    pub connect_to: ConnectTo,
}

/// The agent's side of a Person Server conversation.
///
/// Every call is signed as the agent. A `5xx`, a `429` or a transport
/// failure is an [`Error::Http`], never an [`Answer::Denied`], so a dropped
/// connection is not mistaken for a refusal.
#[derive(Clone, Debug)]
pub struct PersonServerClient {
    http: reqwest::Client,
    reach: Reach,
}

impl PersonServerClient {
    /// A client over `http`. Use [`super::guarded_client`]'s when Person
    /// Server URLs come from someone else.
    pub fn new(http: reqwest::Client, reach: Reach) -> Self {
        Self { http, reach }
    }

    fn request(&self, method: Method, url: &str) -> Result<reqwest::RequestBuilder> {
        let route = self.reach.connect_to.route(url)?;
        let request = self.http.request(method, &route.url);
        Ok(match route.host {
            Some(host) => request.header("host", host),
            None => request,
        })
    }

    fn admit(&self, url: &str) -> Result<()> {
        if !(url.starts_with("https://") || (self.reach.allow_http && url.starts_with("http://"))) {
            return Err(Error::Url(format!("refusing to call {url}")));
        }
        if self.reach.allow_private {
            return Ok(());
        }
        admit_host(url)
    }

    /// The Person Server's `aauth-person.json`. It must name `issuer`, and
    /// every endpoint in it must be on the issuer's own origin.
    pub async fn metadata(&self, issuer: &str) -> Result<PersonServerMetadata> {
        let url = metadata_url(issuer, dwk::PERSON);
        self.admit(&url)?;
        let response = self
            .request(Method::GET, &url)?
            .send()
            .await
            .map_err(|e| Error::Http(format!("{url}: {e}")))?;
        if !response.status().is_success() {
            return Err(Error::Discovery(format!(
                "{url} answered {}",
                response.status()
            )));
        }
        let body = read_limited(response, MAX_DOCUMENT_BYTES).await?;
        let metadata: PersonServerMetadata = serde_json::from_slice(&body)
            .map_err(|e| Error::Discovery(format!("{url} is unreadable: {e}")))?;
        if metadata.issuer.trim_end_matches('/') != issuer.trim_end_matches('/') {
            return Err(Error::Discovery(format!("{url} names another issuer")));
        }
        // A caller that routes only the Person Server's own host would
        // otherwise be sent somewhere nobody declared.
        let endpoints = [
            Some(&metadata.auth_token_endpoint),
            metadata.person_token_endpoint.as_ref(),
            metadata.mission_endpoint.as_ref(),
        ];
        if let Some(elsewhere) = endpoints
            .into_iter()
            .flatten()
            .find(|endpoint| !same_origin(issuer, endpoint))
        {
            return Err(Error::Discovery(format!(
                "{url} names an endpoint on another host: {elsewhere}"
            )));
        }
        Ok(metadata)
    }

    /// A signed `POST` of JSON `body`, or a `GET` or `DELETE` with none, to `url`.
    pub async fn call(
        &self,
        agent: &Agent<'_>,
        method: Method,
        url: &str,
        body: Option<&Value>,
    ) -> Result<Answer> {
        self.admit(url)?;
        let bytes = body
            .map(serde_json::to_vec)
            .transpose()
            .map_err(|e| Error::Invalid(e.to_string()))?;
        let headers = header_map(&sign_jwt(
            agent.key,
            agent.agent_token,
            &Outgoing {
                method: method.as_str(),
                url,
                content_type: None,
                body: bytes.as_deref(),
                created: crate::now_unix(),
            },
        )?)?;
        let mut request = self.request(method, url)?.headers(headers);
        if let Some(bytes) = bytes {
            request = request.body(bytes);
        }
        let response = request
            .send()
            .await
            .map_err(|e| Error::Http(format!("{url}: {e}")))?;
        let status = response.status();
        let headers = response.headers().clone();
        let body = read_limited(response, MAX_DOCUMENT_BYTES).await?;
        answer_from(url, status, &headers, &body)
    }

    /// Ask for a person token for `resource`, under a mission when `mission_s256` names one.
    pub async fn person_token(
        &self,
        agent: &Agent<'_>,
        metadata: &PersonServerMetadata,
        resource: &str,
        person: &Person<'_>,
        mission_s256: Option<&str>,
    ) -> Result<Answer> {
        let endpoint = metadata
            .person_token_endpoint
            .as_deref()
            .ok_or_else(|| Error::Discovery("this Person Server issues no person tokens".into()))?;
        let mut body = json!({ "resource": resource });
        person.name_in(&mut body);
        if let Some(s256) = mission_s256 {
            body["mission_s256"] = json!(s256);
        }
        self.call(agent, Method::POST, endpoint, Some(&body)).await
    }

    /// Exchange a resource token for an auth token, presenting the token the
    /// resource token was issued against.
    pub async fn auth_token(
        &self,
        agent: &Agent<'_>,
        metadata: &PersonServerMetadata,
        resource_token: &str,
        presented_token: &str,
        person: &Person<'_>,
    ) -> Result<Answer> {
        let mut body = json!({
            "resource_token": resource_token,
            "presented_token": presented_token,
            "capabilities": ["interaction"],
        });
        person.name_in(&mut body);
        self.call(
            agent,
            Method::POST,
            &metadata.auth_token_endpoint,
            Some(&body),
        )
        .await
    }

    /// Propose a mission: `{"description": …, "resources": [...]}`.
    pub async fn propose_mission(
        &self,
        agent: &Agent<'_>,
        metadata: &PersonServerMetadata,
        proposal: &Value,
        person: &Person<'_>,
    ) -> Result<Answer> {
        let endpoint = self.mission_endpoint(metadata)?;
        let mut body = proposal.clone();
        person.name_in(&mut body);
        self.call(agent, Method::POST, endpoint, Some(&body)).await
    }

    /// Act on a mission: `{"action": "update", "description": …}`, or
    /// `{"action": "completion", "summary": …}`.
    pub async fn mission_action(
        &self,
        agent: &Agent<'_>,
        metadata: &PersonServerMetadata,
        s256: &str,
        action: &Value,
    ) -> Result<Answer> {
        let url = format!(
            "{}/{s256}",
            self.mission_endpoint(metadata)?.trim_end_matches('/')
        );
        self.call(agent, Method::POST, &url, Some(action)).await
    }

    /// Poll a pending request once.
    pub async fn poll(&self, agent: &Agent<'_>, location: &str) -> Result<Answer> {
        self.call(agent, Method::GET, location, None).await
    }

    /// Withdraw a pending request.
    pub async fn cancel(&self, agent: &Agent<'_>, location: &str) -> Result<Answer> {
        self.call(agent, Method::DELETE, location, None).await
    }

    fn mission_endpoint<'m>(&self, metadata: &'m PersonServerMetadata) -> Result<&'m str> {
        metadata
            .mission_endpoint
            .as_deref()
            .ok_or_else(|| Error::Discovery("this Person Server has no missions".into()))
    }
}

/// Read a Person Server's response to a request sent to `url`.
pub fn answer_from(
    url: &str,
    status: StatusCode,
    headers: &HeaderMap,
    body: &[u8],
) -> Result<Answer> {
    if status == StatusCode::ACCEPTED {
        let location = headers
            .get("location")
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| Error::Http("a pending answer has no Location".into()))?;
        let location = url::Url::parse(url)
            .and_then(|base| base.join(location))
            .map_err(|e| Error::Http(format!("bad Location: {e}")))?
            .to_string();
        if !same_origin(url, &location) {
            return Err(Error::Http(format!(
                "pending Location on another host: {location}"
            )));
        }
        let retry_after = headers
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse().ok())
            .unwrap_or(5);
        return Ok(Answer::Pending {
            location,
            retry_after,
        });
    }
    // A 5xx or 429 is the Person Server (or a proxy in front of it) failing,
    // not a decision: answering Denied would turn a dropped tunnel into a refusal.
    if status.is_server_error() || status == StatusCode::TOO_MANY_REQUESTS {
        return Err(Error::Http(format!("{url} answered {status}")));
    }
    let body: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    if status.is_success() {
        if body.get("permission").and_then(Value::as_str) == Some("denied") {
            return Ok(Answer::Denied {
                status: 403,
                reason: reason_of(&body),
            });
        }
        return Ok(Answer::Granted(body));
    }
    Ok(Answer::Denied {
        status: status.as_u16(),
        reason: reason_of(&body),
    })
}

fn reason_of(body: &Value) -> String {
    ["reason", "detail", "error_description", "error"]
        .iter()
        .find_map(|k| body.get(*k).and_then(Value::as_str))
        .unwrap_or("refused")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use http::HeaderValue;

    #[test]
    fn reasons_come_from_whichever_field_the_server_used() {
        assert_eq!(reason_of(&json!({"reason": "over cap"})), "over cap");
        assert_eq!(reason_of(&json!({"error": "denied"})), "denied");
        assert_eq!(
            reason_of(&json!({"error": "denied", "detail": "Dana declined"})),
            "Dana declined"
        );
        assert_eq!(reason_of(&Value::Null), "refused");
    }

    #[test]
    fn a_failing_person_server_is_not_a_refusal() {
        for status in [500, 502, 503, 530, 429] {
            let status = StatusCode::from_u16(status).unwrap();
            let answer = answer_from(
                "https://ps.example/pending/1",
                status,
                &HeaderMap::new(),
                b"<html>",
            );
            assert!(answer.is_err(), "{status} read as {answer:?}");
        }
    }

    #[test]
    fn a_client_error_is_still_a_refusal() {
        let answer = answer_from(
            "https://ps.example/pending/1",
            StatusCode::FORBIDDEN,
            &HeaderMap::new(),
            br#"{"reason":"over cap"}"#,
        );
        assert!(
            matches!(answer, Ok(Answer::Denied { status: 403, ref reason }) if reason == "over cap")
        );
    }

    #[test]
    fn a_pending_location_on_another_host_is_refused() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "location",
            HeaderValue::from_static("https://169.254.169.254/pending/1"),
        );
        let refused = answer_from(
            "https://ps.example/token",
            StatusCode::ACCEPTED,
            &headers,
            b"",
        )
        .unwrap_err();
        assert!(refused.to_string().contains("another host"), "{refused}");
        headers.insert("location", HeaderValue::from_static("/pending/1"));
        let Ok(Answer::Pending {
            location,
            retry_after,
        }) = answer_from(
            "https://ps.example/token",
            StatusCode::ACCEPTED,
            &headers,
            b"",
        )
        else {
            panic!()
        };
        assert_eq!(location, "https://ps.example/pending/1");
        assert_eq!(retry_after, 5);
    }

    #[test]
    fn plain_http_is_refused_unless_allowed() {
        let client = PersonServerClient::new(reqwest::Client::new(), Reach::default());
        assert!(client.admit("http://ps.example").is_err());
        assert!(client.admit("https://ps.example").is_ok());
        let dev = PersonServerClient::new(
            reqwest::Client::new(),
            Reach {
                allow_http: true,
                allow_private: true,
                ..Reach::default()
            },
        );
        assert!(dev.admit("http://127.0.0.1:3400").is_ok());
    }

    #[test]
    fn a_private_or_literal_host_is_refused_in_production() {
        let client = PersonServerClient::new(reqwest::Client::new(), Reach::default());
        assert!(client.admit("https://127.0.0.1/token").is_err());
        assert!(client.admit("https://169.254.169.254/").is_err());
        assert!(client.admit("https://ps.localhost/").is_err());
    }

    #[test]
    fn the_session_names_the_person_never_the_body() {
        let mut body = json!({ "login_hint": "dana", "tenant": "other", "description": "x" });
        Person {
            login_hint: Some("sam"),
            tenant: None,
        }
        .name_in(&mut body);
        assert_eq!(body, json!({ "login_hint": "sam", "description": "x" }));
    }

    #[tokio::test]
    async fn a_document_naming_another_host_is_refused() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let issuer = format!("http://{}", listener.local_addr().unwrap());
        let body = json!({
            "issuer": issuer,
            "auth_token_endpoint": "http://metadata.google.internal/token",
        })
        .to_string();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 4096];
            let _ = socket.read(&mut buf).await;
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{body}",
                body.len()
            );
            socket.write_all(response.as_bytes()).await.unwrap();
        });
        let dev = PersonServerClient::new(
            reqwest::Client::new(),
            Reach {
                allow_http: true,
                allow_private: true,
                ..Reach::default()
            },
        );
        let refused = dev.metadata(&issuer).await.unwrap_err();
        assert!(refused.to_string().contains("another host"), "{refused}");
    }
}
