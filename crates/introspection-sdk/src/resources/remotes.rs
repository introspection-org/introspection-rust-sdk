//! `client.remotes()` (DP) — the `introspection dev` remote connection.
//!
//! A developer's machine dials the Data Plane at `/v1/remotes/connect`, a
//! WebSocket, and serves its local MCP servers and Recipe edits to the
//! development tasks of one Runtime. This namespace opens that socket with
//! the client's Data Plane URL and token ([`Remotes::connect`], behind the
//! `remotes` cargo feature) and reads the route's protocol description
//! ([`Remotes::connect_docs`]).
//!
//! The frames that cross the socket are not modelled here. Every message is
//! one protobuf-encoded `Frame` in a binary message, whose schema
//! (`remote.proto`) the CLI's `introspection-remote-protocol` crate owns; the
//! caller encodes and decodes them. The handshake is the caller's too: send
//! an `attach_request` frame first and read `attach_ok` or an error back.
//!
//! The socket needs a login access token carrying the `remotes:connect`
//! scope, which only a development login (`introspection login`, see
//! [`crate::auth::DeviceCodeAuth`]) holds. A project API key or a token
//! without the scope is refused after the upgrade: the server closes the
//! socket with code `4403`, or `4001` for a token it cannot verify.

use std::sync::Arc;

use crate::api::error::{ApiResult, IntrospectionAPIError};
use crate::api::http::HttpClient;
use crate::api::schemas::RemotesConnectDocs;

/// The WebSocket client [`Remotes::connect`] is built on, re-exported so a
/// caller names its `Message` and error types at the SDK's version.
#[cfg(feature = "remotes")]
pub use tokio_tungstenite;

/// The remote connection's path on the Data Plane.
pub const REMOTES_CONNECT_PATH: &str = "/v1/remotes/connect";

/// `client.remotes()` namespace. Holds a DP-bound HTTP client.
#[derive(Clone)]
pub struct Remotes {
    http: Arc<HttpClient>,
}

impl Remotes {
    #[doc(hidden)]
    pub fn new(http: Arc<HttpClient>) -> Self {
        Self { http }
    }

    /// `GET /v1/remotes/connect` — the WebSocket's protocol description:
    /// authentication, handshake, schema version and frame catalogue.
    /// WebSocket routes are not expressible in OpenAPI, so the Data Plane
    /// answers a plain `GET` on the same path with this.
    pub async fn connect_docs(&self) -> ApiResult<RemotesConnectDocs> {
        self.http.get_json(REMOTES_CONNECT_PATH, &()).await
    }

    /// The `ws[s]://` URL [`Self::connect`] dials, derived from the client's
    /// Data Plane URL (see [`remotes_connect_url`]).
    pub fn connect_url(&self) -> ApiResult<String> {
        remotes_connect_url(&self.http.config().api_url)
    }

    /// Open the remote connection with the client's token as
    /// `Authorization: Bearer`.
    ///
    /// Returns the socket once the upgrade succeeds. The server checks the
    /// token after the upgrade, so a refused credential arrives as a close
    /// frame (`4001` or `4403`) on the socket, not as an error here; a
    /// refusal before the upgrade is a [`RemoteConnectError::WebSocket`]
    /// holding the HTTP response.
    ///
    /// A `wss://` URL is verified against the platform's native roots, the
    /// trust store the REST client reads. rustls needs a crypto provider: the
    /// SDK's REST client enables `aws-lc-rs`, and a process that enables a
    /// second one installs its choice as the process default first.
    #[cfg(feature = "remotes")]
    pub async fn connect(&self) -> Result<RemoteSocket, RemoteConnectError> {
        let url = self.connect_url().map_err(RemoteConnectError::Config)?;
        connect_remote(&url, &self.http.config().token).await
    }
}

