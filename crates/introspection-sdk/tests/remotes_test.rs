//! `client.remotes()`: the protocol description over plain HTTP, and the
//! WebSocket open against a local server that checks the bearer.

// tungstenite's handshake callback returns its own large `ErrorResponse`.
#![allow(clippy::result_large_err)]

use futures::{SinkExt, StreamExt};
use introspection_sdk::resources::remotes::tokio_tungstenite;
use introspection_sdk::{AdvancedOptions, ClientConfig, IntrospectionClient};
use serde_json::json;
use tokio_tungstenite::tungstenite::handshake::server::{Request, Response};
use tokio_tungstenite::tungstenite::Message;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn client(dp_url: &str) -> IntrospectionClient {
    IntrospectionClient::new(
        ClientConfig::with_token("login-token").advanced(AdvancedOptions {
            dp_url: Some(dp_url.to_string()),
            ..Default::default()
        }),
    )
    .unwrap()
}

#[tokio::test]
async fn connect_docs_reads_the_protocol_description() {
    let dp = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/remotes/connect"))
        .and(header("authorization", "Bearer login-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "endpoint": "/v1/remotes/connect",
            "protocol": "wss",
            "authentication": {"method": "m", "description": "d", "example": "e"},
            "handshake": ["attach_request", "attach_ok"],
            "schema_version": 3,
            "encoding": "protobuf",
            "message_protocol": {
                "remote_to_cloud": [{"frame": "attach_request", "description": "first"}],
                "cloud_to_remote": [{"frame": "attach_ok", "description": "reply"}],
            },
        })))
        .expect(1)
        .mount(&dp)
        .await;

    let remotes = client(&dp.uri()).remotes();
    let docs = remotes.connect_docs().await.unwrap();
    assert_eq!(docs.schema_version, 3);
    assert_eq!(
        docs.message_protocol.remote_to_cloud[0].frame,
        "attach_request"
    );
    assert_eq!(
        remotes.connect_url().unwrap(),
        format!("{}/v1/remotes/connect", dp.uri().replacen("http", "ws", 1))
    );
}

#[tokio::test]
async fn connect_opens_the_socket_with_the_bearer() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut seen = None;
        let mut ws = tokio_tungstenite::accept_hdr_async(stream, |req: &Request, res: Response| {
            seen = Some((
                req.uri().path().to_string(),
                req.headers()["authorization"].to_str().unwrap().to_string(),
            ));
            Ok(res)
        })
        .await
        .unwrap();
        let frame = ws.next().await.unwrap().unwrap();
        ws.send(frame).await.unwrap();
        seen.unwrap()
    });

    let mut socket = client(&format!("http://{addr}"))
        .remotes()
        .connect()
        .await
        .unwrap();
    socket
        .send(Message::Binary(vec![1, 2, 3].into()))
        .await
        .unwrap();
    let echoed = socket.next().await.unwrap().unwrap();
    assert_eq!(echoed, Message::Binary(vec![1, 2, 3].into()));

    let (path, authorization) = server.await.unwrap();
    assert_eq!(path, "/v1/remotes/connect");
    assert_eq!(authorization, "Bearer login-token");
}
