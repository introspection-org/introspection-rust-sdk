//! The `authoring` feature's writes: runtimes, recipes, endpoint bindings and
//! repositories, backed by `wiremock`.
//!
//! Each test pins the method, path, `?project=` and the exact JSON body, and
//! that the request types read back from a document the way the CLI's
//! `@FILE` arguments read them.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::time::Duration;

use introspection_sdk::api::{HttpClient, HttpConfig};
use introspection_sdk::{
    EndpointCreateParams, EndpointKind, EndpointMetadata, EndpointUpdateParams, Endpoints,
    IntrospectionAPIError, Lane, RecipeCreateParams, RecipeRepositorySource, RecipeUpdateParams,
    Recipes, Repositories, RepositoryCreateParams, RuntimeCreateParams, Runtimes, Secret,
    StringOrUuid,
};
use serde_json::{json, Value};
use uuid::Uuid;
use wiremock::matchers::{body_json, method, path, query_param, query_param_is_missing};
use wiremock::{Mock, MockServer, ResponseTemplate};

const ID: &str = "11111111-1111-1111-1111-111111111111";
const PROJECT_ID: &str = "00000000-0000-0000-0000-0000000000bb";
const RECIPE_ID: &str = "22222222-2222-2222-2222-222222222222";
const INTEGRATION_ID: &str = "33333333-3333-3333-3333-333333333333";
const SHA: &str = "543b8db50a83543b8db50a83543b8db50a83543b";

fn build_http(server: &MockServer) -> Arc<HttpClient> {
    let cfg = HttpConfig {
        api_url: server.uri(),
        token: "intro_test".to_string(),
        additional_headers: HashMap::new(),
        timeout: Duration::from_secs(5),
        max_retries: 0,
        retry_base: Duration::from_millis(1),
    };
    Arc::new(HttpClient::from_parts(reqwest::Client::new(), cfg))
}

fn id() -> Uuid {
    ID.parse().unwrap()
}

fn runtime_json() -> Value {
    json!({
        "id": ID,
        "org_id": "00000000-0000-0000-0000-0000000000aa",
        "project_id": PROJECT_ID,
        "recipe_id": RECIPE_ID,
        "created_by_member_id": "00000000-0000-0000-0000-0000000000cc",
        "created_at": "2026-10-01T00:00:00Z",
        "updated_at": "2026-10-01T00:00:00Z",
        "name": "Support",
        "slug": "support",
    })
}

fn recipe_json() -> Value {
    json!({
        "id": ID,
        "org_id": "00000000-0000-0000-0000-0000000000aa",
        "project_id": PROJECT_ID,
        "repository_id": "44444444-4444-4444-4444-444444444444",
        "name": "support",
        "slug": "support-main-543b8db50a83",
        "git_ref": "main",
        "git_commit_sha": SHA,
        "description": "Answers tickets",
        "created_by_member_id": "00000000-0000-0000-0000-0000000000cc",
        "created_at": "2026-10-01T00:00:00Z",
        "updated_at": "2026-10-01T00:00:00Z",
    })
}

fn endpoint_json() -> Value {
    json!({
        "id": ID,
        "org_id": "00000000-0000-0000-0000-0000000000aa",
        "project_id": PROJECT_ID,
        "name": "CRM",
        "host": "api.crm.example",
        "kind": "api",
        "metadata": {"extra_hosts": []},
        "headers": {"X-Api-Key": "${CRM_KEY}"},
        "header_names": ["X-Api-Key"],
        "credential_refs": ["CRM_KEY"],
        "created_by_member_id": "00000000-0000-0000-0000-0000000000cc",
        "created_at": "2026-10-01T00:00:00Z",
        "updated_at": "2026-10-01T00:00:00Z",
    })
}

fn repository_json(provider: &str) -> Value {
    json!({
        "id": ID,
        "project_id": PROJECT_ID,
        "integration_id": null,
        "url": "https://git.example.test/v1/git/support.git",
        "name": "support",
        "slug": "support",
        "provider": provider,
        "default_branch": "main",
        "provisioning_status": "pending",
        "created_at": "2026-10-01T00:00:00Z",
    })
}

fn no_content() -> ResponseTemplate {
    ResponseTemplate::new(204)
}

// ----- runtimes --------------------------------------------------------------

