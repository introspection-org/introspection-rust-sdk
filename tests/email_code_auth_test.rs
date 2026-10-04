//! Native email-code sign-in against a mock Control Plane and Data Plane.
//!
//! The parts that only exist on the wire are what break: the send route's
//! JSON body, the URN grant type, the refresh grant's `session_id` /
//! `org_id`, a `dp_url` the refresh response leaves out, and the order in
//! which a slow response lands against a sign-out.

use std::time::Duration;

use introspection_sdk::api::IntrospectionAPIError;
use introspection_sdk::auth::{
    AuthChangeEvent, AuthSession, EmailCodeAuth, EmailCodeAuthConfig, EMAIL_CODE_GRANT,
};
use introspection_sdk::{TaskListParams, Tasks};
use serde_json::{json, Value};
use wiremock::matchers::{body_json, header, method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

const CLIENT_ID: &str = "intro_app_native";
const PROJECT: &str = "acme";

fn auth(cp: &MockServer) -> EmailCodeAuth {
    EmailCodeAuth::new(
        EmailCodeAuthConfig::builder()
            .client_id(CLIENT_ID)
            .project(PROJECT)
            .base_api_url(cp.uri())
            .build()
            .unwrap(),
    )
    .unwrap()
}

fn signed_in_body(access: &str, refresh: &str, expires_in: i64, dp_url: &str) -> Value {
    json!({
        "access_token": access,
        "token_type": "Bearer",
        "expires_in": expires_in,
        "refresh_token": refresh,
        "scope": "tasks:read tasks:write",
        "session_id": "0199a000-0000-7000-8000-000000000001",
        "org_id": "0199a000-0000-7000-8000-0000000000aa",
        "project_id": "0199a000-0000-7000-8000-0000000000bb",
        "dp_url": dp_url,
        "member_id": "0199a000-0000-7000-8000-0000000000cc",
        "member_name": "user@example.com",
    })
}

/// The refresh grant answers without `dp_url`.
fn refreshed_body(access: &str, refresh: &str) -> Value {
    json!({
        "access_token": access,
        "token_type": "Bearer",
        "expires_in": 900,
        "refresh_token": refresh,
        "session_id": "0199a000-0000-7000-8000-000000000001",
        "org_id": "0199a000-0000-7000-8000-0000000000aa",
    })
}

fn form(req: &Request) -> Vec<(String, String)> {
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
    let bytes = value.replace('+', " ").into_bytes();
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

fn field<'a>(form: &'a [(String, String)], key: &str) -> Option<&'a str> {
    form.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
}

async fn token_requests(cp: &MockServer) -> Vec<Vec<(String, String)>> {
    cp.received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|r| r.url.path() == "/v1/oauth/token")
        .map(form)
        .collect()
}

#[tokio::test]
async fn send_code_posts_the_client_email_and_project_as_json() {
    let cp = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/oauth/email/code"))
        .and(body_json(json!({
            "client_id": CLIENT_ID,
            "email": "user@example.com",
            "project": PROJECT,
        })))
        .respond_with(ResponseTemplate::new(202))
        .expect(1)
        .mount(&cp)
        .await;

    auth(&cp).send_code("user@example.com").await.unwrap();
}

#[tokio::test]
async fn a_send_rate_limit_carries_its_code_and_wait() {
    let cp = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/oauth/email/code"))
        .respond_with(
            ResponseTemplate::new(429)
                .insert_header("Retry-After", "42")
                .set_body_json(json!({
                    "error": "slow_down",
                    "error_description": "Too many codes requested; try again later",
                })),
        )
        .mount(&cp)
        .await;

    let err = auth(&cp).send_code("user@example.com").await.unwrap_err();
    assert_eq!(err.status(), Some(429));
    assert_eq!(err.code(), Some("slow_down"));
    assert_eq!(err.retry_after(), Some(Duration::from_secs(42)));
    assert!(err.to_string().contains("Too many codes requested"));
}

