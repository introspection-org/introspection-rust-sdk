//! Step 17: Avery's work, read by Blake (a member share), Casey (the
//! platform group tag), Drew (an administrator) and Emery (only a
//! project-wide share).

use std::collections::HashMap;
use std::time::Duration;

use introspection_sdk::api::ApiResult;
use introspection_sdk::{
    service_account_token, ApplicationCreateParams, ApplicationSecretCreateParams, ApplicationType,
    Conversation, ConversationListParams, CustomerMemberCreate, DataPlaneResources, FileCreateText,
    FileListParams, MemberCreateParams, MemberListParams, MemberType, MemberUpdateParams,
    RunRequest, Runner, RunnerIdentity, ServiceAccountTokenParams, ShareCreate, ShareResourceType,
    Task, TaskCreate, TaskListParams, TaskStatus,
};
use serde_json::json;
use uuid::Uuid;

use crate::steps::work::list_files;
use crate::support::{settled, walk, Creds};
use crate::{set, Walk};

/// What one person can open: the conversation, the task, Avery's file and
/// the task's output file.
type Reads = [bool; 4];
const ALL: Reads = [true; 4];
const NONE: Reads = [false; 4];
/// Casey holds one of the task's two tags, which opens the task and its
/// conversation; the output file carries no tags of its own, so a tag
/// reader cannot open it.
const BY_TAG: Reads = [true, true, false, false];

struct Targets {
    conversation: String,
    task: String,
    file: String,
    output: String,
}

async fn can<T>(read: impl std::future::Future<Output = ApiResult<T>>) -> bool {
    match read.await {
        Ok(_) => true,
        Err(err) if err.status() == Some(404) => false,
        Err(err) => panic!("a read failed: {err}"),
    }
}

async fn reads(who: &impl DataPlaneResources, t: &Targets) -> Reads {
    [
        can(who.conversations().get(&t.conversation)).await,
        can(who.tasks().get(&t.task)).await,
        can(who.files().get(&t.file)).await,
        can(who.files().get(&t.output)).await,
    ]
}

struct People {
    avery: Runner,
    blake: Runner,
    casey: Runner,
    drew: introspection_sdk::IntrospectionClient,
    emery: Runner,
    blake_id: Uuid,
}

impl People {
    async fn matrix(&self, t: &Targets) -> [Reads; 5] {
        [
            reads(&self.avery, t).await,
            reads(&self.blake, t).await,
            reads(&self.casey, t).await,
            reads(&self.drew, t).await,
            reads(&self.emery, t).await,
        ]
    }
}

/// Drew, the administrator: no person signs in on a walk, so Drew is an
/// application bound to the organisation's admin agent member, whose
/// client-credentials token is privileged.
async fn drew(w: &Walk) -> (introspection_sdk::IntrospectionClient, Uuid) {
    let owner = w.owner();
    let project = w.project_id();
    let agents = owner
        .members()
        .list(&MemberListParams {
            member_type: Some(MemberType::Agent),
            ..Default::default()
        })
        .collect_all(0)
        .await
        .expect("list agent members");
    let admin = agents
        .into_iter()
        .find(|m| m.role == "admin")
        .expect("the organisation's admin agent");
    let app = owner
        .applications()
        .create(
            &ApplicationCreateParams {
                name: walk::ADMIN_NAME.into(),
                application_type: ApplicationType::ServiceAccount,
                environment: None,
                redirect_uris: None,
                allowed_origins: None,
                allowed_scopes: Some(
                    [
                        "conversations:read",
                        "tasks:read",
                        "files:read",
                        "shares:read",
                        "shares:delete",
                    ]
                    .map(String::from)
                    .to_vec(),
                ),
                agent_member_id: Some(admin.id),
            },
            Some(project.into()),
        )
        .await
        .expect("create Drew's application");
    let secret = owner
        .applications()
        .secrets
        .create(
            app.id,
            &ApplicationSecretCreateParams {
                name: Some("drew".into()),
            },
        )
        .await
        .expect("create Drew's secret");
    let token = service_account_token(
        ServiceAccountTokenParams::builder()
            .client_id(app.client_id.clone())
            .client_secret(secret.secret.expect("the plaintext").expose())
            .project(project.to_string())
            .base_api_url(w.settings.base_api_url.clone())
            .build()
            .expect("token params"),
    )
    .await
    .expect("mint Drew's token");
    let creds = Creds {
        base_api_url: w.settings.base_api_url.clone(),
        dp_url: token.dp_url.clone(),
        token: token.access_token,
        cp_session: None,
    };
    (creds.client(), app.id)
}

