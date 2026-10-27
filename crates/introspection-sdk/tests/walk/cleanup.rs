//! Everything the walk creates, found by its fixed names and removed in
//! reverse, so step 0 clears a crashed run and step 19 clears this one. The
//! JS walk's `removeWalkResources`, request for request.

use futures::StreamExt as _;
use introspection_sdk::api::ApiResult;
use introspection_sdk::{
    ApiKeyDeleteParams, ApiKeyListParams, AutomationListParams, ConnectionListParams,
    ConnectorListParams, EndpointListParams, ExperimentListParams, FileListParams,
    IntrospectionClient, RecipeListParams, RepositoryListParams, RuntimeListParams,
    RuntimeSettingListParams, ShareListParams, ShareResourceType, StringOrUuid, TagKind,
    TaskListParams,
};
use uuid::Uuid;

use crate::support::walk;
use crate::Walk;

/// A `404` is what a removal may meet: the thing is already gone.
pub fn ignore_404(result: ApiResult<()>) {
    if let Err(err) = result {
        assert_eq!(err.status(), Some(404), "a removal failed: {err}");
    }
}

/// Tasks and files are listed as the project's agent member (the service
/// account) once it exists: a member's own token lists only the tasks it
/// owns, not the runner's.
pub async fn remove_walk_resources(w: &Walk, dp: &IntrospectionClient) {
    let owner = w.owner();
    let member = w.member();
    let project = w.project_id();
    remove_api_keys(&owner, project).await;
    let mut automations = member.automations().list(&AutomationListParams::default());
    while let Some(a) = automations.next().await {
        let a = a.expect("list automations");
        if a.name == walk::AUTOMATION_NAME {
            member
                .automations()
                .delete(a.id)
                .await
                .expect("delete automation");
        }
    }
    ignore_404(member.tags().delete(TagKind::Access, walk::TAG_NAME).await);
    remove_tasks_and_files(dp).await;
    remove_control_plane(&owner, project).await;
}

async fn remove_api_keys(owner: &IntrospectionClient, project: Uuid) {
    let keys = owner
        .api_keys()
        .list(&ApiKeyListParams {
            project: Some(project.into()),
            ..Default::default()
        })
        .await
        .expect("list api keys");
    let names = [
        walk::API_KEY_NAME.to_string(),
        format!("{} (renamed)", walk::API_KEY_NAME),
    ];
    for key in keys {
        if names.contains(&key.name.clone().unwrap_or_default()) {
            owner
                .api_keys()
                .delete(
                    key.id,
                    &ApiKeyDeleteParams {
                        project: Some(project.into()),
                        ..Default::default()
                    },
                )
                .await
                .expect("delete api key");
        }
    }
}

async fn remove_tasks_and_files(dp: &IntrospectionClient) {
    let tasks = dp
        .tasks()
        .list(&TaskListParams::default())
        .collect_all(0)
        .await
        .expect("list tasks");
    for task in tasks {
        let suite = task.metadata.as_ref().and_then(|m| m.get("suite"));
        if suite.and_then(|s| s.as_str()) == Some("walk") {
            dp.tasks()
                .delete(&task.id.to_string())
                .await
                .expect("delete task");
        }
    }
    let files = dp
        .files()
        .list(&FileListParams::default())
        .collect_all(0)
        .await
        .expect("list files");
    let walk_files: Vec<String> = files
        .into_iter()
        .filter(|f| {
            f.name == walk::INPUT_FILE_NAME
                || f.name == walk::PERSONA_FILE_NAME
                || f.name.ends_with(walk::OUTPUT_FILE_NAME)
        })
        .map(|f| f.id.to_string())
        .collect();
    let shares = dp
        .shares()
        .list(&ShareListParams {
            resource_type: Some(ShareResourceType::File),
            ..Default::default()
        })
        .collect_all(0)
        .await
        .expect("list shares");
    for share in shares {
        if walk_files.contains(&share.resource_id) {
            dp.shares()
                .delete(&share.id.to_string())
                .await
                .expect("delete share");
        }
    }
    for id in walk_files {
        ignore_404(dp.files().delete(&id).await);
    }
}

async fn remove_control_plane(owner: &IntrospectionClient, project: Uuid) {
    remove_runtimes(owner, project).await;
    remove_connectors(owner, project).await;
    remove_settings(owner, project).await;
    remove_repositories(owner, project).await;
    remove_applications(owner, project).await;
}

