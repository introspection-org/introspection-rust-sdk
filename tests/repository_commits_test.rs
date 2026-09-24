//! `client.repositories().commits(id, …)` / `commit(id, sha)` against a mock
//! Data Plane.
//!
//! The Control Plane is a separate mock with nothing mounted, so a commits
//! read that went to it instead would 404.

use futures::StreamExt;
use introspection_sdk::{
    AdvancedOptions, ClientConfig, CommitsQuery, IntrospectionAPIError, IntrospectionClient,
    RepositoryCommitFileStatus,
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

fn commit(sha: &str) -> serde_json::Value {
    json!({
        "sha": sha,
        "parents": ["p1"],
        "message": format!("commit {sha}"),
        "author": {"name": "Ada", "email": "ada@example.com", "date": "2026-09-01T00:00:00Z"},
        "committer": {"name": "Bot", "email": null, "date": null}
    })
}

#[tokio::test]
async fn commits_sends_the_query_and_follows_the_cursor() {
    let dp = MockServer::start().await;
    let commits = format!("/v1/repositories/{REPOSITORY_ID}/commits");
    Mock::given(method("GET"))
        .and(path(commits.clone()))
        .and(query_param("sha", "main"))
        .and(query_param("path", "agents/agent.yaml"))
        .and(query_param("limit", "1"))
        .and(query_param_is_missing("cursor"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "records": [commit("c2")], "count": 1, "next": "page-2"
        })))
        .expect(1)
        .mount(&dp)
        .await;
    Mock::given(method("GET"))
        .and(path(commits))
        .and(query_param("sha", "main"))
        .and(query_param("cursor", "page-2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "records": [commit("c1")], "count": 1, "next": null
        })))
        .expect(1)
        .mount(&dp)
        .await;
    let (client, _cp) = client(&dp).await;

    let commits: Vec<_> = client
        .repositories()
        .commits(
            repository_id(),
            &CommitsQuery {
                sha: Some("main".into()),
                path: Some("agents/agent.yaml".into()),
                limit: Some(1),
            },
        )
        .unwrap()
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .collect::<Result<_, _>>()
        .unwrap();

    let shas: Vec<_> = commits.iter().map(|c| c.sha.as_str()).collect();
    assert_eq!(shas, ["c2", "c1"]);
    assert_eq!(commits[0].parents, ["p1"]);
    assert_eq!(commits[0].author.email.as_deref(), Some("ada@example.com"));
    assert_eq!(commits[0].committer.email, None);
}

#[tokio::test]
async fn commits_omits_unset_query_params() {
    let dp = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/v1/repositories/{REPOSITORY_ID}/commits")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "records": [], "count": 0, "next": null
        })))
        .expect(1)
        .mount(&dp)
        .await;
    let (client, _cp) = client(&dp).await;

    let mut paginator = client
        .repositories()
        .commits(repository_id(), &CommitsQuery::default())
        .unwrap();
    assert!(paginator.next().await.is_none());

    let request = &dp.received_requests().await.unwrap()[0];
    assert_eq!(request.url.query(), None);
}

#[tokio::test]
async fn commit_reads_one_commit_with_its_diff() {
    let dp = MockServer::start().await;
    let mut body = commit("c0ffee");
    body["files"] = json!([
        {"filename": "a.txt", "status": "added", "additions": 1, "deletions": 0, "changes": 1},
        {"filename": "b.txt", "status": "copied", "additions": 0, "deletions": 0, "changes": 0}
    ]);
    body["patch"] = json!("diff --git a/a.txt b/a.txt\n");
    Mock::given(method("GET"))
        .and(path(format!(
            "/v1/repositories/{REPOSITORY_ID}/commits/c0ffee"
        )))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .expect(1)
        .mount(&dp)
        .await;
    let (client, _cp) = client(&dp).await;

    let detail = client
        .repositories()
        .commit(repository_id(), "c0ffee")
        .await
        .unwrap();

    assert_eq!(detail.commit.sha, "c0ffee");
    assert_eq!(detail.commit.author.name, "Ada");
    assert_eq!(detail.files.len(), 2);
    assert_eq!(detail.files[0].status, RepositoryCommitFileStatus::Added);
    assert_eq!(detail.files[0].additions, 1);
    assert_eq!(
        detail.files[1].status,
        RepositoryCommitFileStatus::Other("copied".into())
    );
    assert!(detail.patch.starts_with("diff --git"));
}

#[tokio::test]
async fn commit_encodes_a_ref_and_refuses_dot_segments() {
    let dp = MockServer::start().await;
    let mut body = commit("c0ffee");
    body["files"] = json!([]);
    body["patch"] = json!("");
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .expect(1)
        .mount(&dp)
        .await;
    let (client, _cp) = client(&dp).await;
    let repositories = client.repositories();

    repositories
        .commit(repository_id(), "feature/x#1")
        .await
        .unwrap();
    let request = &dp.received_requests().await.unwrap()[0];
    assert_eq!(
        request.url.path(),
        format!("/v1/repositories/{REPOSITORY_ID}/commits/feature%2Fx%231")
    );

    for sha in ["", ".", ".."] {
        let err = repositories.commit(repository_id(), sha).await.unwrap_err();
        assert!(
            matches!(err, IntrospectionAPIError::InvalidConfig(_)),
            "{sha:?}"
        );
    }
}
