//! Live CRUD walks against a real deployment: a local stack seeded by
//! `introspection-cloud`'s `scripts/seed_sdk_project.py`, or the staging
//! `sdk-tests` project.
//!
//! The wiremock suites pin what the SDK *sends*; these pin that a deployed
//! API *answers* it, and they are what the backend coverage run drives. Each
//! test creates what it reads and deletes it again, so it holds no state
//! between runs.
//!
//! `#[ignore]`d, following this crate's convention for tests that reach the
//! network. Run them with:
//!
//! ```sh
//! INTROSPECTION_LIVE=1 cargo test --test live_api_test -- --ignored --test-threads=1
//! ```
//!
//! reading `INTROSPECTION_TOKEN`, `INTROSPECTION_BASE_API_URL` (Control Plane)
//! and `INTROSPECTION_DP_URL` (Data Plane), plus `INTROSPECTION_CP_SESSION` for
//! the member-authored routes an API key cannot reach. A test whose settings
//! are missing skips rather than fails.

use std::collections::HashMap;
use std::env;

use introspection_sdk::api::IntrospectionAPIError;
use introspection_sdk::{
    AdvancedOptions, AutomationCreateParams, AutomationListParams, AutomationTriggerType,
    AutomationUpdateParams, ClientConfig, ConnectorListParams, ConversationListParams,
    EventListParams, ExperimentListParams, FileCreateText, FileListParams, FileUpdate, FileUpload,
    FileVersionListParams, IntrospectionClient, IntrospectionEventName, MemberListParams,
    RecipeListParams, RuntimeListParams, ShareCreate, ShareListParams, ShareResourceType,
    ShareUpdate, TagCreate, TagKind, TagListParams, TagUpdate, UploadSource,
};
use serde_json::json;
use uuid::Uuid;

const PAGES: usize = 5;

/// Which credential a test drives the API with.
#[derive(Clone, Copy)]
enum Credential {
    /// The project's API key, the SDK's primary credential.
    ApiKey,
    /// The API key plus a member's Control Plane session, for member-authored
    /// Control Plane routes.
    Session,
    /// A member's Data Plane token, for the Data Plane writes no API key
    /// capability grants (automations, annotation tags).
    Member,
}

fn setting(name: &str) -> Option<String> {
    let value = env::var(name).ok().filter(|v| !v.is_empty());
    if value.is_none() {
        eprintln!("skipping: {name} is not set");
    }
    value
}

/// The run's client, or `None` (and a note on stderr) when it should skip.
fn live_client(credential: Credential) -> Option<IntrospectionClient> {
    if env::var("INTROSPECTION_LIVE").ok().as_deref() != Some("1") {
        eprintln!("skipping: INTROSPECTION_LIVE is not 1");
        return None;
    }
    let (token, cp_session) = match credential {
        Credential::ApiKey => (setting("INTROSPECTION_TOKEN")?, None),
        Credential::Session => (
            setting("INTROSPECTION_TOKEN")?,
            Some(setting("INTROSPECTION_CP_SESSION")?),
        ),
        Credential::Member => (setting("INTROSPECTION_MEMBER_TOKEN")?, None),
    };
    let config = ClientConfig::builder()
        .token(token)
        .advanced(AdvancedOptions {
            base_api_url: env::var("INTROSPECTION_BASE_API_URL").ok(),
            dp_url: env::var("INTROSPECTION_DP_URL").ok(),
            cp_session,
            ..Default::default()
        })
        .build()
        .expect("client config");
    Some(IntrospectionClient::new(config).expect("client"))
}

/// A name no other run shares, so concurrent runs against one project never
/// collide and a leftover from a crashed run is recognisable.
fn unique(prefix: &str) -> String {
    format!("sdk-{prefix}-{}", Uuid::now_v7().simple())
}

fn assert_status<T: std::fmt::Debug>(result: Result<T, IntrospectionAPIError>, expected: u16) {
    match result {
        Err(err) => assert_eq!(err.status(), Some(expected), "unexpected error: {err}"),
        Ok(value) => panic!("expected HTTP {expected}, got Ok({value:?})"),
    }
}

