//! The CP `POST /v1/oauth/token` grants, on the wire.
//!
//! Until these landed the crate had no authentication surface at all, so a
//! caller authenticating a CI job, a hosted-login backend, or a federation
//! broker had to hand-roll the form-encoded POST themselves.
//!
//! These drive a real mock CP so the parts that only exist on the wire — the
//! form encoding, the defaulted `subject_token_type`, the omitted empty
//! scope, and the error mapping — are exercised as they run in production.

use introspection_sdk::api::IntrospectionAPIError;
use introspection_sdk::auth::{
    authorization_code_token, service_account_token, token_exchange, AuthorizationCodeParams,
    ServiceAccountTokenParams, TokenExchangeParams,
};
use introspection_sdk::IntrospectionClient;
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

fn token_response() -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({
        "access_token": "intro_at_abc",
        "token_type": "Bearer",
        "expires_in": 3600,
        "scope": "runtimes:run",
        "dp_url": "https://dp.example.com",
        "project": "proj_1",
    }))
}

/// The decoded form body of the single request the mock received.
async fn captured_form(server: &MockServer) -> Vec<(String, String)> {
    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 1, "expected exactly one token request");
    parse_form(&requests[0])
}

fn parse_form(req: &Request) -> Vec<(String, String)> {
    let content_type = req
        .headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    assert!(
        content_type.starts_with("application/x-www-form-urlencoded"),
        "token POST must be form-encoded, got {content_type:?}"
    );
    String::from_utf8(req.body.clone())
        .unwrap()
        .split('&')
        .filter(|p| !p.is_empty())
        .map(|pair| {
            let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
            (decode(k), decode(v))
        })
        .collect()
}

fn decode(value: &str) -> String {
    percent_decode(value.replace('+', " ").as_bytes())
}

fn percent_decode(bytes: &[u8]) -> String {
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap();
            out.push(u8::from_str_radix(hex, 16).unwrap());
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).unwrap()
}

fn get<'a>(form: &'a [(String, String)], key: &str) -> Option<&'a str> {
    form.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
}

async fn mock_cp() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/oauth/token"))
        .respond_with(token_response())
        .mount(&server)
        .await;
    server
}

#[tokio::test]
async fn service_account_grant_builds_the_form_and_parses_the_response() {
    let server = mock_cp().await;
    let token = service_account_token(
        ServiceAccountTokenParams::builder()
            .client_id("intro_app_1")
            .client_secret("intro_sk_2")
            .project("proj_1")
            .base_api_url(server.uri())
            .build()
            .unwrap(),
    )
    .await
    .unwrap();

    assert_eq!(token.access_token, "intro_at_abc");
    assert_eq!(token.token_type, "Bearer");
    assert_eq!(token.expires_in, 3600);
    assert_eq!(token.scope.as_deref(), Some("runtimes:run"));
    assert_eq!(token.dp_url.as_deref(), Some("https://dp.example.com"));
    // Unmodelled CP fields stay reachable rather than being dropped.
    assert_eq!(token.extra["project"], "proj_1");

    let form = captured_form(&server).await;
    assert_eq!(get(&form, "grant_type"), Some("client_credentials"));
    assert_eq!(get(&form, "client_id"), Some("intro_app_1"));
    assert_eq!(get(&form, "client_secret"), Some("intro_sk_2"));
    assert_eq!(get(&form, "project"), Some("proj_1"));
    // An unset scope must be absent, not empty: the CP caps to the
    // Application's default scope only when the key is missing.
    assert_eq!(get(&form, "scope"), None);
}

#[tokio::test]
async fn service_account_grant_sends_an_explicit_scope() {
    let server = mock_cp().await;
    service_account_token(
        ServiceAccountTokenParams::builder()
            .client_id("intro_app_1")
            .client_secret("intro_sk_2")
            .project("proj_1")
            .scope("runtimes:run tasks:read")
            .base_api_url(server.uri())
            .build()
            .unwrap(),
    )
    .await
    .unwrap();

    let form = captured_form(&server).await;
    assert_eq!(get(&form, "scope"), Some("runtimes:run tasks:read"));
}

#[tokio::test]
async fn token_exchange_defaults_the_subject_token_type() {
    let server = mock_cp().await;
    token_exchange(
        TokenExchangeParams::builder()
            .subject_token("idp_id_token")
            .client_id("intro_app_fed")
            .project("proj_1")
            .base_api_url(server.uri())
            .build()
            .unwrap(),
    )
    .await
    .unwrap();

    let form = captured_form(&server).await;
    assert_eq!(
        get(&form, "grant_type"),
        Some("urn:ietf:params:oauth:grant-type:token-exchange")
    );
    assert_eq!(get(&form, "subject_token"), Some("idp_id_token"));
    assert_eq!(
        get(&form, "subject_token_type"),
        Some("urn:ietf:params:oauth:token-type:id_token")
    );
    assert_eq!(get(&form, "scope"), None);
}

