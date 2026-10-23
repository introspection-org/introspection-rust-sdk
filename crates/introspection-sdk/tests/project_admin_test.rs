//! What the project-administration namespaces (API keys, Applications,
//! credentials, variables, integrations, judges, projects, `oidc/me`) and the
//! eval namespaces (images, task commands) send and decode, backed by
//! `wiremock`. `live_api_test` pins that a deployment answers the same calls.

use futures::StreamExt;
use introspection_sdk::{
    AdvancedOptions, ApiKeyCapability, ApiKeyCreateParams, ApiKeyDeleteParams, ApiKeyListParams,
    ApiKeyStatus, ApplicationCreateParams, ApplicationIdpCreateParams, ApplicationIdpProvider,
    ApplicationListParams, ApplicationType, ClientConfig, CredentialCreateParams,
    CredentialUpdateParams, FeatureFlagValue, ImageCreate, ImageKind, ImageListParams, ImageSource,
    ImageStatus, IntegrationCreateParams, IntegrationStatus, IntegrationType, IntrospectionClient,
    JudgeUpdateParams, Lane, PaginationParams, RuntimeSettingListParams, Secret,
    TaskCommandListParams, TaskCommandSpec, TaskCommandStatus, VariableCreateParams,
    MAX_COMMAND_WAIT_SECONDS,
};
use serde_json::json;
use uuid::Uuid;
use wiremock::matchers::{body_json, method, path, query_param, query_param_is_missing};
use wiremock::{Mock, MockServer, ResponseTemplate};

const ID: &str = "11111111-1111-4111-8111-111111111111";
const ORG: &str = "00000000-0000-0000-0000-0000000000aa";
const PROJECT: &str = "00000000-0000-0000-0000-0000000000bb";
const MEMBER: &str = "00000000-0000-0000-0000-0000000000cc";
const NOW: &str = "2026-10-19T00:00:00Z";

fn id() -> Uuid {
    Uuid::parse_str(ID).unwrap()
}

fn client(server: &MockServer) -> IntrospectionClient {
    IntrospectionClient::new(
        ClientConfig::with_token("intro_test").advanced(AdvancedOptions {
            base_api_url: Some(server.uri()),
            dp_url: Some(server.uri()),
            ..Default::default()
        }),
    )
    .expect("client")
}

fn page(records: Vec<serde_json::Value>) -> serde_json::Value {
    json!({ "records": records, "count": records.len(), "next": null })
}

fn api_key_json() -> serde_json::Value {
    json!({
        "id": ID, "org_id": ORG, "project_id": PROJECT, "created_at": NOW,
        "name": "ci", "environment": "production", "allow": ["evals"], "scope": "*",
        "member_id": null, "key_prefix": "intro_", "key_suffix": "abcd",
        "redacted_key": "intro_…abcd", "status": "active", "expires_at": null,
        "last_used_at": null, "acknowledged_at": null, "key_display": "intro_…abcd",
        "prefix": "intro_",
    })
}

fn application_json() -> serde_json::Value {
    json!({
        "id": ID, "org_id": ORG, "project_id": PROJECT, "created_at": NOW, "updated_at": NOW,
        "name": "web", "application_type": "spa", "environment": "production",
        "redirect_uris": ["https://app.example.com/cb"], "allowed_origins": [],
        "allowed_scopes": ["openid"], "client_id": "app_123", "agent_member_id": null,
        "allowed_grants": ["authorization_code", "refresh_token"], "last_used_at": null,
        "revoked_at": null, "created_by_member_id": MEMBER,
    })
}

fn credential_json() -> serde_json::Value {
    json!({
        "id": ID, "org_id": ORG, "project_id": PROJECT, "created_at": NOW, "updated_at": NOW,
        "name": "OPENAI_API_KEY", "runtime_id": null, "runtime_group_id": null,
        "environment": null, "has_secret": true, "secret_display": "sk-…wxyz",
        "secret_set_at": NOW, "created_by_member_id": MEMBER,
    })
}

