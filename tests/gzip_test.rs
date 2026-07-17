//! Gzip transport tests: the client advertises `Accept-Encoding: gzip` and
//! transparently decompresses gzipped JSON responses (the DP gateway
//! compresses `application/json` when the client offers gzip).
//!
//! Built like the other API tests: namespaces constructed via
//! [`HttpClient::from_parts`] against a `wiremock::MockServer`.

use std::collections::HashMap;
use std::io::Write;
use std::sync::Arc;
use std::time::Duration;

use flate2::write::GzEncoder;
use flate2::Compression;
use introspection_sdk::api::{ConversationListParams, Conversations, HttpClient, HttpConfig};
use serde_json::json;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn build_http(server: &MockServer) -> Arc<HttpClient> {
    let cfg = HttpConfig {
        api_url: server.uri(),
        token: "intro_test".to_string(),
        additional_headers: HashMap::new(),
        timeout: Duration::from_secs(5),
        max_retries: 2,
        retry_base: Duration::from_millis(1),
    };
    Arc::new(HttpClient::from_parts(reqwest::Client::new(), cfg))
}

fn gzip_bytes(body: &[u8]) -> Vec<u8> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(body).unwrap();
    encoder.finish().unwrap()
}

/// A gzipped JSON page decodes exactly like an identity one, and the request
/// that fetched it advertised gzip — the mock only matches when the
/// `Accept-Encoding` offer is present.
#[tokio::test]
async fn gzipped_json_response_is_transparently_decoded() {
    let server = MockServer::start().await;
    let conversations = Conversations::new(build_http(&server));

    let body = json!({
        "records": [{"conversation_id": "conv-1"}],
        "count": 1,
        "total_count": 1,
        "next": null,
    });
    Mock::given(method("GET"))
        .and(path("/v1/conversations"))
        .and(header("accept-encoding", "gzip"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-encoding", "gzip")
                .insert_header("content-type", "application/json")
                .set_body_bytes(gzip_bytes(body.to_string().as_bytes())),
        )
        .expect(1)
        .mount(&server)
        .await;

    let mut paginator = conversations
        .list(&ConversationListParams::default())
        .expect("params validate");
    let page = paginator.next_page().await.unwrap().unwrap();
    assert_eq!(page.count, 1);
    assert_eq!(page.records[0].conversation_id.as_deref(), Some("conv-1"));
}
