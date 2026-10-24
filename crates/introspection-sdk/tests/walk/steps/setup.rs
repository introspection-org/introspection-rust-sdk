//! Steps 0 to 4: the login's project, the service account, the hosted
//! repository and the recipe its push versions.

use std::time::Duration;

use futures::StreamExt as _;
use introspection_sdk::{
    service_account_token, ApplicationCreateParams, ApplicationListParams,
    ApplicationSecretCreateParams, ApplicationType, ApplicationUpdateParams, CommitsQuery,
    ContentsQuery, IntrospectionClient, PaginationParams, Project, ProjectListParams, Recipe,
    RecipeListParams, Repository, RepositoryListParams, RepositoryProvisioningStatus, Runtime,
    ServiceAccountTokenParams,
};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::steps::expect_not_found;
use crate::support::{
    assert_stable, recordings_dir, settled, walk, walk_project_born, Creds, Mode, BOOTSTRAP_FILE,
};
use crate::{set, Walk};

/// Every scope a service-account application may hold that the walk uses.
pub const APPLICATION_SCOPES: [&str; 54] = [
    "projects:read",
    "repositories:read",
    "repositories:write",
    "repositories:delete",
    "recipes:read",
    "recipes:write",
    "runtimes:read",
    "runtimes:write",
    "experiments:read",
    "experiments:write",
    "tasks:read",
    "tasks:write",
    "tasks:delete",
    "tasks:eval",
    "tasks:exec",
    "files:read",
    "files:write",
    "files:delete",
    "conversations:read",
    "events:read",
    "metrics:read",
    "shares:read",
    "shares:write",
    "shares:delete",
    "connectors:read",
    "connectors:write",
    "connections:read",
    "connections:write",
    "connections:delete",
    "automations:read",
    "automations:write",
    "annotations:read",
    "approvals:read",
    "approvals:write",
    "images:read",
    "images:write",
    "api_keys:read",
    "api_keys:write",
    "api_keys:delete",
    "applications:read",
    "applications:write",
    "applications:delete",
    "credentials:read",
    "credentials:write",
    "credentials:delete",
    "endpoints:read",
    "endpoints:write",
    "endpoints:delete",
    "variables:read",
    "variables:write",
    "variables:delete",
    "judges:read",
    "judges:write",
    "telemetry:write",
];

pub async fn clean_slate(w: &mut Walk) {
    // Every run starts in a project of its own, created here on the login's
    // deployment and deleted by the teardown. A record run writes both ids
    // beside the recording, which is all replay needs to send the same calls.
    let owner = w.owner();
    let login_project = owner
        .projects()
        .get(w.settings.bootstrap_project_id.as_str())
        .await
        .expect("read the login's project");
    // A run that crashed before its teardown left its project behind; one
    // still running is younger than the cutoff. Measured from this run's own
    // slug, so replay decides as the recording did.
    // todo: no `Projects::create` / `delete`.
    let now =
        walk_project_born(&w.settings.project_slug).expect("the walk's own slug carries its time");
    let raw = w.owner.cp_raw();
    let projects = owner
        .projects()
        .list(&ProjectListParams::default())
        .collect_all(0)
        .await
        .expect("list projects");
    for stale in projects {
        let born = stale.slug.as_deref().and_then(walk_project_born);
        if born.is_some_and(|born| now.saturating_sub(born) > walk::STALE_AFTER_SECONDS) {
            raw.delete(&format!("/v1/projects/{}", stale.id))
                .await
                .expect("delete a crashed run's project");
        }
    }
    let project: Project = raw
        .post(
            "/v1/projects",
            &json!({
                "name": walk::PROJECT_NAME,
                "slug": w.settings.project_slug,
                "deployment_id": login_project.deployment_id,
            }),
        )
        .await
        .expect("create the walk's project");
    if w.mode == Mode::Record {
        std::fs::create_dir_all(recordings_dir()).expect("create the recordings directory");
        std::fs::write(
            recordings_dir().join(BOOTSTRAP_FILE),
            json!({ "project_id": login_project.id, "project_slug": w.settings.project_slug })
                .to_string()
                + "\n",
        )
        .expect("write bootstrap.json");
    }
    w.login_project = Some(login_project);
    // The owner's Data Plane token for the project: it carries `git:push`,
    // which no service account can, and the member-only Data Plane writes.
    // todo: no SDK method mints it.
    let minted: Value = w
        .owner
        .cp_raw()
        .post(
            "/v1/tokens",
            &json!({ "project": project.id, "expires_hours": 24 }),
        )
        .await
        .expect("mint the owner's Data Plane token");
    assert_eq!(minted["project_id"], json!(project.id));
    w.member = Some(Creds {
        base_api_url: w.settings.base_api_url.clone(),
        dp_url: w.settings.dp_url.clone(),
        token: minted["access_token"].as_str().expect("a token").into(),
        cp_session: None,
    });
    w.original_description = project.description.clone();
    w.project = Some(project);
}