#[tokio::test]
async fn api_keys_create_names_the_project_and_redacts_the_plaintext() {
    let server = MockServer::start().await;
    let mut created = api_key_json();
    created["api_key"] = json!("intro_plaintext_abcd");
    Mock::given(method("POST"))
        .and(path("/v1/api-keys"))
        .and(query_param("project", "acme/web"))
        .and(body_json(json!({ "name": "ci", "allow": ["evals"] })))
        .respond_with(ResponseTemplate::new(201).set_body_json(created))
        .expect(1)
        .mount(&server)
        .await;

    let created = client(&server)
        .api_keys()
        .create(
            &ApiKeyCreateParams {
                name: Some("ci".into()),
                allow: Some(vec![ApiKeyCapability::Evals]),
                ..Default::default()
            },
            Some("acme/web".into()),
        )
        .await
        .expect("create");

    assert_eq!(created.key.status, ApiKeyStatus::Active);
    let plaintext = created.api_key.as_ref().expect("plaintext on create");
    assert_eq!(plaintext.expose(), "intro_plaintext_abcd");
    assert!(!format!("{created:?}").contains("intro_plaintext_abcd"));
}

#[tokio::test]
async fn api_keys_list_unwraps_the_envelope_and_delete_sends_the_grace() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/api-keys"))
        .and(query_param("expiring", "true"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({ "api_keys": [api_key_json()] })),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path(format!("/v1/api-keys/{ID}")))
        .and(query_param("project", "web"))
        .and(query_param("expires_in", "3600"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(format!("/v1/api-keys/{ID}/acknowledge")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "acknowledged": true })))
        .expect(1)
        .mount(&server)
        .await;

    let client = client(&server);
    let keys = client
        .api_keys()
        .list(&ApiKeyListParams {
            expiring: Some(true),
            ..Default::default()
        })
        .await
        .expect("list");
    assert_eq!(keys.len(), 1);
    assert_eq!(keys[0].allow, vec![ApiKeyCapability::Evals]);
    client
        .api_keys()
        .delete(
            id(),
            &ApiKeyDeleteParams {
                project: Some("web".into()),
                expires_in: Some(3600),
            },
        )
        .await
        .expect("delete");
    assert!(
        client
            .api_keys()
            .acknowledge(id(), None)
            .await
            .expect("acknowledge")
            .acknowledged
    );
}

