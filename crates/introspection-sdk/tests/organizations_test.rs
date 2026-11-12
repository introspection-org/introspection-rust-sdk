//! Integration tests for `client.organizations`, backed by `wiremock`.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use introspection_sdk::api::{HttpClient, HttpConfig};
use introspection_sdk::{Organizations, PasskeyRequirement, SignInSettings};
use serde_json::json;
use wiremock::matchers::{method, path};
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

fn organization_json() -> serde_json::Value {
    json!({
        "id": "00000000-0000-0000-0000-0000000000aa",
        "name": "Acme",
        "slug": "acme",
        "external_org_id": "3001",
        "image_url": null,
        "hosted_git": true,
        "subscription_live": true,
    })
}

async fn me(body: serde_json::Value) -> introspection_sdk::Organization {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/organizations/me"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(&server)
        .await;
    Organizations::new(build_http(&server)).me().await.unwrap()
}

#[tokio::test]
async fn me_decodes_enterprise_sign_in_settings() {
    let mut body = organization_json();
    body["sign_in"] = json!({
        "passkey_requirement": "except_sso",
        "restrict_invites_to_domains": true,
    });
    let org = me(body).await;
    assert_eq!(
        org.sign_in,
        Some(SignInSettings {
            passkey_requirement: PasskeyRequirement::ExceptSso,
            restrict_invites_to_domains: true,
        })
    );
}

#[tokio::test]
async fn me_without_enterprise_sign_in_has_none() {
    assert_eq!(me(organization_json()).await.sign_in, None);
    let mut body = organization_json();
    body["sign_in"] = serde_json::Value::Null;
    assert_eq!(me(body).await.sign_in, None);
}

#[test]
fn sign_in_settings_default_and_keep_an_unknown_requirement() {
    let settings: SignInSettings = serde_json::from_value(json!({})).unwrap();
    assert_eq!(settings, SignInSettings::default());
    assert_eq!(settings.passkey_requirement, PasskeyRequirement::Off);

    let settings: SignInSettings =
        serde_json::from_value(json!({"passkey_requirement": "hardware_key"})).unwrap();
    assert_eq!(
        settings.passkey_requirement,
        PasskeyRequirement::Other("hardware_key".into())
    );
    assert_eq!(
        serde_json::to_value(&settings).unwrap()["passkey_requirement"],
        "hardware_key"
    );
}

#[tokio::test]
async fn me_reads_whether_the_subscription_is_live() {
    assert_eq!(me(organization_json()).await.subscription_live, Some(true));
    let mut body = organization_json();
    body.as_object_mut().unwrap().remove("subscription_live");
    assert_eq!(me(body).await.subscription_live, None);
}
