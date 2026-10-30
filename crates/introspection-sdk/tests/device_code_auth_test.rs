//! The device-code login (`POST /v1/oauth/device/code`, then the token poll)
//! the refresh, development-credential and revoke calls, and
//! `GET /v1/oidc/logout`, against a mock Control Plane.

use introspection_sdk::auth::{
    oidc_logout, DeviceCodeAuth, DeviceCodeAuthConfig, DeviceCodeParams, DeviceTokenPoll,
    DEVICE_CODE_GRANT,
};
use serde_json::json;
use wiremock::matchers::{body_string, body_string_contains, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn auth(cp: &MockServer) -> DeviceCodeAuth {
    DeviceCodeAuth::new(
        DeviceCodeAuthConfig::builder()
            .base_api_url(cp.uri())
            .build()
            .unwrap(),
    )
    .unwrap()
}

fn device_code(interval: u64) -> serde_json::Value {
    json!({
        "device_code": "dev-code",
        "user_code": "ABCD-EFGH",
        "verification_uri": "http://app.test/activate",
        "verification_uri_complete": "http://app.test/activate?user_code=ABCD-EFGH",
        "expires_in": 600,
        "interval": interval,
    })
}

fn oauth_error(error: &str) -> ResponseTemplate {
    ResponseTemplate::new(400)
        .set_body_json(json!({"error": error, "error_description": "from the test"}))
}

fn poll_form() -> String {
    format!(
        "grant_type={}&device_code=dev-code&client_id=cli",
        DEVICE_CODE_GRANT.replace(':', "%3A")
    )
}

#[tokio::test]
async fn request_code_posts_the_cli_client_and_the_narrowing() {
    let cp = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/oauth/device/code"))
        .and(header("content-type", "application/x-www-form-urlencoded"))
        .and(body_string(
            "client_id=cli&project=acme&capabilities=runtimes%2Creviews",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(device_code(5)))
        .expect(1)
        .mount(&cp)
        .await;

    let device = auth(&cp)
        .request_code(
            &DeviceCodeParams::builder()
                .project("acme")
                .capabilities(vec!["runtimes".to_string(), "reviews".to_string()])
                .build()
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(device.user_code, "ABCD-EFGH");
    assert_eq!(device.interval, 5);
}

#[tokio::test]
async fn an_unknown_capability_is_the_servers_400() {
    let cp = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/oauth/device/code"))
        .respond_with(
            ResponseTemplate::new(400).set_body_json(json!({"detail": "Unknown capabilities: x"})),
        )
        .mount(&cp)
        .await;
    let err = auth(&cp)
        .request_code(
            &DeviceCodeParams::builder()
                .capabilities(vec!["x".to_string()])
                .build()
                .unwrap(),
        )
        .await
        .unwrap_err();
    assert_eq!(err.status(), Some(400));
}

#[tokio::test]
async fn poll_token_classifies_the_rfc_8628_answers() {
    for (error, expected) in [
        ("authorization_pending", "Pending"),
        ("slow_down", "SlowDown"),
        ("access_denied", "AccessDenied"),
        ("expired_token", "Expired"),
    ] {
        let cp = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/oauth/token"))
            .and(body_string(poll_form()))
            .respond_with(oauth_error(error))
            .mount(&cp)
            .await;
        let poll = auth(&cp).poll_token("dev-code").await.unwrap();
        assert_eq!(format!("{poll:?}"), expected, "{error}");
    }

    let cp = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/oauth/token"))
        .respond_with(oauth_error("invalid_grant"))
        .mount(&cp)
        .await;
    let err = auth(&cp).poll_token("dev-code").await.unwrap_err();
    assert_eq!(err.code(), Some("invalid_grant"));
}

#[tokio::test]
async fn wait_for_token_polls_past_pending_to_the_login() {
    let cp = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/oauth/token"))
        .and(body_string_contains("device_code=dev-code"))
        .respond_with(oauth_error("authorization_pending"))
        .up_to_n_times(1)
        .expect(1)
        .mount(&cp)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/oauth/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "at",
            "token_type": "Bearer",
            "expires_in": 3600,
            "refresh_token": "rt",
            "session_id": "s",
            "org_id": "o",
            "cp_session": "cookie",
            "cp_url": "http://cp.test",
            "dp_url": "http://dp.test",
        })))
        .expect(1)
        .mount(&cp)
        .await;

    let auth = auth(&cp);
    let device = serde_json::from_value(device_code(1)).unwrap();
    let token = auth.wait_for_token(&device).await.unwrap();
    assert_eq!(token.access_token, "at");
    assert_eq!(token.cp_session.as_deref(), Some("cookie"));
    assert_eq!(token.dp_url.as_deref(), Some("http://dp.test"));
}