#[tokio::test]
#[ignore = "reaches a live deployment; see the module docs"]
async fn files_crud_walk() {
    let Some(client) = live_client(Credential::ApiKey) else {
        return;
    };
    let files = client.files();
    let name = unique("file") + ".txt";

    let created = files
        .create_text(&FileCreateText {
            name: name.clone(),
            content: "hello from the rust sdk".into(),
            mime_type: Some("text/plain".into()),
            metadata: Some(HashMap::from([("suite".into(), json!("live"))])),
            ..Default::default()
        })
        .await
        .expect("create_text");
    let file_id = created.id.to_string();

    let fetched = files.get(&file_id).await.expect("get");
    assert_eq!(fetched.name, name);

    let listed = files
        .list(&FileListParams {
            name: Some(name.clone()),
            include_total: Some(true),
            ..Default::default()
        })
        .collect_all(PAGES)
        .await
        .expect("list");
    assert!(listed.iter().any(|file| file.id == created.id));

    let renamed = unique("renamed") + ".txt";
    let updated = files
        .update(
            &file_id,
            &FileUpdate {
                name: Some(renamed.clone()),
                metadata: Some(HashMap::from([("suite".into(), json!("live-updated"))])),
                ..Default::default()
            },
        )
        .await
        .expect("update");
    assert_eq!(updated.name, renamed);

    let body = files.download(&file_id).await.expect("download");
    assert_eq!(&body[..], b"hello from the rust sdk");

    let version = files
        .versions
        .create(
            &file_id,
            FileUpload {
                source: UploadSource::Bytes(b"second version".to_vec()),
                name: Some(renamed.clone()),
                file_type: None,
                content_type: Some("text/plain".into()),
                metadata: None,
                tags: None,
            },
        )
        .await
        .expect("versions.create");
    let versions = files
        .versions
        .list(&file_id, &FileVersionListParams::default())
        .collect_all(PAGES)
        .await
        .expect("versions.list");
    assert!(!versions.is_empty());
    assert!(versions.iter().any(|v| v.id == version.id));
    files
        .versions
        .get(&file_id, &version.id.to_string())
        .await
        .expect("versions.get");

    files.delete(&file_id).await.expect("delete");
    assert_status(files.get(&file_id).await, 404);
}

#[tokio::test]
#[ignore = "reaches a live deployment; see the module docs"]
async fn upload_and_shares_crud_walk() {
    let Some(client) = live_client(Credential::ApiKey) else {
        return;
    };
    let files = client.files();
    let file = files
        .upload(FileUpload {
            source: UploadSource::Bytes(b"shared bytes".to_vec()),
            name: Some(unique("shared") + ".bin"),
            file_type: None,
            content_type: Some("application/octet-stream".into()),
            metadata: None,
            tags: None,
        })
        .await
        .expect("upload");
    let file_id = file.id.to_string();
    let shares = client.shares();
    let tag = unique("audience");

    let share = shares
        .create(&ShareCreate {
            resource_type: ShareResourceType::File,
            resource_id: file_id.clone(),
            granted_member_id: None,
            granted_tag: Some(tag.clone()),
            visible_from: None,
        })
        .await
        .expect("shares.create");
    let share_id = share.id.to_string();
    shares.get(&share_id).await.expect("shares.get");
    let listed = shares
        .list(&ShareListParams {
            resource_type: Some(ShareResourceType::File),
            resource_id: Some(file_id.clone()),
            ..Default::default()
        })
        .collect_all(PAGES)
        .await
        .expect("shares.list");
    assert!(listed.iter().any(|s| s.id == share.id));
    // `visible_from` windows a conversation's history; a file share refuses it.
    assert_status(
        shares
            .update(
                &share_id,
                &ShareUpdate {
                    visible_from: Some("2026-01-01T00:00:00Z".into()),
                },
            )
            .await,
        422,
    );
    // No API key capability carries `shares:delete`, so a member removes it.
    let Some(member) = live_client(Credential::Member) else {
        return;
    };
    member
        .shares()
        .delete(&share_id)
        .await
        .expect("shares.delete");
    assert_status(shares.get(&share_id).await, 404);

    files.delete(&file_id).await.expect("delete");
}

#[tokio::test]
#[ignore = "reaches a live deployment; see the module docs"]
async fn tags_crud_walk() {
    let Some(client) = live_client(Credential::Member) else {
        return;
    };
    let tags = client.tags();
    let access = unique("access");
    let annotation = unique("label");

    tags.create(TagCreate::access(access.clone()))
        .await
        .expect("create access tag");
    let created = tags
        .create(TagCreate::annotation(annotation.clone(), "#f97316"))
        .await
        .expect("create annotation tag");

    tags.get(TagKind::Access, &access)
        .await
        .expect("get access");
    tags.get(TagKind::Annotation, &created.name)
        .await
        .expect("get annotation");
    let listed = tags
        .list(&TagListParams::new(TagKind::Annotation))
        .collect_all(PAGES)
        .await
        .expect("list");
    assert!(listed.iter().any(|tag| tag.name == created.name));

    tags.update(
        TagKind::Annotation,
        &created.name,
        TagUpdate {
            color: Some("#22c55e".into()),
            description: Some(Some("recolored".into())),
        },
    )
    .await
    .expect("update");

    tags.delete(TagKind::Annotation, &created.name)
        .await
        .expect("delete annotation");
    tags.delete(TagKind::Access, &access)
        .await
        .expect("delete access");
    assert_status(tags.get(TagKind::Annotation, &created.name).await, 404);
}

