//! Steps 18 and 19: the owner's reads, the sign-in routes `introspection
//! login` takes and a development API key, then everything the walk made,
//! removed in reverse.

use introspection_sdk::auth::{
    oidc_logout, DeviceCodeAuth, DeviceCodeAuthConfig, DeviceCodeParams, DeviceTokenPoll,
};
use introspection_sdk::{
    ApiKeyCapability, ApiKeyCreateParams, ApiKeyListParams, ApiKeyUpdateParams,
    BusinessMemberCreate, Lane, MemberCreateParams, MemberListParams, MemberUpdateParams,
    Organization,
};

use crate::cleanup::remove_walk_resources;
use crate::steps::expect_not_found;
use crate::support::{assert_stable, walk};
use crate::{set, Walk};

/// The owner's reads: members, an invite, the organisation and the session.
async fn members(w: &Walk) -> Organization {
    let owner = w.owner();
    let members = owner
        .members()
        .list(&MemberListParams::default())
        .collect_all(0)
        .await
        .expect("list members");
    let first = members.first().expect("a member");
    owner.members().get(first.id).await.expect("read a member");
    let org = owner
        .organizations()
        .me()
        .await
        .expect("read the organisation");
    let slug = org.slug.clone().expect("the organisation's slug");
    // An invite outlives the project, so a repeat run finds the one the last
    // run sent; and the address names this organisation, or a second
    // organisation's walk answers 409.
    let email = format!("walk-member@{slug}.example.com");
    let invited = match members
        .iter()
        .find(|m| m.email.as_deref() == Some(email.as_str()))
    {
        Some(member) => member.clone(),
        None => owner
            .members()
            .create(&MemberCreateParams::Business(BusinessMemberCreate {
                email: email.clone(),
                name: "Walk member".into(),
                role: Some("member".into()),
                tags: None,
                metadata: None,
                policy_attributes: None,
            }))
            .await
            .expect("invite a member"),
    };
    assert_eq!(invited.email.as_deref(), Some(email.as_str()));
    assert_eq!(invited.role, "member");
    let renamed = owner
        .members()
        .update(
            invited.id,
            &MemberUpdateParams {
                name: Some("Walk member (renamed)".into()),
                ..Default::default()
            },
        )
        .await
        .expect("rename the member");
    assert_eq!(renamed.name.as_deref(), Some("Walk member (renamed)"));
    let me = owner.oidc().me().await.expect("who the session is");
    assert_eq!(me.org_id, Some(set(&w.project, "the project").org_id));
    org
}

/// What `introspection login`, `logout` and `dev` reach through the SDK,
/// short of a person approving in a browser: a device code, one poll that
/// finds it still pending, a sign-out of a cookie that names no session (the
/// owner's own would end the walk's session), and the remote connection's
/// protocol description. The WebSocket itself is not recorded: it bypasses
/// the walk's HTTP proxy, and attaching needs a development login.
async fn sign_in_routes(w: &Walk) {
    let device = DeviceCodeAuth::new(
        DeviceCodeAuthConfig::builder()
            .base_api_url(w.settings.base_api_url.clone())
            .build()
            .expect("device-code config"),
    )
    .expect("device-code client");
    let code = device
        .request_code(&DeviceCodeParams::default())
        .await
        .expect("request a device code");
    assert!(!code.user_code.is_empty());
    assert!(code
        .verification_uri_complete
        .starts_with(&code.verification_uri));
    let poll = device
        .poll_token(&code.device_code)
        .await
        .expect("poll the device code");
    assert!(
        matches!(poll, DeviceTokenPoll::Pending),
        "nobody approved the code, so it is pending: {poll:?}"
    );
    oidc_logout(Some(&w.settings.base_api_url), "walk-no-session")
        .await
        .expect("sign out a cookie that names no session");

    let docs = w
        .member()
        .remotes()
        .connect_docs()
        .await
        .expect("read the remote connection's protocol");
    assert_eq!(docs.endpoint, "/v1/remotes/connect");
    assert!(docs
        .message_protocol
        .remote_to_cloud
        .iter()
        .any(|frame| frame.frame == "attach_request"));
}