/// Each person a customer member with a runner of their own. Members have no
/// public delete, so a persona from an earlier run is reused and its name
/// and tags set again.
async fn persona(w: &Walk, name: &str, tags: &[&str]) -> (Uuid, Runner) {
    let owner = w.owner();
    let external_user_id = format!("user:walk-{name}");
    let display = format!("{}{}", name[..1].to_uppercase(), &name[1..]);
    let tags: Vec<String> = tags.iter().map(|t| t.to_string()).collect();
    let mut existing = owner.members().list(&MemberListParams {
        member_type: Some(MemberType::Customer),
        external_user_ids: Some(vec![external_user_id.clone()]),
        ..Default::default()
    });
    let existing = existing
        .next_page()
        .await
        .expect("look the persona up")
        .expect("a page");
    let id = match existing.records.first() {
        Some(member) => {
            owner
                .members()
                .update(
                    member.id,
                    &MemberUpdateParams {
                        name: Some(display),
                        tags: Some(tags),
                        ..Default::default()
                    },
                )
                .await
                .expect("reset the persona");
            member.id
        }
        None => {
            owner
                .members()
                .create(&MemberCreateParams::Customer(CustomerMemberCreate {
                    external_user_id: external_user_id.clone(),
                    name: Some(display),
                    tags: Some(tags),
                    email: None,
                    metadata: None,
                    policy_attributes: None,
                }))
                .await
                .expect("create the persona")
                .id
        }
    };
    let runner = w
        .sa()
        .runtimes()
        .handle(set(&w.runtime, "the runtime").id)
        .run(RunRequest {
            agent_name: Some(w.settings.agent.clone()),
            identity: Some(RunnerIdentity {
                user_id: Some(format!("walk-{name}")),
                ..Default::default()
            }),
            ..Default::default()
        })
        .await
        .expect("a runner for the persona");
    (id, runner)
}

async fn people(w: &Walk) -> (People, Uuid) {
    let (drew, drew_app) = drew(w).await;
    let mut runners = Vec::new();
    let mut blake_id = None;
    for (name, tags) in walk::PERSONAS {
        let (id, runner) = persona(w, name, tags).await;
        if name == "blake" {
            blake_id = Some(id);
        }
        runners.push(runner);
    }
    let [avery, blake, casey, emery]: [Runner; 4] = runners.try_into().ok().expect("four personas");
    let people = People {
        avery,
        blake,
        casey,
        drew,
        emery,
        blake_id: blake_id.expect("Blake"),
    };
    (people, drew_app)
}

/// Avery makes everything first: a task tagged for the platform group, its
/// conversation and output file, and a file of their own.
async fn averys_work(w: &Walk, avery: &Runner) -> Targets {
    let member = w.member();
    // The owner login is privileged, so its quiet client polls for what
    // Avery makes; every person's own read is then recorded once.
    let owner_quiet = set(&w.member, "the member token").quiet();
    let tags = vec![walk::GROUP_TAG.to_string(), "suite:walk".to_string()];
    let conversation_metadata: HashMap<String, String> = [("suite", "walk"), ("persona", "avery")]
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .into();
    let started = avery
        .tasks()
        .start(&TaskCreate {
            prompt: Some(w.settings.prompt()),
            agent_name: Some(w.settings.agent.clone()),
            metadata: Some([("suite".to_string(), json!("walk"))].into()),
            conversation_metadata: Some(conversation_metadata.clone()),
            tags: Some(tags.clone()),
            idle_timeout_seconds: Some(180),
            ..Default::default()
        })
        .await
        .expect("Avery starts a task");
    started.text().await.expect("stream Avery's turn");
    let id = started.task.as_ref().expect("the task").id.to_string();
    let (tasks, quiet_tasks) = (avery.tasks(), owner_quiet.tasks());
    let task = settled(
        w.mode,
        "Avery's task",
        Duration::from_secs(300),
        || tasks.get(&id),
        || quiet_tasks.get(&id),
        |t: &Task| {
            matches!(
                t.status,
                TaskStatus::Idle | TaskStatus::AwaitingUser | TaskStatus::Completed
            )
        },
    )
    .await;
    assert_eq!(task.tags, tags);
    // A row carrying two tags is found by either one.
    for tag in &tags {
        let listed = member
            .tasks()
            .list(&TaskListParams {
                tag: Some(tag.clone()),
                ..Default::default()
            })
            .collect_all(0)
            .await
            .expect("list tasks by tag");
        assert!(
            listed.iter().any(|t| t.id == task.id),
            "the task list by {tag}"
        );
    }
    assert_eq!(
        task.conversation_metadata.as_ref(),
        Some(&conversation_metadata.clone().into_iter().collect())
    );
    let by_task = FileListParams {
        filters: Some([("task_id".to_string(), json!(task.id))].into()),
        ..Default::default()
    };
    let outputs = settled(
        w.mode,
        "Avery's output",
        Duration::from_secs(120),
        || list_files(&member, &by_task),
        || list_files(&owner_quiet, &by_task),
        |files: &Vec<introspection_sdk::File>| {
            files
                .iter()
                .any(|f| f.name.ends_with(walk::OUTPUT_FILE_NAME))
        },
    )
    .await;
    let output = outputs
        .into_iter()
        .find(|f| f.name.ends_with(walk::OUTPUT_FILE_NAME))
        .expect("Avery's output file");
    // `llm_call_count` says when the conversation has landed.
    let conversations = member.conversations();
    let quiet_conversations = owner_quiet.conversations();
    let _: Conversation = settled(
        w.mode,
        "Avery's conversation",
        Duration::from_secs(120),
        || conversations.get(&id),
        || quiet_conversations.get(&id),
        |c: &Conversation| c.metrics.llm_call_count > 0,
    )
    .await;
    let file = avery
        .files()
        .create_text(&FileCreateText {
            name: walk::PERSONA_FILE_NAME.into(),
            content: "Avery's notes".into(),
            mime_type: Some("text/plain".into()),
            metadata: Some([("suite".to_string(), json!("walk"))].into()),
            tags: None,
            ttl_seconds: None,
        })
        .await
        .expect("Avery writes a file");
    // Both keys must sit on the conversation (ANDed).
    let filtered = member
        .conversations()
        .list(&ConversationListParams {
            metadata: Some(conversation_metadata),
            ..Default::default()
        })
        .expect("a conversations paginator")
        .collect_all(0)
        .await
        .expect("list conversations by metadata");
    assert!(
        filtered.iter().any(|c| c.id == id),
        "conversation_metadata filters"
    );
    Targets {
        conversation: id.clone(),
        task: id,
        file: file.id.to_string(),
        output: output.id.to_string(),
    }
}

