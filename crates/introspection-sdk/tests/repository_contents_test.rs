//! `client.repositories().contents(id)` against a mock Data Plane.
//!
//! The Control Plane is a separate mock with nothing mounted, so a contents
//! read that went to it instead would 404.

use futures::StreamExt;
use introspection_sdk::{
    AdvancedOptions, ClientConfig, ContentsQuery, IntrospectionAPIError, IntrospectionClient,
    RepositoryContent, RepositoryEntryType,
};
use serde_json::json;
use uuid::Uuid;
use wiremock::matchers::{method, path, query_param, query_param_is_missing};
use wiremock::{Mock, MockServer, ResponseTemplate};

const REPOSITORY_ID: &str = "22222222-2222-7222-8222-222222222222";

fn repository_id() -> Uuid {
    Uuid::parse_str(REPOSITORY_ID).unwrap()
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

fn entry(name: &str, kind: &str) -> serde_json::Value {
    json!({"name": name, "path": format!("agents/{name}"), "type": kind, "size": 3, "sha": "e1"})
}

#[tokio::test]
async fn list_reads_the_root_directory() {
    let dp = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/v1/repositories/{REPOSITORY_ID}/contents")))
        .and(query_param("ref", "main"))
        .and(query_param("limit", "50"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "type": "dir",
            "path": "",
            "commit_sha": "c0ffee",
            "records": [entry("agent.yaml", "file"), entry("tools", "dir"), entry("x", "gitlink")],
            "count": 3,
            "next": null
        })))
        .expect(1)
        .mount(&dp)
        .await;
    let (client, _cp) = client(&dp).await;

    let entries: Vec<_> = client
        .repositories()
        .contents(repository_id())
        .list(&ContentsQuery {
            r#ref: Some("main".into()),
            limit: Some(50),
            ..Default::default()
        })
        .unwrap()
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .collect::<Result<_, _>>()
        .unwrap();

    assert_eq!(entries.len(), 3);
    assert_eq!(entries[0].entry_type, RepositoryEntryType::File);
    assert_eq!(entries[1].entry_type, RepositoryEntryType::Dir);
    assert_eq!(
        entries[2].entry_type,
        RepositoryEntryType::Other("gitlink".into())
    );
}

#[tokio::test]
async fn list_follows_the_cursor_to_the_second_page() {
    let dp = MockServer::start().await;
    let dir = format!("/v1/repositories/{REPOSITORY_ID}/contents/agents");
    Mock::given(method("GET"))
        .and(path(dir.clone()))
        .and(query_param_is_missing("cursor"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "type": "dir", "path": "agents", "commit_sha": "c0ffee",
            "records": [entry("a.yaml", "file")], "count": 1, "next": "page-2"
        })))
        .expect(1)
        .mount(&dp)
        .await;
    Mock::given(method("GET"))
        .and(path(dir))
        .and(query_param("cursor", "page-2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "type": "dir", "path": "agents", "commit_sha": "c0ffee",
            "records": [entry("b.yaml", "file")], "count": 1, "next": null
        })))
        .expect(1)
        .mount(&dp)
        .await;
    let (client, _cp) = client(&dp).await;

    let mut paginator = client
        .repositories()
        .contents(repository_id())
        .list(&ContentsQuery {
            path: "agents".into(),
            ..Default::default()
        })
        .unwrap();
    let mut names = Vec::new();
    while let Some(entry) = paginator.next().await {
        names.push(entry.unwrap().name);
    }

    assert_eq!(names, ["a.yaml", "b.yaml"]);
}

#[tokio::test]
async fn get_reads_a_file() {
    let dp = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!(
            "/v1/repositories/{REPOSITORY_ID}/contents/agents/agent.yaml"
        )))
        .and(query_param("ref", "v1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "type": "file", "name": "agent.yaml", "path": "agents/agent.yaml",
            "size": 5, "sha": "f1", "commit_sha": "c0ffee",
            "encoding": "utf-8", "content": "name:", "truncated": false
        })))
        .expect(1)
        .mount(&dp)
        .await;
    let (client, _cp) = client(&dp).await;

    let content = client
        .repositories()
        .contents(repository_id())
        .get(
            "/agents/agent.yaml",
            &ContentsQuery {
                r#ref: Some("v1".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();

    let RepositoryContent::File(file) = content else {
        panic!("expected a file, got {content:?}");
    };
    assert_eq!(file.content, "name:");
    assert_eq!(file.encoding, "utf-8");
    assert_eq!(file.commit_sha, "c0ffee");
    assert!(!file.truncated);
}

#[tokio::test]
async fn path_is_encoded_per_segment() {
    let dp = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "type": "dir", "path": "a b/c#d", "commit_sha": "c0ffee",
            "records": [], "count": 0, "next": null
        })))
        .expect(1)
        .mount(&dp)
        .await;
    let (client, _cp) = client(&dp).await;
    let contents = client.repositories().contents(repository_id());

    let content = contents
        .get("a b/c#d?", &ContentsQuery::default())
        .await
        .unwrap();
    assert!(matches!(content, RepositoryContent::Dir(_)));

    let request = &dp.received_requests().await.unwrap()[0];
    assert_eq!(
        request.url.path(),
        format!("/v1/repositories/{REPOSITORY_ID}/contents/a%20b/c%23d%3F")
    );
    assert_eq!(request.url.query(), None);

    // A dot segment would be resolved away by the URL parser, so it is refused
    // before any request is made.
    let err = contents
        .get("agents/../secrets", &ContentsQuery::default())
        .await
        .unwrap_err();
    assert!(matches!(err, IntrospectionAPIError::InvalidConfig(_)));
}

#[tokio::test]
async fn listing_a_file_path_is_an_error() {
    let dp = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!(
            "/v1/repositories/{REPOSITORY_ID}/contents/README.md"
        )))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "type": "file", "name": "README.md", "path": "README.md",
            "size": 1, "sha": "f1", "commit_sha": "c0ffee",
            "encoding": "utf-8", "content": "#"
        })))
        .expect(1)
        .mount(&dp)
        .await;
    let (client, _cp) = client(&dp).await;

    let mut paginator = client
        .repositories()
        .contents(repository_id())
        .list(&ContentsQuery {
            path: "README.md".into(),
            ..Default::default()
        })
        .unwrap();

    let err = paginator.next().await.unwrap().unwrap_err();
    assert!(err.to_string().contains("is a file"), "{err}");
    assert!(paginator.next().await.is_none());
}
