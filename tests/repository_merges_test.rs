//! `client.repositories().merge(id, …)` against a mock Data Plane.
//!
//! The Control Plane is a separate mock with nothing mounted, so a merge that
//! went to it instead would 404.

use introspection_sdk::{
    AdvancedOptions, ClientConfig, IntrospectionAPIError, IntrospectionClient,
    RepositoryMergeCreate,
};
use serde_json::json;
use uuid::Uuid;
use wiremock::matchers::{body_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const REPOSITORY_ID: &str = "22222222-2222-7222-8222-222222222222";

fn repository_id() -> Uuid {
    Uuid::parse_str(REPOSITORY_ID).unwrap()
}

fn merges_path() -> String {
    format!("/v1/repositories/{REPOSITORY_ID}/merges")
}

async fn client(dp: &MockServer) -> (IntrospectionClient, MockServer) {
    let cp = MockServer::start().await;
    let client = IntrospectionClient::new(ClientConfig::with_token("intro_test").advanced(
        AdvancedOptions {
            base_api_url: Some(cp.uri()),
            dp_url: Some(dp.uri()),
            ..Default::default()
        },
    ))
    .unwrap();
    (client, cp)
}

fn feature_into_main() -> RepositoryMergeCreate {
    RepositoryMergeCreate {
        base: "main".into(),
        head: "feature/x".into(),
        commit_message: None,
    }
}

#[tokio::test]
async fn merge_returns_the_merge_commit() {
    let dp = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(merges_path()))
        .and(body_json(json!({
            "base": "main", "head": "feature/x", "commit_message": "Ship it"
        })))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({
            "sha": "m1", "base": "main", "head": "feature/x",
            "head_sha": "h1", "parents": ["b1", "h1"]
        })))
        .expect(1)
        .mount(&dp)
        .await;
    let (client, _cp) = client(&dp).await;

    let commit = client
        .repositories()
        .merge(
            repository_id(),
            &RepositoryMergeCreate {
                commit_message: Some("Ship it".into()),
                ..feature_into_main()
            },
        )
        .await
        .unwrap()
        .expect("a 201 carries the merge commit");

    assert_eq!(commit.sha, "m1");
    assert_eq!(commit.head_sha, "h1");
    assert_eq!(commit.parents, ["b1", "h1"]);
}

#[tokio::test]
async fn merge_omits_an_unset_commit_message() {
    let dp = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(merges_path()))
        .and(body_json(json!({"base": "main", "head": "feature/x"})))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&dp)
        .await;
    let (client, _cp) = client(&dp).await;

    let merged = client
        .repositories()
        .merge(repository_id(), &feature_into_main())
        .await
        .unwrap();

    assert!(merged.is_none(), "204 means base already contains head");
}

#[tokio::test]
async fn merge_surfaces_a_conflict_as_an_http_error() {
    let dp = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(merges_path()))
        .respond_with(ResponseTemplate::new(409).set_body_json(json!({"detail": "Merge conflict"})))
        .expect(1)
        .mount(&dp)
        .await;
    let (client, _cp) = client(&dp).await;

    let err = client
        .repositories()
        .merge(repository_id(), &feature_into_main())
        .await
        .unwrap_err();

    assert!(matches!(err, IntrospectionAPIError::Http { .. }), "{err:?}");
    assert_eq!(err.status(), Some(409));
}