#[tokio::test]
async fn applications_crud_and_subresources_address_the_application() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/applications"))
        .and(query_param("project", "web"))
        .and(body_json(json!({
            "name": "web",
            "application_type": "spa",
            "redirect_uris": ["https://app.example.com/cb"],
        })))
        .respond_with(ResponseTemplate::new(201).set_body_json(application_json()))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1/applications"))
        .and(query_param("project", "web"))
        .respond_with(ResponseTemplate::new(200).set_body_json(page(vec![application_json()])))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(format!("/v1/applications/{ID}/idps")))
        .and(body_json(json!({
            "idp_provider": "okta",
            "issuer": "https://acme.okta.com",
            "client_id": "okta_client",
            "client_secret": "okta_secret",
        })))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({
            "id": ID, "org_id": ORG, "application_id": ID, "created_at": NOW, "updated_at": NOW,
            "idp_provider": "okta", "issuer": "https://acme.okta.com", "client_id": "okta_client",
            "scopes": ["openid"], "status": "pending", "is_active": false,
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path(format!("/v1/applications/{ID}/secrets/{ID}")))
        .and(query_param_is_missing("expires_in"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/v1/applications/{ID}/keys")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .expect(1)
        .mount(&server)
        .await;

    let client = client(&server);
    let mut create = ApplicationCreateParams::new("web", ApplicationType::Spa);
    create.redirect_uris = Some(vec!["https://app.example.com/cb".into()]);
    let app = client
        .applications()
        .create(&create, Some("web".into()))
        .await
        .expect("create");
    assert_eq!(app.application_type, ApplicationType::Spa);

    let listed: Vec<_> = client
        .applications()
        .list(&ApplicationListParams {
            project: Some("web".into()),
            ..Default::default()
        })
        .collect()
        .await;
    assert_eq!(listed.len(), 1);

    let idp = client
        .applications()
        .idps
        .create(
            app.id,
            &ApplicationIdpCreateParams {
                idp_provider: Some(ApplicationIdpProvider::Okta),
                issuer: Some("https://acme.okta.com".into()),
                client_id: Some("okta_client".into()),
                client_secret: Some(Secret::new("okta_secret")),
                ..Default::default()
            },
        )
        .await
        .expect("idps.create");
    assert_eq!(idp.idp_provider, ApplicationIdpProvider::Okta);

    client
        .applications()
        .secrets
        .delete(app.id, id(), None)
        .await
        .expect("secrets.delete");
    assert!(client
        .applications()
        .keys
        .list(app.id)
        .await
        .expect("keys.list")
        .is_empty());
}

#[tokio::test]
async fn credentials_send_the_secret_but_never_print_it() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/credentials"))
        .and(body_json(json!({
            "name": "OPENAI_API_KEY",
            "secret": "sk-live",
            "environment": "staging",
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(credential_json()))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("PATCH"))
        .and(path(format!("/v1/credentials/{ID}")))
        .and(body_json(json!({ "secret": "sk-rotated" })))
        .respond_with(ResponseTemplate::new(200).set_body_json(credential_json()))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1/credentials"))
        .and(query_param("runtime", "support"))
        .and(query_param("environment", "staging"))
        .respond_with(ResponseTemplate::new(200).set_body_json(page(vec![credential_json()])))
        .expect(1)
        .mount(&server)
        .await;

    let client = client(&server);
    let mut create = CredentialCreateParams::new("OPENAI_API_KEY", "sk-live");
    create.environment = Some(Lane::Staging);
    assert!(!format!("{create:?}").contains("sk-live"));
    let credential = client
        .credentials()
        .create(&create, None)
        .await
        .expect("create");
    assert!(credential.has_secret);
    client
        .credentials()
        .update(
            credential.id,
            &CredentialUpdateParams {
                secret: Some("sk-rotated".into()),
                ..Default::default()
            },
            None,
        )
        .await
        .expect("update");
    let listed = client
        .credentials()
        .list(&RuntimeSettingListParams {
            runtime: Some("support".into()),
            environment: Some(Lane::Staging),
            ..Default::default()
        })
        .collect_all(1)
        .await
        .expect("list");
    assert_eq!(listed.len(), 1);
}

#[tokio::test]
async fn variables_integrations_and_judges_send_their_wire_names() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/variables"))
        .and(body_json(json!({ "name": "LOG_LEVEL", "value": "debug" })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": ID, "org_id": ORG, "project_id": PROJECT, "created_at": NOW,
            "updated_at": NOW, "name": "LOG_LEVEL", "kind": "value", "value": "debug",
            "created_by_member_id": MEMBER,
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/integrations"))
        .and(body_json(json!({ "type": "custom", "name": "internal" })))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({
            "id": ID, "org_id": ORG, "created_at": NOW, "updated_at": NOW, "type": "custom",
            "name": "internal", "status": "pending", "created_by_member_id": MEMBER,
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("PATCH"))
        .and(path(format!("/v1/judges/{ID}")))
        .and(query_param("project", "web"))
        .and(body_json(json!({ "enabled": false, "sample_rate": 0.25 })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": ID, "org_id": ORG, "project_id": PROJECT, "runtime_group_id": ID,
            "created_at": NOW, "updated_at": NOW, "name": "helpfulness", "enabled": false,
            "sample_rate": 0.25, "source": "recipe", "definition_hash": "abc",
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = client(&server);
    let variable = client
        .variables()
        .create(&VariableCreateParams::new("LOG_LEVEL", "debug"), None)
        .await
        .expect("variables.create");
    assert_eq!(variable.value.as_deref(), Some("debug"));

    let mut create = IntegrationCreateParams::new(IntegrationType::Custom);
    create.name = Some("internal".into());
    let integration = client
        .integrations()
        .create(&create)
        .await
        .expect("integrations.create");
    assert_eq!(integration.integration_type, IntegrationType::Custom);
    assert_eq!(integration.status, IntegrationStatus::Pending);

    let judge = client
        .judges()
        .update(
            id(),
            &JudgeUpdateParams {
                enabled: Some(false),
                sample_rate: Some(0.25),
            },
            Some("web".into()),
        )
        .await
        .expect("judges.update");
    assert!(!judge.enabled);
}

#[tokio::test]
async fn projects_get_encodes_the_slug_and_me_decodes_flags() {
    let server = MockServer::start().await;
    let project = json!({
        "id": PROJECT, "org_id": ORG, "deployment_id": ID, "name": "Web",
        "slug": "web", "description": null,
    });
    Mock::given(method("GET"))
        .and(path("/v1/projects/a%2Fb"))
        .respond_with(ResponseTemplate::new(200).set_body_json(project))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1/oidc/me"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "org_id": ORG, "member_id": MEMBER, "email": "owner@example.com",
            "feature_flags": { "evals": true, "project_limit": 5, "tier": "pro" },
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = client(&server);
    let fetched = client.projects().get("a/b").await.expect("get");
    assert_eq!(fetched.slug.as_deref(), Some("web"));

    let me = client.oidc().me().await.expect("me");
    assert!(me.authenticated);
    assert_eq!(me.feature_flags["evals"], FeatureFlagValue::Bool(true));
    assert_eq!(me.feature_flags["project_limit"], FeatureFlagValue::Int(5));
}

#[tokio::test]
async fn images_send_a_tagged_source_and_filter_the_list() {
    let server = MockServer::start().await;
    let reference = format!("ghcr.io/acme/db:16@sha256:{}", "a".repeat(64));
    let image = json!({
        "id": ID, "kind": "service", "name": "acme/db", "content_id": "c",
        "source_kind": "mirror", "source": { "kind": "mirror", "reference": reference },
        "build_platform": "linux/amd64", "status": "ready", "image_ref": "registry/acme/db",
        "image_digest": "sha256:feed", "created_at": NOW, "updated_at": NOW,
    });
    Mock::given(method("POST"))
        .and(path("/v1/images"))
        .and(body_json(json!({
            "name": "acme/db",
            "source": { "kind": "mirror", "reference": reference },
        })))
        .respond_with(ResponseTemplate::new(201).set_body_json(image.clone()))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1/images"))
        .and(query_param("kind", "service"))
        .and(query_param("status", "ready"))
        .respond_with(ResponseTemplate::new(200).set_body_json(page(vec![image])))
        .expect(1)
        .mount(&server)
        .await;

    let client = client(&server);
    let created = client
        .images()
        .create(&ImageCreate::new(
            "acme/db",
            ImageSource::Mirror {
                reference: reference.clone(),
            },
        ))
        .await
        .expect("create");
    assert_eq!(
        created.pull_reference().as_deref(),
        Some("registry/acme/db@sha256:feed")
    );
    let listed = client
        .images()
        .list(&ImageListParams {
            kind: Some(ImageKind::Service),
            status: Some(ImageStatus::Ready),
            ..Default::default()
        })
        .collect_all(1)
        .await
        .expect("list");
    assert_eq!(listed.len(), 1);
}

#[tokio::test]
async fn task_commands_send_an_argv_and_cap_the_wait() {
    let server = MockServer::start().await;
    let command = json!({
        "id": ID, "task_id": ID, "kind": "exec", "spec": { "kind": "exec", "command": ["ls"] },
        "status": "completed", "exit_code": 0, "stdout": "a\n", "created_at": NOW,
        "updated_at": NOW,
    });
    Mock::given(method("POST"))
        .and(path(format!("/v1/tasks/{ID}/commands")))
        .and(body_json(
            json!({ "kind": "exec", "command": ["ls", "-la"] }),
        ))
        .respond_with(ResponseTemplate::new(201).set_body_json(command.clone()))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/v1/tasks/{ID}/commands/{ID}")))
        .and(query_param(
            "wait_seconds",
            MAX_COMMAND_WAIT_SECONDS.to_string().as_str(),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(command.clone()))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/v1/tasks/{ID}/commands")))
        .and(query_param("status", "failed"))
        .and(query_param("limit", "5"))
        .respond_with(ResponseTemplate::new(200).set_body_json(page(vec![])))
        .expect(1)
        .mount(&server)
        .await;

    let tasks = client(&server).tasks();
    let created = tasks
        .commands
        .create(ID, &TaskCommandSpec::exec(["ls", "-la"]))
        .await
        .expect("create");
    assert!(created.status.is_terminal());
    let read = tasks
        .commands
        .get(ID, ID, Some(60))
        .await
        .expect("get waits at most the cap");
    assert_eq!(read.exit_code, Some(0));
    let listed = tasks
        .commands
        .list(
            ID,
            &TaskCommandListParams {
                pagination: PaginationParams {
                    limit: Some(5),
                    next: None,
                },
                status: Some(TaskCommandStatus::Failed),
            },
        )
        .collect_all(1)
        .await
        .expect("list");
    assert!(listed.is_empty());
}