#[tokio::test]
async fn runtime_create_posts_the_body_with_its_project() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/runtimes"))
        .and(query_param_is_missing("project"))
        .and(body_json(json!({
            "project": "support",
            "recipe_id": RECIPE_ID,
            "name": "Support",
            "slug": "support",
            "config_json": {"resources": {"cpu": 2}},
        })))
        .respond_with(ResponseTemplate::new(201).set_body_json(runtime_json()))
        .expect(1)
        .mount(&server)
        .await;

    let params = RuntimeCreateParams {
        slug: Some("support".into()),
        config_json: Some(BTreeMap::from([(
            "resources".to_string(),
            json!({"cpu": 2}),
        )])),
        ..RuntimeCreateParams::new("support", RECIPE_ID.parse().unwrap(), "Support")
    };
    let runtime = Runtimes::new(build_http(&server))
        .create(&params)
        .await
        .unwrap();
    assert_eq!(runtime.slug, "support");
}

#[tokio::test]
async fn runtime_delete_sends_the_project_when_named() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path(format!("/v1/runtimes/{ID}")))
        .and(query_param("project", "support"))
        .respond_with(no_content())
        .expect(1)
        .mount(&server)
        .await;

    Runtimes::new(build_http(&server))
        .delete(id(), Some("support".into()))
        .await
        .unwrap();
}

#[tokio::test]
async fn runtime_delete_without_a_project_sends_none() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path(format!("/v1/runtimes/{ID}")))
        .and(query_param_is_missing("project"))
        .respond_with(no_content())
        .expect(1)
        .mount(&server)
        .await;

    Runtimes::new(build_http(&server))
        .delete(id(), None)
        .await
        .unwrap();
}

// ----- recipes ---------------------------------------------------------------

#[tokio::test]
async fn recipe_create_pins_a_commit() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/recipes"))
        .and(body_json(json!({
            "project": PROJECT_ID,
            "name": "support",
            "repository_id": "44444444-4444-4444-4444-444444444444",
            "git_ref": "main",
            "git_commit_sha": SHA,
        })))
        .respond_with(ResponseTemplate::new(201).set_body_json(recipe_json()))
        .expect(1)
        .mount(&server)
        .await;

    let params = RecipeCreateParams {
        repository_id: Some("44444444-4444-4444-4444-444444444444".parse().unwrap()),
        git_ref: Some("main".into()),
        git_commit_sha: Some(SHA.into()),
        ..RecipeCreateParams::new(PROJECT_ID.parse::<Uuid>().unwrap(), "support")
    };
    let recipe = Recipes::new(build_http(&server))
        .create(&params)
        .await
        .unwrap();
    assert_eq!(recipe.git_commit_sha, SHA);
}

#[test]
fn recipe_create_names_a_github_repository_to_link() {
    let params = RecipeCreateParams {
        repository: Some(RecipeRepositorySource {
            integration_id: INTEGRATION_ID.parse().unwrap(),
            full_name: "acme/support".into(),
        }),
        ..RecipeCreateParams::new("support", "support")
    };
    assert_eq!(
        serde_json::to_value(&params).unwrap(),
        json!({
            "project": "support",
            "name": "support",
            "repository": {"integration_id": INTEGRATION_ID, "full_name": "acme/support"},
        })
    );
}

#[tokio::test]
async fn recipe_update_patches_only_the_set_fields() {
    let server = MockServer::start().await;
    Mock::given(method("PATCH"))
        .and(path(format!("/v1/recipes/{ID}")))
        .and(query_param("project", "support"))
        .and(body_json(json!({"description": "Answers tickets"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(recipe_json()))
        .expect(1)
        .mount(&server)
        .await;

    let params = RecipeUpdateParams {
        description: Some("Answers tickets".into()),
        ..Default::default()
    };
    let recipe = Recipes::new(build_http(&server))
        .update(id(), &params, Some("support".into()))
        .await
        .unwrap();
    assert_eq!(recipe.description.as_deref(), Some("Answers tickets"));
}

#[test]
fn recipe_retry_validation_resets_it_to_pending() {
    assert_eq!(
        serde_json::to_value(RecipeUpdateParams::retry_validation()).unwrap(),
        json!({"validation": {"status": "pending"}})
    );
}

#[tokio::test]
async fn recipe_delete_sends_the_project() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path(format!("/v1/recipes/{ID}")))
        .and(query_param("project", PROJECT_ID))
        .respond_with(no_content())
        .expect(1)
        .mount(&server)
        .await;

    Recipes::new(build_http(&server))
        .delete(id(), Some(PROJECT_ID.parse::<Uuid>().unwrap().into()))
        .await
        .unwrap();
}

// ----- endpoints -------------------------------------------------------------

#[tokio::test]
async fn endpoint_create_posts_its_headers_in_plaintext() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/endpoints"))
        .and(query_param("project", "support"))
        .and(body_json(json!({
            "name": "CRM",
            "host": "api.crm.example",
            "kind": "api",
            "headers": {"X-Api-Key": "${CRM_KEY}"},
            "environment": "production",
        })))
        .respond_with(ResponseTemplate::new(201).set_body_json(endpoint_json()))
        .expect(1)
        .mount(&server)
        .await;

    let params = EndpointCreateParams {
        kind: Some(EndpointKind::Api),
        headers: BTreeMap::from([("X-Api-Key".to_string(), Secret::new("${CRM_KEY}"))]),
        environment: Some(Lane::Production),
        ..EndpointCreateParams::new("CRM", "api.crm.example")
    };
    let endpoint = Endpoints::new(build_http(&server))
        .create(&params, Some("support".into()))
        .await
        .unwrap();
    assert_eq!(endpoint.header_names, vec!["X-Api-Key"]);
}