#[tokio::test]
async fn wait_for_token_ends_on_a_denial() {
    let cp = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/oauth/token"))
        .respond_with(oauth_error("access_denied"))
        .mount(&cp)
        .await;
    let device = serde_json::from_value(device_code(1)).unwrap();
    let err = auth(&cp).wait_for_token(&device).await.unwrap_err();
    assert_eq!(err.code(), Some("access_denied"));
    assert!(matches!(
        auth(&cp).poll_token("dev-code").await,
        Ok(DeviceTokenPoll::AccessDenied)
    ));
}

#[tokio::test]
async fn logout_sends_the_cookie_and_does_not_follow_the_redirect() {
    let cp = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/oidc/logout"))
        .and(header("cookie", "intro_cp_session=cookie"))
        .respond_with(
            ResponseTemplate::new(307).insert_header("location", "http://idp.invalid/end_session"),
        )
        .expect(1)
        .mount(&cp)
        .await;
    oidc_logout(Some(&cp.uri()), "cookie").await.unwrap();
}

#[tokio::test]
async fn logout_reports_a_server_error() {
    let cp = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/oidc/logout"))
        .respond_with(ResponseTemplate::new(500).set_body_json(json!({"detail": "boom"})))
        .mount(&cp)
        .await;
    let err = oidc_logout(Some(&cp.uri()), "cookie").await.unwrap_err();
    assert_eq!(err.status(), Some(500));
}

#[tokio::test]
async fn refresh_posts_the_keys_the_login_returned() {
    let cp = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/oauth/token"))
        .and(body_string(
            "grant_type=refresh_token&refresh_token=rt&client_id=cli&session_id=s&org_id=o",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "at2",
            "token_type": "Bearer",
            "expires_in": 3600,
            "session_id": "s",
            "org_id": "o",
        })))
        .expect(1)
        .mount(&cp)
        .await;
    let token = auth(&cp).refresh("rt", "s", "o").await.unwrap();
    assert_eq!(token.access_token, "at2");
    assert_eq!(token.refresh_token, None, "not rotated: keep the old one");

    let cp = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/oauth/token"))
        .respond_with(oauth_error("invalid_grant"))
        .mount(&cp)
        .await;
    let err = auth(&cp).refresh("rt", "s", "o").await.unwrap_err();
    assert_eq!(err.code(), Some("invalid_grant"));
}

#[tokio::test]
async fn development_credential_exchanges_the_login_access_token() {
    let cp = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/oauth/token"))
        .and(body_string(
            "grant_type=urn%3Aietf%3Aparams%3Aoauth%3Agrant-type%3Atoken-exchange&client_id=cli\
             &subject_token=login&subject_token_type=urn%3Aietf%3Aparams%3Aoauth%3Atoken-type%3Aaccess_token",
        ))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"access_token": "dev", "token_type": "Bearer", "expires_in": 300})),
        )
        .expect(1)
        .mount(&cp)
        .await;
    let credential = auth(&cp).development_credential("login").await.unwrap();
    assert_eq!(credential.access_token, "dev");
    assert_eq!(credential.expires_in, 300);
}

#[tokio::test]
async fn revoke_ends_the_login_session() {
    let cp = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/oauth/revoke"))
        .and(body_string("client_id=cli&session_id=s&org_id=o&token=rt"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"status": "revoked"})))
        .expect(1)
        .mount(&cp)
        .await;
    let revoked = auth(&cp).revoke("s", "o", Some("rt")).await.unwrap();
    assert_eq!(revoked.status, "revoked");
}

#[tokio::test]
async fn revoke_surfaces_a_refusal_as_an_http_error() {
    let cp = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/oauth/revoke"))
        .respond_with(oauth_error("invalid_client"))
        .expect(1)
        .mount(&cp)
        .await;
    let err = auth(&cp).revoke("s", "o", None).await.unwrap_err();
    assert_eq!(err.status(), Some(400));
}