#[tokio::test]
async fn verify_code_exchanges_a_letter_code_for_a_refreshable_session() {
    let cp = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/oauth/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(signed_in_body(
            "at1",
            "rt1",
            900,
            "https://dp.example.com",
        )))
        .mount(&cp)
        .await;

    let auth = auth(&cp);
    let changes = auth.changes();
    // A new user's first code is six characters of A-Z0-9, not six digits.
    let session = auth
        .verify_code("user@example.com", " K7Q2ZD\n")
        .await
        .unwrap();

    let sent = token_requests(&cp).await;
    assert_eq!(sent.len(), 1);
    assert_eq!(field(&sent[0], "grant_type"), Some(EMAIL_CODE_GRANT));
    assert_eq!(field(&sent[0], "client_id"), Some(CLIENT_ID));
    assert_eq!(field(&sent[0], "email"), Some("user@example.com"));
    assert_eq!(field(&sent[0], "code"), Some("K7Q2ZD"));
    assert_eq!(field(&sent[0], "project"), Some(PROJECT));

    assert_eq!(session.access_token, "at1");
    assert_eq!(session.refresh_token.as_deref(), Some("rt1"));
    assert_eq!(session.dp_url.as_deref(), Some("https://dp.example.com"));
    assert_eq!(
        session.member_id.as_deref(),
        Some("0199a000-0000-7000-8000-0000000000cc")
    );
    assert!(session.expires_at.is_some());

    let state = changes.borrow().clone();
    assert_eq!(state.event, AuthChangeEvent::SignedIn);
    assert_eq!(state.session, Some(session));
}

#[tokio::test]
async fn a_rejected_code_is_invalid_grant_and_leaves_the_user_signed_out() {
    let cp = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/oauth/token"))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({
            "error": "invalid_grant",
            "error_description": "The code is invalid or has expired",
        })))
        .mount(&cp)
        .await;

    let auth = auth(&cp);
    let err = auth
        .verify_code("user@example.com", "000000")
        .await
        .unwrap_err();
    assert_eq!(err.code(), Some("invalid_grant"));
    assert!(auth.current_session().is_none());
}

#[tokio::test]
async fn a_session_near_expiry_is_refreshed_before_it_is_handed_out() {
    let cp = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/oauth/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(refreshed_body("at2", "rt2")))
        .expect(1)
        .mount(&cp)
        .await;

    let auth = auth(&cp);
    // 30 s left is inside the default 60 s leeway.
    auth.set_session(AuthSession::from_token(
        &serde_json::from_value(signed_in_body("at1", "rt1", 30, "https://dp.example.com"))
            .unwrap(),
        std::time::SystemTime::now(),
        None,
    ));
    let changes = auth.changes();

    assert_eq!(auth.access_token().await.unwrap(), "at2");

    let sent = token_requests(&cp).await;
    assert_eq!(field(&sent[0], "grant_type"), Some("refresh_token"));
    assert_eq!(field(&sent[0], "client_id"), Some(CLIENT_ID));
    assert_eq!(field(&sent[0], "refresh_token"), Some("rt1"));
    assert_eq!(
        field(&sent[0], "session_id"),
        Some("0199a000-0000-7000-8000-000000000001")
    );
    assert_eq!(
        field(&sent[0], "org_id"),
        Some("0199a000-0000-7000-8000-0000000000aa")
    );

    let refreshed = auth.current_session().unwrap();
    assert_eq!(refreshed.refresh_token.as_deref(), Some("rt2"));
    // Not in the refresh response, so kept from the sign-in.
    assert_eq!(refreshed.dp_url.as_deref(), Some("https://dp.example.com"));
    assert_eq!(changes.borrow().event, AuthChangeEvent::TokenRefreshed);
}

async fn signed_in(cp: &MockServer, dp_url: &str) -> EmailCodeAuth {
    let auth = auth(cp);
    auth.set_session(AuthSession::from_token(
        &serde_json::from_value(signed_in_body("at1", "rt1", 900, dp_url)).unwrap(),
        std::time::SystemTime::now(),
        None,
    ));
    auth
}

#[tokio::test]
async fn concurrent_refreshes_share_one_request() {
    let cp = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/oauth/token"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(refreshed_body("at2", "rt2"))
                .set_delay(Duration::from_millis(100)),
        )
        .expect(1)
        .mount(&cp)
        .await;

    let auth = signed_in(&cp, "https://dp.example.com").await;
    let (a, b) = tokio::join!(auth.refresh(), auth.refresh_after_unauthorized("at1"));
    assert_eq!(a.unwrap().access_token, "at2");
    assert_eq!(b.unwrap().access_token, "at2");
}