pub async fn access(w: &mut Walk) {
    let (people, drew_app) = people(w).await;
    let t = averys_work(w, &people.avery).await;
    assert_eq!(
        people.matrix(&t).await,
        [ALL, NONE, BY_TAG, ALL, NONE],
        "before any share"
    );

    // A share to one member, and one to everyone holding the platform tag.
    let avery_shares = people.avery.shares();
    let to_blake = avery_shares
        .create(&ShareCreate {
            granted_member_id: Some(people.blake_id),
            ..ShareCreate::new(ShareResourceType::Conversation, t.conversation.clone())
        })
        .await
        .expect("share the conversation to Blake");
    let to_platform = avery_shares
        .create(&ShareCreate {
            granted_tag: Some(walk::GROUP_TAG.into()),
            ..ShareCreate::new(ShareResourceType::File, t.file.clone())
        })
        .await
        .expect("share the file to the platform group");
    // A conversation share opens the conversation, never the task.
    assert_eq!(
        people.matrix(&t).await,
        [
            ALL,
            [true, false, false, false],
            [true, true, true, false],
            ALL,
            NONE
        ],
        "a member share and a tag share"
    );
    // A share naming neither opens the conversation to the whole project.
    let to_project = avery_shares
        .create(&ShareCreate::new(
            ShareResourceType::Conversation,
            t.conversation.clone(),
        ))
        .await
        .expect("share the conversation project-wide");
    assert_eq!(
        people.matrix(&t).await[4],
        [true, false, false, false],
        "a project-wide share"
    );

    revoke(
        w,
        &people,
        &t,
        [to_blake, to_platform, to_project],
        drew_app,
    )
    .await;
}

/// Revoked, every share stops granting. A runner cannot hold
/// `shares:delete`, so the administrator revokes Avery's shares.
async fn revoke(
    w: &Walk,
    people: &People,
    t: &Targets,
    shares: [introspection_sdk::ResourceShare; 3],
    drew_app: Uuid,
) {
    for share in shares {
        people
            .drew
            .shares()
            .delete(&share.id.to_string())
            .await
            .expect("revoke a share");
    }
    assert_eq!(
        people.matrix(t).await,
        [ALL, NONE, BY_TAG, ALL, NONE],
        "every share revoked"
    );

    let member = w.member();
    member
        .files()
        .delete(&t.file)
        .await
        .expect("delete Avery's file");
    member
        .tasks()
        .delete(&t.task)
        .await
        .expect("delete Avery's task");
    w.owner()
        .applications()
        .delete(drew_app, Some(w.project_id().into()))
        .await
        .expect("delete Drew's application");
}