pub async fn members_and_keys(w: &mut Walk) {
    let sa = w.sa();
    let project = w.project_id();
    let org = members(w).await;
    sign_in_routes(w).await;
    let key = sa
        .api_keys()
        .create(
            &ApiKeyCreateParams {
                name: Some(walk::API_KEY_NAME.into()),
                environment: Some(Lane::Development),
                allow: Some(vec![ApiKeyCapability::from("runtimes")]),
                member_id: None,
                expires_at: None,
            },
            Some(project.into()),
        )
        .await
        .expect("create a development API key");
    assert!(key.api_key.is_some(), "the plaintext key is returned once");
    assert_stable!(key, { ".api_key" => "[omitted]" }, @r#"
    {
      "allow": [
        "runtimes"
      ],
      "api_key": "[redacted]",
      "created_at": "[timestamp]",
      "environment": "development",
      "id": "[uuid]",
      "key_display": "[redacted]",
      "key_prefix": "[redacted]",
      "key_suffix": "[redacted]",
      "member_id": "[uuid]",
      "name": "walk",
      "org_id": "[uuid]",
      "prefix": "[redacted]",
      "project_id": "[uuid]",
      "redacted_key": "[redacted]",
      "scope": "telemetry:write projects:read repositories:read recipes:read recipes:write runtimes:read runtimes:write experiments:read experiments:write tasks:read tasks:write tasks:delete files:read files:write conversations:read events:read metrics:read shares:read shares:write shares:delete connectors:read connectors:write annotations:read approvals:read approvals:write",
      "status": "active"
    }
    "#);
    // The key list is its own envelope, `api_keys`, not a page.
    let keys = sa
        .api_keys()
        .list(&ApiKeyListParams {
            project: Some(project.into()),
            ..Default::default()
        })
        .await
        .expect("list API keys");
    assert!(keys.iter().any(|k| k.id == key.key.id));
    sa.api_keys()
        .update(
            key.key.id,
            &ApiKeyUpdateParams {
                name: Some(format!("{} (renamed)", walk::API_KEY_NAME)),
                ..Default::default()
            },
            Some(project.into()),
        )
        .await
        .expect("rename the key");
    sa.api_keys()
        .acknowledge(key.key.id, Some(project.into()))
        .await
        .expect("acknowledge the key");
    // The organisation's name and slug are whoever is running the walk.
    assert_stable!(org, { ".name" => "[login's]", ".slug" => "[login's]" }, @r#"
    {
      "external_org_id": "[id]",
      "hosted_git": true,
      "id": "[uuid]",
      "name": "[login's]",
      "slug": "[login's]"
    }
    "#);
}

pub async fn teardown(w: &mut Walk) {
    let sa = w.sa();
    let owner = w.owner();
    let project = w.project_id();
    // The default file list omits the upload a task holds, so it goes by id.
    let input = set(&w.input_file, "the input file").id.to_string();
    sa.files()
        .delete(&input)
        .await
        .expect("delete the input file");
    remove_walk_resources(w, &sa).await;
    expect_not_found(
        sa.tasks()
            .get(&set(&w.task, "the task").id.to_string())
            .await,
    );
    expect_not_found(sa.files().get(&input).await);
    expect_not_found(
        sa.runtimes()
            .get(set(&w.runtime_v2, "the second version").id, None)
            .await,
    );
    expect_not_found(
        sa.connectors()
            .get(set(&w.connector, "the connector").id, Some(project.into()))
            .await,
    );
    expect_not_found(
        sa.recipes()
            .get(set(&w.recipe, "the recipe").id, Some(project.into()))
            .await,
    );
    let application = set(&w.application, "the application").id;
    expect_not_found(
        owner
            .applications()
            .get(application, Some(project.into()))
            .await,
    );
    // The walk's project goes with it; the login's is untouched.
    owner
        .projects()
        .delete(project)
        .await
        .expect("delete the walk's project");
    expect_not_found(owner.projects().get(project).await);
    let login_project = set(&w.login_project, "the login's project").id;
    let after = owner
        .projects()
        .get(login_project)
        .await
        .expect("read the login's project");
    assert_eq!(after.id, login_project);
}