#[tokio::test]
async fn a_rejected_refresh_signs_out_but_an_outage_does_not() {
    let cp = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/oauth/token"))
        .respond_with(ResponseTemplate::new(503).set_body_json(json!({
            "error": "temporarily_unavailable",
            "error_description": "try again",
        })))
        .up_to_n_times(1)
        .mount(&cp)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/oauth/token"))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({
            "error": "invalid_grant",
            "error_description": "Invalid or expired refresh_token; please re-authorize.",
        })))
        .mount(&cp)
        .await;

    let auth = signed_in(&cp, "https://dp.example.com").await;
    let changes = auth.changes();

    let outage = auth.refresh().await.unwrap_err();
    assert_eq!(outage.status(), Some(503));
    assert!(auth.current_session().is_some());

    let rejected = auth.refresh().await.unwrap_err();
    assert_eq!(rejected.code(), Some("invalid_grant"));
    assert!(auth.current_session().is_none());
    assert_eq!(changes.borrow().event, AuthChangeEvent::SignedOut);
}

#[tokio::test]
async fn a_sign_in_answered_after_a_sign_out_is_dropped() {
    let cp = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/oauth/token"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(signed_in_body("at1", "rt1", 900, "https://dp.example.com"))
                .set_delay(Duration::from_millis(200)),
        )
        .mount(&cp)
        .await;

    let auth = auth(&cp);
    let verifying = {
        let auth = auth.clone();
        tokio::spawn(async move { auth.verify_code("user@example.com", "123456").await })
    };
    tokio::time::sleep(Duration::from_millis(50)).await;
    auth.sign_out().await.unwrap();

    let err = verifying.await.unwrap().unwrap_err();
    assert!(matches!(err, IntrospectionAPIError::Superseded(_)), "{err}");
    assert!(auth.current_session().is_none());
}

#[tokio::test]
async fn the_data_plane_is_retried_once_with_a_refreshed_token_after_a_401() {
    let cp = MockServer::start().await;
    let dp = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/oauth/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(refreshed_body("at2", "rt2")))
        .expect(1)
        .mount(&cp)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1/tasks"))
        .and(header("authorization", "Bearer at1"))
        .respond_with(ResponseTemplate::new(401).set_body_json(json!({"detail": "expired"})))
        .mount(&dp)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1/tasks"))
        .and(header("authorization", "Bearer at2"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"records": [], "count": 0, "next": null})),
        )
        .expect(1)
        .mount(&dp)
        .await;

    let auth = signed_in(&cp, &dp.uri()).await;
    let page = auth
        .with_data_plane(|dp| async move {
            Tasks::new(dp)
                .list(&TaskListParams::default())
                .next_page()
                .await
        })
        .await
        .unwrap()
        .expect("one page");
    assert_eq!(page.count, 0);
}

#[tokio::test]
async fn sign_out_clears_the_session_and_revokes_it() {
    let cp = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/oauth/revoke"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"status": "revoked"})))
        .expect(1)
        .mount(&cp)
        .await;

    let auth = signed_in(&cp, "https://dp.example.com").await;
    auth.sign_out().await.unwrap();
    assert!(auth.current_session().is_none());

    let requests = cp.received_requests().await.unwrap();
    let sent = form(&requests[0]);
    assert_eq!(field(&sent, "client_id"), Some(CLIENT_ID));
    assert_eq!(
        field(&sent, "session_id"),
        Some("0199a000-0000-7000-8000-000000000001")
    );
    assert_eq!(
        field(&sent, "org_id"),
        Some("0199a000-0000-7000-8000-0000000000aa")
    );
}

#[tokio::test]
async fn the_client_targets_the_sessions_data_plane() {
    let cp = MockServer::start().await;
    let dp = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/automations"))
        .and(header("authorization", "Bearer at1"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"records": [], "count": 0, "next": null})),
        )
        .expect(1)
        .mount(&dp)
        .await;

    let auth = signed_in(&cp, &dp.uri()).await;
    let client = auth.client(None).await.unwrap();
    let page = client
        .automations()
        .list(&Default::default())
        .next_page()
        .await
        .unwrap()
        .unwrap();
    assert_eq!(page.count, 0);
}