#[test]
fn endpoint_create_always_sends_headers() {
    // The route requires the field; an empty map is a public host.
    assert_eq!(
        serde_json::to_value(EndpointCreateParams::new("CRM", "api.crm.example")).unwrap(),
        json!({"name": "CRM", "host": "api.crm.example", "headers": {}})
    );
}

#[tokio::test]
async fn endpoint_create_redacts_header_values_from_an_error() {
    let server = MockServer::start().await;
    // A validation error echoes the body it refused.
    Mock::given(method("POST"))
        .and(path("/v1/endpoints"))
        .respond_with(ResponseTemplate::new(422).set_body_json(json!({
            "detail": [{
                "type": "value_error",
                "loc": ["body"],
                "msg": "Authorization: Bearer sk-live-123 cannot be combined with assertion_claims",
                "input": {"headers": {"Authorization": "Bearer sk-live-123"}},
            }],
        })))
        .mount(&server)
        .await;

    let params = EndpointCreateParams {
        headers: BTreeMap::from([(
            "Authorization".to_string(),
            Secret::new("Bearer sk-live-123"),
        )]),
        assertion_claims: Some(BTreeMap::from([("role".into(), "user".into())])),
        ..EndpointCreateParams::new("CRM", "api.crm.example")
    };
    let err = Endpoints::new(build_http(&server))
        .create(&params, None)
        .await
        .unwrap_err();
    let IntrospectionAPIError::Http { status, body, .. } = &err else {
        panic!("expected an HTTP error, got {err:?}");
    };
    assert_eq!(*status, 422);
    let body = body.as_ref().expect("the error body").to_string();
    assert!(!body.contains("sk-live-123"), "{body}");
    assert!(body.contains("[redacted]"), "{body}");
    assert!(!err.to_string().contains("sk-live-123"), "{err}");
}

#[tokio::test]
async fn endpoint_update_can_clear_the_environment() {
    let server = MockServer::start().await;
    Mock::given(method("PATCH"))
        .and(path(format!("/v1/endpoints/{ID}")))
        .and(query_param_is_missing("project"))
        .and(body_json(json!({
            "name": "CRM (eu)",
            "environment": null,
            "metadata": {"extra_hosts": [], "connector_id": RECIPE_ID},
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(endpoint_json()))
        .expect(1)
        .mount(&server)
        .await;

    let params = EndpointUpdateParams {
        name: Some("CRM (eu)".into()),
        environment: Some(None),
        metadata: Some(EndpointMetadata {
            connector_id: Some(RECIPE_ID.parse().unwrap()),
            ..Default::default()
        }),
        ..Default::default()
    };
    Endpoints::new(build_http(&server))
        .update(id(), &params, None)
        .await
        .unwrap();
}

#[test]
fn endpoint_update_leaves_out_what_it_does_not_change() {
    assert_eq!(
        serde_json::to_value(EndpointUpdateParams::default()).unwrap(),
        json!({})
    );
}

#[tokio::test]
async fn endpoint_delete_sends_the_project() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path(format!("/v1/endpoints/{ID}")))
        .and(query_param("project", "support"))
        .respond_with(no_content())
        .expect(1)
        .mount(&server)
        .await;

    Endpoints::new(build_http(&server))
        .delete(id(), Some("support".into()))
        .await
        .unwrap();
}

// ----- repositories ----------------------------------------------------------

fn repositories(server: &MockServer) -> Repositories {
    let http = build_http(server);
    Repositories::new(http.clone(), http)
}

#[tokio::test]
async fn repository_create_hosted_takes_the_project_as_a_query() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/repositories"))
        .and(query_param("project", "support"))
        .and(body_json(
            json!({"provider": "introspection", "slug": "support"}),
        ))
        .respond_with(ResponseTemplate::new(201).set_body_json(repository_json("introspection")))
        .expect(1)
        .mount(&server)
        .await;

    let created = repositories(&server)
        .create("support", &RepositoryCreateParams::hosted("support"))
        .await
        .unwrap();
    assert_eq!(created.slug.as_deref(), Some("support"));
}