#[tokio::test]
async fn token_exchange_honours_an_explicit_subject_token_type() {
    let server = mock_cp().await;
    token_exchange(
        TokenExchangeParams::builder()
            .subject_token("partner_access_token")
            .client_id("intro_app_fed")
            .project("proj_1")
            .subject_token_type("urn:ietf:params:oauth:token-type:access_token")
            .base_api_url(server.uri())
            .build()
            .unwrap(),
    )
    .await
    .unwrap();

    let form = captured_form(&server).await;
    assert_eq!(
        get(&form, "subject_token_type"),
        Some("urn:ietf:params:oauth:token-type:access_token")
    );
}

#[tokio::test]
async fn authorization_code_grant_builds_the_pkce_form() {
    let server = mock_cp().await;
    authorization_code_token(
        AuthorizationCodeParams::builder()
            .code("auth_code_1")
            .client_id("intro_app_spa")
            .redirect_uri("https://app.example.com/callback?next=/home")
            .code_verifier("verifier_1")
            .base_api_url(server.uri())
            .build()
            .unwrap(),
    )
    .await
    .unwrap();

    let form = captured_form(&server).await;
    assert_eq!(get(&form, "grant_type"), Some("authorization_code"));
    assert_eq!(get(&form, "code"), Some("auth_code_1"));
    assert_eq!(get(&form, "client_id"), Some("intro_app_spa"));
    assert_eq!(get(&form, "code_verifier"), Some("verifier_1"));
    // The `?` and `/` in the redirect URI have to survive the encoding: an
    // unescaped `&` or `=` here would rewrite the form itself.
    assert_eq!(
        get(&form, "redirect_uri"),
        Some("https://app.example.com/callback?next=/home")
    );
}

#[tokio::test]
async fn a_rejected_grant_maps_to_the_typed_api_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/oauth/token"))
        .respond_with(ResponseTemplate::new(401).set_body_json(json!({
            "error": "invalid_client",
            "detail": "client authentication failed",
        })))
        .mount(&server)
        .await;

    let err = service_account_token(
        ServiceAccountTokenParams::builder()
            .client_id("intro_app_1")
            .client_secret("wrong")
            .project("proj_1")
            .base_api_url(server.uri())
            .build()
            .unwrap(),
    )
    .await
    .unwrap_err();

    match err {
        IntrospectionAPIError::Http { status, .. } => assert_eq!(status, 401),
        other => panic!("expected an Http error, got {other:?}"),
    }
}

#[tokio::test]
async fn from_service_account_wires_the_minted_token_and_the_minting_host() {
    let server = mock_cp().await;
    // The client must talk to the CP the token was minted against. Leaving
    // the host to `new()` would send a token minted here to whatever
    // INTROSPECTION_BASE_API_URL happened to say.
    Mock::given(method("GET"))
        .and(path("/v1/runtimes/11111111-1111-1111-1111-111111111111"))
        .and(wiremock::matchers::header(
            "authorization",
            "Bearer intro_at_abc",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "11111111-1111-1111-1111-111111111111",
            "name": "customer-agent",
        })))
        .mount(&server)
        .await;

    let client = IntrospectionClient::from_service_account(
        ServiceAccountTokenParams::builder()
            .client_id("intro_app_1")
            .client_secret("intro_sk_2")
            .project("proj_1")
            .base_api_url(server.uri())
            .build()
            .unwrap(),
        None,
    )
    .await
    .unwrap();

    // The route above matches on the Authorization header, so reaching it at
    // all is the assertion: the client resolved to the minting host and sent
    // the minted bearer token. The response body is a stub, so the decode
    // that follows is not what is under test.
    let _ = client
        .runtimes()
        .get(
            "11111111-1111-1111-1111-111111111111".parse().unwrap(),
            "proj_1",
        )
        .await;

    let requests = server.received_requests().await.unwrap();
    let runtime_get = requests
        .iter()
        .find(|r| r.url.path() == "/v1/runtimes/11111111-1111-1111-1111-111111111111")
        .expect("the runtimes GET never reached the minting host");
    assert_eq!(
        runtime_get.headers.get("authorization").unwrap(),
        "Bearer intro_at_abc"
    );
}

#[tokio::test]
async fn a_trailing_slash_on_the_base_url_does_not_double_up_the_path() {
    let server = mock_cp().await;
    service_account_token(
        ServiceAccountTokenParams::builder()
            .client_id("intro_app_1")
            .client_secret("intro_sk_2")
            .project("proj_1")
            .base_api_url(format!("{}/", server.uri()))
            .build()
            .unwrap(),
    )
    .await
    .unwrap();

    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests[0].url.path(), "/v1/oauth/token");
}

#[tokio::test]
async fn a_form_value_carrying_a_separator_cannot_rewrite_the_form() {
    let server = mock_cp().await;
    // A secret with `&` / `=` in it used to be the classic way to smuggle an
    // extra grant parameter into a hand-rolled form body.
    service_account_token(
        ServiceAccountTokenParams::builder()
            .client_id("intro_app_1")
            .client_secret("a&scope=admin=x")
            .project("proj_1")
            .base_api_url(server.uri())
            .build()
            .unwrap(),
    )
    .await
    .unwrap();

    let form = captured_form(&server).await;
    assert_eq!(get(&form, "client_secret"), Some("a&scope=admin=x"));
    assert_eq!(get(&form, "scope"), None);
    assert_eq!(form.len(), 4);
}