pub async fn project(w: &mut Walk) {
    let owner = w.owner();
    let project = set(&w.project, "the project").clone();
    // The slug is new on every live run.
    assert_stable!(project, { ".slug" => "[per run]" }, @r#"
    {
      "created_by_member_id": "[uuid]",
      "deployment_id": "[uuid]",
      "id": "[uuid]",
      "name": "SDK end-to-end walk",
      "org_id": "[uuid]",
      "slug": "[per run]"
    }
    "#);
    let read = owner
        .projects()
        .get(project.id)
        .await
        .expect("read the project");
    assert_eq!(read.id, project.id);
    let listed = owner
        .projects()
        .list(&ProjectListParams::default())
        .collect_all(0)
        .await
        .expect("list projects");
    assert!(listed.iter().any(|p| p.id == project.id));

    // todo: no `Projects::update` / `create` / `delete` (projects:write is
    // in no SDK credential's scope), so the owner's writes go by hand.
    let raw = w.owner.cp_raw();
    let path = format!("/v1/projects/{}", project.id);
    let body = json!({ "description": "The SDK end-to-end walk ran here" });
    let updated: Project = raw.patch(&path, &body).await.expect("update the project");
    assert_eq!(
        updated.description.as_deref(),
        Some("The SDK end-to-end walk ran here")
    );
    // A PATCH with only nulls is "no fields to update", so a project that
    // had no description gets an empty one back.
    let original = w.original_description.clone().unwrap_or_default();
    let restored: Project = raw
        .patch(&path, &json!({ "description": original }))
        .await
        .expect("restore the description");
    assert_eq!(restored.description.unwrap_or_default(), original);

    // A sibling is created and deleted while the walk's project is in use.
    let sibling: Project = raw
        .post(
            "/v1/projects",
            &json!({
                "name": format!("{} (walk)", project.name),
                "slug": format!("{}{}", project.slug.clone().unwrap_or_default(), walk::SIBLING_SUFFIX),
                "deployment_id": project.deployment_id,
            }),
        )
        .await
        .expect("create a sibling project");
    assert_eq!(sibling.deployment_id, project.deployment_id);
    assert_stable!(sibling, { ".slug" => "[per run]" }, @r#"
    {
      "created_by_member_id": "[uuid]",
      "deployment_id": "[uuid]",
      "id": "[uuid]",
      "name": "SDK end-to-end walk (walk)",
      "org_id": "[uuid]",
      "slug": "[per run]"
    }
    "#);
    raw.delete(&format!("/v1/projects/{}", sibling.id))
        .await
        .expect("delete the sibling");
    expect_not_found(owner.projects().get(sibling.id).await);
}

pub async fn application(w: &mut Walk) {
    let owner = w.owner();
    let project = w.project_id();
    let application = owner
        .applications()
        .create(
            &ApplicationCreateParams {
                name: walk::APPLICATION_NAME.into(),
                application_type: ApplicationType::ServiceAccount,
                environment: None,
                redirect_uris: None,
                allowed_origins: None,
                allowed_scopes: Some(APPLICATION_SCOPES.iter().map(|s| s.to_string()).collect()),
                agent_member_id: None,
            },
            Some(project.into()),
        )
        .await
        .expect("create the service account");
    assert_eq!(application.allowed_scopes, APPLICATION_SCOPES);
    assert_stable!(application, { ".allowed_scopes" => "[APPLICATION_SCOPES]" }, @r#"
    {
      "agent_member_id": "[uuid]",
      "allowed_grants": [
        "client_credentials"
      ],
      "allowed_origins": [],
      "allowed_scopes": "[APPLICATION_SCOPES]",
      "application_type": "service_account",
      "client_id": "[redacted]",
      "created_at": "[timestamp]",
      "created_by_member_id": "[uuid]",
      "environment": "production",
      "id": "[uuid]",
      "name": "sdk-coverage-walk",
      "org_id": "[uuid]",
      "project_id": "[uuid]",
      "redirect_uris": [],
      "updated_at": "[timestamp]"
    }
    "#);

    let secrets = &owner.applications().secrets;
    let first = secrets
        .create(
            application.id,
            &ApplicationSecretCreateParams {
                name: Some("first".into()),
            },
        )
        .await
        .expect("create a secret");
    let plaintext = first
        .secret
        .clone()
        .expect("the plaintext is returned once");
    let minted = service_account_token(
        ServiceAccountTokenParams::builder()
            .client_id(application.client_id.clone())
            .client_secret(plaintext.expose())
            .project(project.to_string())
            .base_api_url(w.settings.base_api_url.clone())
            .build()
            .expect("token params"),
    )
    .await
    .expect("mint the service account's token");
    let dp_url = minted
        .dp_url
        .clone()
        .expect("the token response names the Data Plane");
    w.sa = Some(Creds {
        base_api_url: w.settings.base_api_url.clone(),
        dp_url: Some(dp_url),
        token: minted.access_token.clone(),
        cp_session: None,
    });

    let listed = owner
        .applications()
        .list(&ApplicationListParams {
            project: Some(project.into()),
            ..Default::default()
        })
        .collect_all(0)
        .await
        .expect("list applications");
    assert!(listed.iter().any(|a| a.id == application.id));
    let read = owner
        .applications()
        .get(application.id, Some(project.into()))
        .await
        .expect("read the application");
    assert_eq!(read.client_id, application.client_id);
    let renamed = owner
        .applications()
        .update(
            application.id,
            &ApplicationUpdateParams {
                name: Some(format!("{}-renamed", walk::APPLICATION_NAME)),
                ..Default::default()
            },
            Some(project.into()),
        )
        .await
        .expect("rename the application");
    assert_eq!(renamed.name, format!("{}-renamed", walk::APPLICATION_NAME));

    rotate(w, application.id, first.id).await;
    w.application = Some(application);
}

/// Rotate: a second secret, then the first revoked.
async fn rotate(w: &Walk, application: Uuid, first: Uuid) {
    let owner = w.owner();
    let secrets = &owner.applications().secrets;
    let second = secrets
        .create(
            application,
            &ApplicationSecretCreateParams {
                name: Some("second".into()),
            },
        )
        .await
        .expect("create a second secret");
    let listed = secrets
        .list(application, &PaginationParams::default())
        .collect_all(0)
        .await
        .expect("list secrets");
    assert_stable!(listed, { "[].secret" => "[omitted]" }, @r#"
    [
      {
        "application_id": "[uuid]",
        "created_at": "[timestamp]",
        "created_by_member_id": "[uuid]",
        "id": "[uuid]",
        "name": "second",
        "org_id": "[uuid]",
        "secret_prefix": "[redacted]",
        "secret_suffix": "[redacted]",
        "status": "active",
        "updated_at": "[timestamp]"
      },
      {
        "application_id": "[uuid]",
        "created_at": "[timestamp]",
        "created_by_member_id": "[uuid]",
        "id": "[uuid]",
        "name": "first",
        "org_id": "[uuid]",
        "secret_prefix": "[redacted]",
        "secret_suffix": "[redacted]",
        "status": "active",
        "updated_at": "[timestamp]"
      }
    ]
    "#);
    secrets
        .delete(application, first, None)
        .await
        .expect("revoke the first secret");
    assert_ne!(second.id, first);
}

pub async fn repository(w: &mut Walk) {
    let sa = w.sa();
    let quiet = w.sa_quiet();
    let project = w.project_id();
    // todo: no `Repositories::create`; `POST /v1/repositories` is documented
    // for the CLI only, so this is the one write the walk sends by hand.
    let created: Repository = set(&w.sa, "the service account")
        .cp_raw()
        .post(
            &format!("/v1/repositories?project={project}"),
            &json!({ "provider": "hosted", "slug": walk::RECIPE_SLUG, "default_branch": "main" }),
        )
        .await
        .expect("create the hosted repository");
    assert!(
        created.url.is_some(),
        "the credential-free Git transport URL"
    );
    assert_stable!(created, @r#"
    {
      "created_at": "[timestamp]",
      "default_branch": "main",
      "head_commit_sha": null,
      "id": "[uuid]",
      "integration_id": null,
      "is_recipe_source": false,
      "name": "hello-mcp",
      "project_id": "[uuid]",
      "provider": "introspection",
      "provisioning_status": "pending",
      "pushed_at": null,
      "seed_template": "pi-agent",
      "slug": "hello-mcp",
      "url": "[url]"
    }
    "#);

    // Provisioning seeds the hosted copy with the deployment's template; that
    // first push pins a recipe, and a runtime made from it registers the
    // group's slug, which lets the next push version the group.
    let repository = settled(
        w.mode,
        "hosted repository",
        Duration::from_secs(180),
        || sa.repositories().get(created.id, project),
        || quiet.repositories().get(created.id, project),
        |r: &Repository| {
            matches!(
                r.provisioning_status,
                RepositoryProvisioningStatus::Ready | RepositoryProvisioningStatus::Failed
            )
        },
    )
    .await;
    assert_eq!(
        repository.provisioning_status,
        RepositoryProvisioningStatus::Ready
    );
    let seeded = settled(
        w.mode,
        "seed recipe",
        Duration::from_secs(180),
        || recipes(&sa, project),
        || recipes(&quiet, project),
        |rows: &Vec<Recipe>| rows.iter().any(|r| r.repository_id == repository.id),
    )
    .await;
    let seed = seeded
        .iter()
        .find(|r| r.repository_id == repository.id)
        .expect("the seed recipe");
    // todo: no `Runtimes::create`.
    let seed_runtime: Runtime = set(&w.sa, "the service account")
        .cp_raw()
        .post(
            "/v1/runtimes",
            &json!({
                "project": project,
                "recipe_id": seed.id,
                "name": walk::RUNTIME_SLUG,
                "slug": walk::RUNTIME_SLUG,
            }),
        )
        .await
        .expect("create the seed runtime");
    assert_eq!(seed_runtime.slug, walk::RUNTIME_SLUG);

    if w.mode != Mode::Replay {
        let member = set(&w.member, "the member token");
        crate::steps::push_recipe(
            &w.settings,
            created.url.as_deref().expect("a URL"),
            &member.token,
        );
    }

    let listed = sa
        .repositories()
        .list(&RepositoryListParams {
            project: Some(project.into()),
            slug: Some(walk::RECIPE_SLUG.into()),
            ..Default::default()
        })
        .await
        .expect("list repositories");
    assert!(listed.iter().any(|r| r.id == repository.id));
    w.repository = Some(repository);
}

pub async fn recipes(
    client: &IntrospectionClient,
    project: Uuid,
) -> introspection_sdk::api::ApiResult<Vec<Recipe>> {
    client
        .recipes()
        .list(&RecipeListParams {
            project: Some(project.into()),
            ..Default::default()
        })
        .collect_all(0)
        .await
}

/// The hosted repository through the Data Plane: its head commit, its
/// branches and its contents at that commit.
async fn repository_reads(w: &Walk) -> String {
    let sa = w.sa();
    let repository = set(&w.repository, "the repository").id;
    let mut commits = sa
        .repositories()
        .commits(
            repository,
            &CommitsQuery {
                sha: Some("main".into()),
                ..Default::default()
            },
        )
        .expect("a commits paginator");
    let page = commits
        .next_page()
        .await
        .expect("list commits")
        .expect("a page");
    let head = page
        .records
        .first()
        .expect("a commit on the hosted main")
        .sha
        .clone();
    let detail = sa
        .repositories()
        .commit(repository, &head)
        .await
        .expect("read the commit");
    assert_eq!(detail.commit.sha, head);
    // todo: no `Repositories::branches`.
    let branches: Value = set(&w.sa, "the service account")
        .dp_raw()
        .get(&format!("/v1/repositories/{repository}/branches"))
        .await
        .expect("list branches");
    assert!(branches["records"]
        .as_array()
        .is_some_and(|b| b.iter().any(|b| b["name"] == "main")));
    let at_head = ContentsQuery {
        r#ref: Some(head.clone()),
        ..Default::default()
    };
    let entry = sa
        .repositories()
        .contents(repository)
        .get("agents/scripted.yaml", &at_head)
        .await
        .expect("read a file");
    let introspection_sdk::RepositoryContent::File(entry) = entry else {
        panic!("agents/scripted.yaml reads as a directory");
    };
    assert_eq!(entry.path, "agents/scripted.yaml");
    let mut root = sa
        .repositories()
        .contents(repository)
        .list(&at_head)
        .expect("a contents paginator");
    let root = root
        .next_page()
        .await
        .expect("list the root")
        .expect("a page");
    assert!(root.records.iter().any(|e| e.path == "judges"));
    head
}

pub async fn recipe(w: &mut Walk) {
    let sa = w.sa();
    let quiet = w.sa_quiet();
    let project = w.project_id();
    let head = repository_reads(w).await;
    let repository = set(&w.repository, "the repository").id;
    // The push versioned the manifest's recipe at the pushed commit.
    let versioned = settled(
        w.mode,
        "push-versioned recipe",
        Duration::from_secs(180),
        || recipes(&sa, project),
        || recipes(&quiet, project),
        |rows: &Vec<Recipe>| rows.iter().any(|r| r.git_commit_sha == head),
    )
    .await;
    let recipe = versioned
        .into_iter()
        .find(|r| r.git_commit_sha == head)
        .expect("the push-versioned recipe");
    assert_stable!(recipe, @r#"
    {
      "created_at": "[timestamp]",
      "created_by_member_id": "[uuid]",
      "description": "Harbor E2B task calling a development MCP server",
      "git_commit_sha": "[sha]",
      "git_ref": "main",
      "id": "[uuid]",
      "name": "hello-mcp",
      "org_id": "[uuid]",
      "project_id": "[uuid]",
      "repository_id": "[uuid]",
      "slug": "hello-mcp-production-main-543b8db50a83",
      "sub_path": ".",
      "updated_at": "[timestamp]"
    }
    "#);

    // A second pin of the same commit under the walk's own name: a sibling
    // of the push's row, not the same one. todo: no `Recipes::create`.
    let raw = set(&w.sa, "the service account").cp_raw();
    let pinned: Recipe = raw
        .post(
            "/v1/recipes",
            &json!({
                "project": project,
                "repository_id": repository,
                "name": "hello-mcp (walk pin)",
                "git_ref": "main",
                "git_commit_sha": head,
            }),
        )
        .await
        .expect("pin the commit");
    assert_eq!(pinned.git_commit_sha, head);
    assert_ne!(pinned.id, recipe.id);
    recipe_reads(w, &recipe).await;
    w.recipe = Some(recipe);
    w.pinned_recipe = Some(pinned);
}

/// Read, list and describe the push's recipe.
async fn recipe_reads(w: &Walk, recipe: &Recipe) {
    let sa = w.sa();
    let project = w.project_id();
    let raw = set(&w.sa, "the service account").cp_raw();
    let read = sa.recipes().get(recipe.id).await.expect("read the recipe");
    assert_eq!(read.slug, recipe.slug);
    let mut seen = false;
    let mut listed = sa.recipes().list(&RecipeListParams {
        project: Some(project.into()),
        ..Default::default()
    });
    while let Some(item) = listed.next().await {
        seen |= item.expect("list recipes").id == recipe.id;
    }
    assert!(seen);
    // todo: no `Recipes::update`.
    let updated: Recipe = raw
        .patch(
            &format!("/v1/recipes/{}", recipe.id),
            &json!({ "description": "The walk's recipe" }),
        )
        .await
        .expect("describe the recipe");
    assert_eq!(updated.description.as_deref(), Some("The walk's recipe"));
}