/// The socket [`Remotes::connect`] opens. Each message is
/// [`Message::Binary`](tokio_tungstenite::tungstenite::Message::Binary)
/// holding one protobuf `Frame`.
#[cfg(feature = "remotes")]
pub type RemoteSocket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// Why [`Remotes::connect`] could not open the socket.
#[cfg(feature = "remotes")]
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum RemoteConnectError {
    /// The client's Data Plane URL has no WebSocket form.
    #[error(transparent)]
    Config(IntrospectionAPIError),
    /// The connection or the upgrade failed. Boxed: the WebSocket error
    /// holds a whole HTTP response.
    #[error("remote connection failed: {0}")]
    WebSocket(Box<tokio_tungstenite::tungstenite::Error>),
}

#[cfg(feature = "remotes")]
impl From<tokio_tungstenite::tungstenite::Error> for RemoteConnectError {
    fn from(err: tokio_tungstenite::tungstenite::Error) -> Self {
        Self::WebSocket(Box::new(err))
    }
}

/// Open the remote connection at `url` (a `ws[s]://` URL, see
/// [`remotes_connect_url`]) with `token` as `Authorization: Bearer`. What
/// [`Remotes::connect`] does with the client's own URL and token.
#[cfg(feature = "remotes")]
pub async fn connect_remote(url: &str, token: &str) -> Result<RemoteSocket, RemoteConnectError> {
    use tokio_tungstenite::tungstenite::client::IntoClientRequest as _;
    use tokio_tungstenite::tungstenite::http::header::{HeaderValue, AUTHORIZATION};

    let mut request = url.into_client_request()?;
    let bearer = HeaderValue::from_str(&format!("Bearer {token}"))
        .map_err(|err| tokio_tungstenite::tungstenite::Error::HttpFormat(err.into()))?;
    request.headers_mut().insert(AUTHORIZATION, bearer);
    let (socket, _response) = tokio_tungstenite::connect_async(request).await?;
    Ok(socket)
}

/// The remote connection's `ws[s]://` URL for a Data Plane base URL.
///
/// `https://` becomes `wss://` and `http://` becomes `ws://`, and
/// [`REMOTES_CONNECT_PATH`] is appended when the URL has no path. A
/// `ws[s]://` URL passes through, with the path appended to a bare host.
///
/// ```
/// use introspection_sdk::resources::remotes::remotes_connect_url;
///
/// assert_eq!(
///     remotes_connect_url("https://dp.example.com/").unwrap(),
///     "wss://dp.example.com/v1/remotes/connect"
/// );
/// ```
pub fn remotes_connect_url(base: &str) -> ApiResult<String> {
    let trimmed = base.trim().trim_end_matches('/');
    let (scheme, rest) = if let Some(rest) = trimmed.strip_prefix("https://") {
        ("wss://", rest)
    } else if let Some(rest) = trimmed.strip_prefix("http://") {
        ("ws://", rest)
    } else if let Some(rest) = trimmed.strip_prefix("wss://") {
        ("wss://", rest)
    } else if let Some(rest) = trimmed.strip_prefix("ws://") {
        ("ws://", rest)
    } else {
        return Err(IntrospectionAPIError::InvalidConfig(format!(
            "the remote connection needs an http(s):// or ws(s):// URL, not `{base}`"
        )));
    };
    if rest.is_empty() {
        return Err(IntrospectionAPIError::InvalidConfig(format!(
            "the URL `{base}` has no host"
        )));
    }
    let path = if rest.contains('/') {
        ""
    } else {
        REMOTES_CONNECT_PATH
    };
    Ok(format!("{scheme}{rest}{path}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connect_url_follows_the_base_url() {
        for (base, url) in [
            (
                "https://dp.example.com",
                "wss://dp.example.com/v1/remotes/connect",
            ),
            (
                "http://localhost:8002/",
                "ws://localhost:8002/v1/remotes/connect",
            ),
            (
                "wss://dp.example.com/v1/remotes/connect",
                "wss://dp.example.com/v1/remotes/connect",
            ),
            (
                "ws://127.0.0.1:8002",
                "ws://127.0.0.1:8002/v1/remotes/connect",
            ),
        ] {
            assert_eq!(remotes_connect_url(base).unwrap(), url);
        }
        for base in ["", "ftp://example.com", "example.com", "https://"] {
            assert!(remotes_connect_url(base).is_err(), "{base}");
        }
    }
}
