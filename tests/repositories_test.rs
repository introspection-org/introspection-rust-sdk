//! `client.repositories()` against a mock Control Plane.
//!
//! `GET /v1/repositories` is the one CP list that answers a bare array rather
//! than the cursor envelope, and a row's `provider` may name a value this SDK
//! has never seen; both are exercised here so a Control Plane ahead of the SDK
//! still reads.

use introspection_sdk::{
    AdvancedOptions, ClientConfig, IntrospectionClient, RepositoryProvider,
    RepositoryProvisioningStatus,
};
use serde_json::json;
use uuid::Uuid;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const PROJECT_ID: &str = "00000000-0000-0000-0000-0000000000bb";
const GITHUB_ID: &str = "11111111-1111-7111-8111-111111111111";
const HOSTED_ID: &str = "22222222-2222-7222-8222-222222222222";

fn client(server: &MockServer) -> IntrospectionClient {
    IntrospectionClient::new(
        ClientConfig::with_token("intro_test").advanced(AdvancedOptions {
            base_api_url: Some(server.uri()),
            ..Default::default()
        }),
    )
    .unwrap()
}

#[tokio::test]
async fn list_reads_the_bare_array_and_tolerates_a_new_provider() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/repositories"))
        .and(query_param("project", "acme"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {
                "id": GITHUB_ID,
                "project_id": PROJECT_ID,
                "integration_id": "33333333-3333-7333-8333-333333333333",
                "url": "https://github.com/example/recipes",
                "name": "example/recipes",
                "slug": "example-recipes",
                "provider": "github",
                "default_branch": "main",
                "provisioning_status": "ready",
                "created_at": "2026-09-01T00:00:00Z",
                "is_recipe_source": true
            },
            {
                "id": HOSTED_ID,
                "project_id": PROJECT_ID,
                "integration_id": null,
                "url": "https://git.example.test/v1/git/support.git",
                "name": "support",
                "provider": "gitlab-someday",
                "provisioning_status": "provisioning-someday",
                "created_at": "2026-09-01T00:00:00Z"
            }
        ])))
        .expect(1)
        .mount(&server)
        .await;

    let repositories = client(&server).repositories().list("acme").await.unwrap();

    assert_eq!(repositories.len(), 2);
    let github = &repositories[0];
    assert_eq!(github.id, Uuid::parse_str(GITHUB_ID).unwrap());
    assert_eq!(github.provider, RepositoryProvider::Github);
    assert_eq!(
        github.provisioning_status,
        RepositoryProvisioningStatus::Ready
    );
    assert_eq!(
        github.url.as_deref(),
        Some("https://github.com/example/recipes")
    );
    assert!(github.is_recipe_source);

    let unknown = &repositories[1];
    assert_eq!(
        unknown.provider,
        RepositoryProvider::Other("gitlab-someday".into())
    );
    assert_eq!(
        unknown.provisioning_status,
        RepositoryProvisioningStatus::Other("provisioning-someday".into())
    );
    assert_eq!(unknown.default_branch, "main");
    assert!(unknown.integration_id.is_none());
    assert!(!unknown.is_recipe_source);
}

#[tokio::test]
async fn get_scopes_the_read_to_the_project() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/v1/repositories/{HOSTED_ID}")))
        .and(query_param("project", PROJECT_ID))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": HOSTED_ID,
            "project_id": PROJECT_ID,
            "url": "https://git.example.test/v1/git/support.git",
            "name": "support",
            "slug": "support",
            "provider": "hosted",
            "default_branch": "trunk",
            "provisioning_status": "pending",
            "seed_template": "pi-agent",
            "created_at": "2026-09-01T00:00:00Z"
        })))
        .expect(1)
        .mount(&server)
        .await;

    let repository = client(&server)
        .repositories()
        .get(
            Uuid::parse_str(HOSTED_ID).unwrap(),
            Uuid::parse_str(PROJECT_ID).unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(repository.provider, RepositoryProvider::Hosted);
    assert_eq!(repository.default_branch, "trunk");
    assert_eq!(repository.seed_template.as_deref(), Some("pi-agent"));
    assert_eq!(
        repository.provisioning_status,
        RepositoryProvisioningStatus::Pending
    );
}