#[test]
fn repository_create_hosted_without_a_seed_sends_null() {
    let mut params = RepositoryCreateParams::hosted("support");
    let RepositoryCreateParams::Hosted(hosted) = &mut params else {
        unreachable!()
    };
    hosted.seed_template = Some(None);
    hosted.default_branch = Some("trunk".into());
    assert_eq!(
        serde_json::to_value(&params).unwrap(),
        json!({
            "provider": "introspection",
            "slug": "support",
            "default_branch": "trunk",
            "seed_template": null,
        })
    );
}

#[tokio::test]
async fn repository_create_github_links_the_url() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/repositories"))
        .and(query_param("project", PROJECT_ID))
        .and(body_json(json!({
            "provider": "github",
            "integration_id": INTEGRATION_ID,
            "url": "https://github.com/acme/support",
            "name": "acme/support",
        })))
        .respond_with(ResponseTemplate::new(201).set_body_json(repository_json("github")))
        .expect(1)
        .mount(&server)
        .await;

    let params = RepositoryCreateParams::github(INTEGRATION_ID.parse().unwrap(), "acme/support");
    repositories(&server)
        .create(PROJECT_ID.parse::<Uuid>().unwrap(), &params)
        .await
        .unwrap();
}

#[tokio::test]
async fn repository_delete_takes_the_project_as_a_query() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path(format!("/v1/repositories/{ID}")))
        .and(query_param("project", "support"))
        .respond_with(no_content())
        .expect(1)
        .mount(&server)
        .await;

    repositories(&server).delete(id(), "support").await.unwrap();
}

#[tokio::test]
async fn repository_delete_surfaces_repository_in_use() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path(format!("/v1/repositories/{ID}")))
        .respond_with(ResponseTemplate::new(409).set_body_json(json!({
            "detail": {"code": "repository_in_use", "message": "Runtimes still deploy from it."},
        })))
        .mount(&server)
        .await;

    let err = repositories(&server)
        .delete(id(), "support")
        .await
        .unwrap_err();
    assert_eq!(err.status(), Some(409));
}

// ----- documents -------------------------------------------------------------

#[test]
fn request_types_read_back_from_a_document() {
    let runtime: RuntimeCreateParams = serde_json::from_value(json!({
        "project": PROJECT_ID,
        "recipe_id": RECIPE_ID,
        "name": "Support",
    }))
    .unwrap();
    assert!(matches!(runtime.project, StringOrUuid::Uuid(_)));

    let recipe: RecipeCreateParams =
        serde_json::from_value(json!({"project": "support", "name": "support"})).unwrap();
    assert!(matches!(recipe.project, StringOrUuid::String(ref s) if s == "support"));

    let update: EndpointUpdateParams =
        serde_json::from_value(json!({"environment": null})).unwrap();
    assert_eq!(update.environment, Some(None));
    let update: EndpointUpdateParams = serde_json::from_value(json!({})).unwrap();
    assert_eq!(update.environment, None);

    let endpoint: EndpointCreateParams = serde_json::from_value(json!({
        "name": "CRM",
        "host": "api.crm.example",
        "headers": {"X-Api-Key": "literal"},
    }))
    .unwrap();
    assert_eq!(endpoint.headers["X-Api-Key"].expose(), "literal");

    // `hosted` is the name the hosted provider shipped under.
    for provider in ["introspection", "hosted"] {
        let repo: RepositoryCreateParams = serde_json::from_value(json!({
            "provider": provider,
            "slug": "support",
            "seed_template": null,
        }))
        .unwrap();
        let RepositoryCreateParams::Hosted(hosted) = repo else {
            panic!("{provider} reads as hosted");
        };
        assert_eq!(hosted.seed_template, Some(None));
    }
    let repo: RepositoryCreateParams = serde_json::from_value(json!({
        "provider": "github",
        "integration_id": INTEGRATION_ID,
        "url": "https://github.com/acme/support",
        "name": "acme/support",
    }))
    .unwrap();
    assert!(matches!(repo, RepositoryCreateParams::Github(_)));
}

#[test]
fn a_header_secret_does_not_debug_print() {
    let params = EndpointCreateParams {
        headers: BTreeMap::from([("X-Api-Key".to_string(), Secret::new("sk-live-123"))]),
        ..EndpointCreateParams::new("CRM", "api.crm.example")
    };
    assert!(!format!("{params:?}").contains("sk-live-123"));
}