#[tokio::test]
#[ignore = "reaches a live deployment; see the module docs"]
async fn automations_crud_walk() {
    let Some(client) = live_client(Credential::Member) else {
        return;
    };
    // An automation dispatches into a runtime group, so it needs a runtime the
    // seed does not create.
    let Some(runtime_group_id) = setting("INTROSPECTION_RUNTIME_GROUP_ID") else {
        return;
    };
    let automations = client.automations();

    let created = automations
        .create(&AutomationCreateParams {
            name: unique("automation"),
            trigger_type: AutomationTriggerType::Manual,
            description: Some("created by the rust sdk live suite".into()),
            cron_schedule: None,
            kind: None,
            prompt: Some("Reply with exactly: hi".into()),
            runtime_group_id: Some(runtime_group_id.parse().expect("runtime group id")),
            task_id: None,
            next_trigger_at: None,
            metadata: None,
            enabled: Some(false),
        })
        .await
        .expect("create");
    automations.get(created.id).await.expect("get");
    let listed = automations
        .list(&AutomationListParams::default())
        .collect_all(PAGES)
        .await
        .expect("list");
    assert!(listed.iter().any(|a| a.id == created.id));
    automations
        .update(
            created.id,
            &AutomationUpdateParams {
                description: Some("updated".into()),
                ..Default::default()
            },
        )
        .await
        .expect("update");
    automations.delete(created.id).await.expect("delete");
    assert_status(automations.get(created.id).await, 404);
}

#[tokio::test]
#[ignore = "reaches a live deployment; see the module docs"]
async fn control_plane_reads() {
    let Some(client) = live_client(Credential::ApiKey) else {
        return;
    };
    let Some(project) = setting("INTROSPECTION_PROJECT") else {
        return;
    };
    client
        .runtimes()
        .list(&RuntimeListParams {
            project: Some(project.clone()),
            ..Default::default()
        })
        .collect_all(PAGES)
        .await
        .expect("runtimes.list");
    client
        .recipes()
        .list(&RecipeListParams {
            project: Some(project.clone().into()),
            ..Default::default()
        })
        .collect_all(PAGES)
        .await
        .expect("recipes.list");
    client
        .experiments()
        .list(&ExperimentListParams {
            project: Some(project.into()),
            ..Default::default()
        })
        .collect_all(PAGES)
        .await
        .expect("experiments.list");
    client
        .connectors()
        .list(&ConnectorListParams::default())
        .collect_all(PAGES)
        .await
        .expect("connectors.list");
    assert_status(client.recipes().get(Uuid::now_v7()).await, 404);
}

#[tokio::test]
#[ignore = "reaches a live deployment; see the module docs"]
async fn telemetry_reads() {
    let Some(client) = live_client(Credential::ApiKey) else {
        return;
    };
    client
        .conversations()
        .list(&ConversationListParams::default())
        .expect("conversations params")
        .collect_all(1)
        .await
        .expect("conversations.list");
    client
        .events()
        .list(&EventListParams::new(IntrospectionEventName::Annotation))
        .expect("events params")
        .collect_all(1)
        .await
        .expect("events.list");
}

#[tokio::test]
#[ignore = "reaches a live deployment; see the module docs"]
async fn members_reads_with_session() {
    let Some(client) = live_client(Credential::Session) else {
        return;
    };
    let members = client
        .members()
        .list(&MemberListParams::default())
        .collect_all(PAGES)
        .await
        .expect("members.list");
    let first = members.first().expect("the seeded org has members");
    client.members().get(first.id).await.expect("members.get");
}

#[tokio::test]
#[ignore = "reaches a live deployment; see the module docs"]
async fn rejected_credentials_are_typed() {
    let Some(_) = live_client(Credential::ApiKey) else {
        return;
    };
    let config = ClientConfig::builder()
        .token("intro_dev_not_a_real_key")
        .advanced(AdvancedOptions {
            base_api_url: env::var("INTROSPECTION_BASE_API_URL").ok(),
            dp_url: env::var("INTROSPECTION_DP_URL").ok(),
            ..Default::default()
        })
        .build()
        .expect("client config");
    let client = IntrospectionClient::new(config).expect("client");
    assert_status(client.files().get(&Uuid::now_v7().to_string()).await, 401);
}