async fn remove_runtimes(owner: &IntrospectionClient, project: Uuid) {
    let experiments = owner
        .experiments()
        .list(&ExperimentListParams {
            project: Some(project.into()),
            ..Default::default()
        })
        .collect_all(0)
        .await
        .expect("list experiments");
    for x in experiments {
        if walk::EXPERIMENT_NAMES.contains(&x.name.as_str()) {
            owner
                .experiments()
                .delete(x.id, project)
                .await
                .expect("delete experiment");
        }
    }
    let runtimes = owner
        .runtimes()
        .list(&RuntimeListParams {
            project: Some(project.to_string()),
            runtime: Some(walk::RUNTIME_SLUG.into()),
            ..Default::default()
        })
        .collect_all(0)
        .await
        .expect("list runtimes");
    for r in runtimes {
        // Deleting the group's live version takes its siblings with it.
        ignore_404(owner.runtimes().delete(r.id, Some(project.into())).await);
    }
    let endpoints = owner
        .endpoints()
        .list(&EndpointListParams {
            project: Some(project.into()),
            mcp_server_id: Some(walk::MCP_SERVER_ID.into()),
            ..Default::default()
        })
        .collect_all(0)
        .await
        .expect("list endpoints");
    for e in endpoints {
        ignore_404(owner.endpoints().delete(e.id, Some(project.into())).await);
    }
}

async fn remove_applications(owner: &IntrospectionClient, project: Uuid) {
    let applications = owner
        .applications()
        .list(&introspection_sdk::ApplicationListParams {
            project: Some(project.into()),
            ..Default::default()
        })
        .collect_all(0)
        .await
        .expect("list applications");
    for a in applications {
        if a.name.starts_with(walk::APPLICATION_NAME) || a.name == walk::ADMIN_NAME {
            owner
                .applications()
                .delete(a.id, Some(project.into()))
                .await
                .expect("delete application");
        }
    }
}

async fn remove_connectors(owner: &IntrospectionClient, project: Uuid) {
    let connectors = owner
        .connectors()
        .list(&ConnectorListParams {
            project: Some(project.into()),
            ..Default::default()
        })
        .collect_all(0)
        .await
        .expect("list connectors");
    for c in connectors
        .into_iter()
        .filter(|c| c.slug == walk::CONNECTOR_NAME)
    {
        let connections = owner
            .connectors()
            .connections
            .list(c.id, &ConnectionListParams::default())
            .collect_all(0)
            .await
            .expect("list connections");
        for conn in connections {
            owner
                .connectors()
                .connections
                .revoke(c.id, conn.id)
                .await
                .expect("revoke connection");
        }
        owner
            .connectors()
            .delete(c.id, Some(project.into()))
            .await
            .expect("delete connector");
    }
}

async fn remove_settings(owner: &IntrospectionClient, project: Uuid) {
    let q = || RuntimeSettingListParams {
        project: Some(project.into()),
        ..Default::default()
    };
    let variables = owner
        .variables()
        .list(&q())
        .collect_all(0)
        .await
        .expect("list variables");
    for v in variables
        .into_iter()
        .filter(|v| v.name == walk::VARIABLE_NAME)
    {
        owner
            .variables()
            .delete(v.id, Some(project.into()))
            .await
            .expect("delete variable");
    }
    let credentials = owner
        .credentials()
        .list(&q())
        .collect_all(0)
        .await
        .expect("list credentials");
    for c in credentials
        .into_iter()
        .filter(|c| c.name == walk::CREDENTIAL_NAME)
    {
        owner
            .credentials()
            .delete(c.id, Some(project.into()))
            .await
            .expect("delete credential");
    }
}

async fn remove_repositories(owner: &IntrospectionClient, project: Uuid) {
    let repositories = owner
        .repositories()
        .list(&RepositoryListParams {
            project: Some(StringOrUuid::from(project)),
            slug: Some(walk::RECIPE_SLUG.into()),
            ..Default::default()
        })
        .await
        .expect("list repositories");
    for repo in repositories {
        let recipes = owner
            .recipes()
            .list(&RecipeListParams {
                project: Some(project.into()),
                repository_id: Some(repo.id),
                ..Default::default()
            })
            .collect_all(0)
            .await
            .expect("list recipes");
        for r in recipes {
            owner
                .recipes()
                .delete(r.id, Some(project.into()))
                .await
                .expect("delete recipe");
        }
        owner
            .repositories()
            .delete(repo.id, project)
            .await
            .expect("delete repository");
    }
}
