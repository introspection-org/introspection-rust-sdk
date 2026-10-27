//! Steps 5 to 8: what a runtime reads (a credential, a variable), the MCP
//! connector, the runtime versions, and the grant that binds them.

use std::collections::BTreeMap;
use std::time::Duration;

use futures::StreamExt as _;
use introspection_sdk::{
    ConnectionListParams, ConnectorAuthMode, ConnectorAuthorizeBinding, ConnectorAuthorizeParams,
    ConnectorCreateParams, ConnectorListParams, ConnectorOAuthDiscoveryParams,
    ConnectorUpdateParams, CredentialCreateParams, CredentialUpdateParams, EndpointCreateParams,
    EndpointKind, EndpointListParams, EndpointUpdateParams, IntrospectionClient, Lane, Runtime,
    RuntimeCreateParams, RuntimeListParams, RuntimeSettingListParams, RuntimeUpdateParams, Secret,
    VariableCreateParams, VariableKind, VariableUpdateParams,
};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::proxy::Exchange;
use crate::support::{assert_stable, settled_raw, walk};
use crate::{set, Walk};

pub async fn credential_and_variable(w: &mut Walk) {
    let sa = w.sa();
    let project = w.project_id();
    let credential = sa
        .credentials()
        .create(
            &CredentialCreateParams {
                name: walk::CREDENTIAL_NAME.into(),
                secret: Secret::new(walk::CREDENTIAL_SECRET),
                runtime_id: None,
                runtime_group_id: None,
                environment: None,
            },
            Some(project.into()),
        )
        .await
        .expect("create the credential");
    // A secret never reads back: `Credential` has no field to hold one.
    assert_stable!(credential, @r#"
    {
      "created_at": "[timestamp]",
      "created_by_member_id": "[uuid]",
      "has_secret": true,
      "id": "[uuid]",
      "name": "WALK_SECRET",
      "org_id": "[uuid]",
      "project_id": "[uuid]",
      "secret_display": "walk***0001",
      "secret_set_at": "[timestamp]",
      "updated_at": "[timestamp]"
    }
    "#);
    let q = RuntimeSettingListParams {
        project: Some(project.into()),
        ..Default::default()
    };
    let listed = sa
        .credentials()
        .list(&q)
        .collect_all(0)
        .await
        .expect("list credentials");
    assert!(listed.iter().any(|c| c.id == credential.id));
    let read = sa
        .credentials()
        .get(credential.id, Some(project.into()))
        .await
        .expect("read the credential");
    assert_eq!(read.name, walk::CREDENTIAL_NAME);
    sa.credentials()
        .update(
            credential.id,
            &CredentialUpdateParams {
                secret: Some(Secret::new(format!("{}-rotated", walk::CREDENTIAL_SECRET))),
                // `null`: shared across every lane, as it already is.
                environment: Some(None),
                ..Default::default()
            },
            Some(project.into()),
        )
        .await
        .expect("rotate the credential");
    variable(&sa, project).await;
}

async fn variable(sa: &IntrospectionClient, project: Uuid) {
    let q = RuntimeSettingListParams {
        project: Some(project.into()),
        ..Default::default()
    };
    let variable = sa
        .variables()
        .create(
            &VariableCreateParams {
                name: walk::VARIABLE_NAME.into(),
                kind: Some(VariableKind::Value),
                value: Some(walk::VARIABLE_VALUE.into()),
                runtime_id: None,
                runtime_group_id: None,
                environment: None,
            },
            Some(project.into()),
        )
        .await
        .expect("create the variable");
    assert_stable!(variable, @r#"
    {
      "created_at": "[timestamp]",
      "created_by_member_id": "[uuid]",
      "id": "[uuid]",
      "kind": "value",
      "name": "WALK_MARKER",
      "org_id": "[uuid]",
      "project_id": "[uuid]",
      "updated_at": "[timestamp]",
      "value": "walk-marker-7f3a"
    }
    "#);
    let listed = sa
        .variables()
        .list(&q)
        .collect_all(0)
        .await
        .expect("list variables");
    assert!(listed.iter().any(|v| v.id == variable.id));
    let staged = sa
        .variables()
        .update(
            variable.id,
            &VariableUpdateParams {
                environment: Some(Some(Lane::Staging)),
                ..Default::default()
            },
            Some(project.into()),
        )
        .await
        .expect("move the variable to staging");
    assert_eq!(staged.environment, Some(Lane::Staging));
    // `null` makes it shared across every lane again, which the runtime's
    // production lane reads later.
    let updated = sa
        .variables()
        .update(
            variable.id,
            &VariableUpdateParams {
                value: Some(walk::VARIABLE_VALUE.into()),
                environment: Some(None),
                ..Default::default()
            },
            Some(project.into()),
        )
        .await
        .expect("update the variable");
    assert_eq!(updated.value.as_deref(), Some(walk::VARIABLE_VALUE));
    assert_eq!(updated.environment, None);
}

pub async fn connector(w: &mut Walk) {
    let sa = w.sa();
    let project = w.project_id();
    let discovered = sa
        .connectors()
        .discover_oauth(&ConnectorOAuthDiscoveryParams {
            issuer: walk::MCP_ISSUER.into(),
        })
        .await
        .expect("discover the issuer's OAuth metadata");
    assert!(!discovered.authorization_endpoint.is_empty());
    // Naming the issuer alone makes the Control Plane discover the endpoints,
    // register a client dynamically and record PKCE support.
    let connector = sa
        .connectors()
        .create(
            &ConnectorCreateParams {
                name: walk::CONNECTOR_NAME.into(),
                slug: Some(walk::CONNECTOR_NAME.into()),
                provider: walk::CONNECTOR_NAME.into(),
                auth_mode: ConnectorAuthMode::OauthStored,
                issuer: Some(walk::MCP_ISSUER.into()),
                api_hosts: Some(vec![walk::MCP_HOST.into()]),
                environment: None,
                agent_member_id: None,
                authorization_endpoint: None,
                token_endpoint: None,
                scopes: None,
                client_id: None,
                client_secret: None,
                signing_secret: None,
                metadata: None,
                person_server_url: None,
                application_id: None,
                assertion_audience: None,
                webhook_url: None,
                policy_entities: None,
            },
            Some(project.into()),
        )
        .await
        .expect("create the connector");
    assert_stable!(connector, @r#"
    {
      "agent_member_id": "[uuid]",
      "api_hosts": [
        "example-server.modelcontextprotocol.io"
      ],
      "auth_mode": "oauth_stored",
      "authorization_endpoint": "[url]",
      "client_id": "[uuid]",
      "created_at": "[timestamp]",
      "created_by_member_id": "[uuid]",
      "environment": "production",
      "id": "[uuid]",
      "metadata": {
        "issuer": "[url]",
        "oauth": {
          "client_registration": "dynamic",
          "code_challenge_methods_supported": [
            "S256"
          ],
          "registration_endpoint": "[url]",
          "resource": "[url]",
          "token_endpoint_auth_methods_supported": [
            "none"
          ]
        }
      },
      "name": "hello-mcp",
      "org_id": "[uuid]",
      "project_id": "[uuid]",
      "provider": "hello-mcp",
      "requires_runtime": false,
      "scopes": [],
      "slug": "hello-mcp",
      "status": "pending",
      "token_endpoint": "[url]",
      "updated_at": "[timestamp]"
    }
    "#);
    let read = sa
        .connectors()
        .get(connector.id, Some(project.into()))
        .await
        .expect("read the connector");
    assert_eq!(read.slug, walk::CONNECTOR_NAME);
    let listed = sa
        .connectors()
        .list(&ConnectorListParams {
            project: Some(project.into()),
            ..Default::default()
        })
        .collect_all(0)
        .await
        .expect("list connectors");
    assert!(listed.iter().any(|c| c.id == connector.id));
    let name = format!("{} (walk)", walk::CONNECTOR_NAME);
    let updated = sa
        .connectors()
        .update(
            connector.id,
            &ConnectorUpdateParams {
                name: Some(name.clone()),
                ..Default::default()
            },
            Some(project.into()),
        )
        .await
        .expect("rename the connector");
    assert_eq!(updated.name, name);
    w.connector = Some(connector);
}

async fn runtimes(
    client: &IntrospectionClient,
    project: Uuid,
) -> introspection_sdk::api::ApiResult<Vec<Runtime>> {
    client
        .runtimes()
        .list(&RuntimeListParams {
            project: Some(project.to_string()),
            runtime: Some(walk::RUNTIME_SLUG.into()),
            ..Default::default()
        })
        .collect_all(0)
        .await
}

/// The bake's end, from a raw record. todo: `Runtime` has no
/// `image_build_status` (nor `image_build_metadata`, `recipe_kind`,
/// `recipe_ref` or `mcp_requirements`), so the SDK cannot say when a
/// runtime is ready.
fn baked(runtime: &Value) -> bool {
    matches!(
        runtime["image_build_status"].as_str(),
        Some("ready" | "failed")
    )
}

pub async fn runtime(w: &mut Walk) {
    let sa = w.sa();
    let project = w.project_id();
    let creds = set(&w.sa, "the service account");
    let recipe = set(&w.recipe, "the recipe").id;
    let bake = Duration::from_secs(30 * 60);
    // The push versioned the group at the recipe's commit; that version is
    // the runtime the walk runs on, and the API creates the next one.
    let quiet = creds.cp_raw_quiet();
    let list_path = format!(
        "/v1/runtimes?project={project}&runtime={}",
        walk::RUNTIME_SLUG
    );
    let versioned = settled_raw(
        w.mode,
        "push-versioned runtime bake",
        bake,
        || runtimes(&sa, project),
        || quiet.get::<Value>(&list_path),
        |page: &Value| {
            page["records"]
                .as_array()
                .and_then(|rows| rows.iter().find(|r| r["recipe_id"] == json!(recipe)))
                .is_some_and(baked)
        },
    )
    .await;
    let runtime = versioned
        .into_iter()
        .find(|r| r.recipe_id == Some(recipe))
        .expect("the push-versioned runtime");
    assert_stable!(runtime, @r#"
    {
      "config_json": {
        "connectors": [
          "hello-mcp"
        ]
      },
      "created_at": "[timestamp]",
      "created_by_member_id": "[uuid]",
      "description": "Harbor E2B task calling a development MCP server",
      "environments": [
        "staging",
        "production"
      ],
      "id": "[uuid]",
      "image_build_metadata": {
        "built_at": "[timestamp]",
        "external_image_name": "[redacted]",
        "image_tag": "[redacted]",
        "size_bytes": "[number]",
        "verdict_by": "[timestamp]"
      },
      "image_build_status": "ready",
      "kind": "byor",
      "llm_mode": "managed",
      "name": "hello-mcp",
      "org_id": "[uuid]",
      "project_id": "[uuid]",
      "recipe_id": "[uuid]",
      "recipe_kind": "production",
      "recipe_ref": "main",
      "runtime_group_id": "[uuid]",
      "runtime_group_type": "custom",
      "slug": "hello-mcp",
      "updated_at": "[timestamp]"
    }
    "#);

    // A version's identity in its group is its recipe, so the next version
    // pins the walk's own recipe of the same commit.
    let created = sa
        .runtimes()
        .create(&RuntimeCreateParams {
            slug: Some(walk::RUNTIME_SLUG.into()),
            config_json: Some(BTreeMap::from([(
                "connectors".to_string(),
                json!([walk::CONNECTOR_NAME]),
            )])),
            ..RuntimeCreateParams::new(
                project,
                set(&w.pinned_recipe, "the pinned recipe").id,
                walk::RUNTIME_SLUG,
            )
        })
        .await
        .expect("create the next runtime version");
    assert_eq!(created.runtime_group_id, runtime.runtime_group_id);
    let one_path = format!("/v1/runtimes/{}", created.id);
    let runtime_v2 = settled_raw(
        w.mode,
        "runtime bake",
        bake,
        || sa.runtimes().get(created.id, None),
        || quiet.get::<Value>(&one_path),
        baked,
    )
    .await;
    let described = sa
        .runtimes()
        .update(
            runtime_v2.id,
            None,
            &RuntimeUpdateParams {
                description: Some(Some("the walk's own version".into())),
                ..Default::default()
            },
        )
        .await
        .expect("describe the version");
    assert_eq!(
        described.description.as_deref(),
        Some("the walk's own version")
    );
    // The list serves the group's live version, the newest one.
    let listed = runtimes(&sa, project).await.expect("list runtimes");
    assert!(listed
        .iter()
        .any(|r| r.runtime_group_id == runtime.runtime_group_id));
    w.runtime = Some(runtime);
    w.runtime_v2 = Some(runtime_v2);
}

pub async fn connection_and_endpoint(w: &mut Walk) {
    let sa = w.sa();
    let project = w.project_id();
    let connector = set(&w.connector, "the connector").id;
    let group = set(&w.runtime, "the runtime")
        .runtime_group_id
        .expect("a runtime group");
    let return_url = format!("{}/walk/connected", w.settings.base_api_url);
    let started = sa
        .connectors()
        .authorize(
            connector,
            &ConnectorAuthorizeParams {
                runtime: Some(group.into()),
                return_url: Some(return_url.clone()),
                binding: Some(ConnectorAuthorizeBinding {
                    environment: "production".into(),
                    mcp_server_id: walk::MCP_SERVER_ID.into(),
                    url: walk::MCP_ENDPOINT.into(),
                    name: None,
                    headers: None,
                }),
                ..Default::default()
            },
            Some(project.into()),
        )
        .await
        .expect("start the grant");
    let landed = follow_authorization(w, &started.authorize_url, &return_url).await;
    let query: std::collections::HashMap<String, String> =
        landed.query_pairs().into_owned().collect();
    assert_eq!(query.get("status").map(String::as_str), Some("active"));
    let connection_id: Uuid = query["connection_id"].parse().expect("a connection id");
    let connection = sa
        .connectors()
        .connections
        .get(connector, connection_id)
        .await
        .expect("read the connection");
    assert_stable!(connection, @r#"
    {
      "connector_id": "[uuid]",
      "created_at": "[timestamp]",
      "created_by_member_id": "[uuid]",
      "id": "[uuid]",
      "org_id": "[uuid]",
      "runtime_group_id": "[uuid]",
      "scopes_granted": [],
      "status": "active",
      "subject_type": "app",
      "token_expires_at": "[timestamp]",
      "updated_at": "[timestamp]"
    }
    "#);
    let mut seen = false;
    let mut listed = sa
        .connectors()
        .connections
        .list(connector, &ConnectionListParams::default());
    while let Some(item) = listed.next().await {
        seen |= item.expect("list connections").id == connection_id;
    }
    assert!(seen);

    endpoint(&sa, project).await;
    // todo: `ConnectionTokenParams` has no `runtime`, so the runtime-scoped
    // token read the JS SDK sends cannot be expressed; it goes by hand.
    let token: Value = set(&w.sa, "the service account")
        .cp_raw()
        .post(
            "/v1/oauth/connections/token",
            &json!({ "connector_id": connector, "runtime": group }),
        )
        .await
        .expect("read the connection's token");
    let token: introspection_sdk::ConnectionToken =
        serde_json::from_value(token).expect("a connection token");
    assert_stable!(token, @r#"
    {
      "expires_at": "[timestamp]",
      "scopes": [],
      "token": "[redacted]",
      "token_type": "bearer"
    }
    "#);
    // todo: no `include=mcp_requirements` on `Runtimes::get`, and no
    // `mcp_requirements` on `Runtime`.
    let runtime = set(&w.runtime, "the runtime").id;
    let requirements: Value = set(&w.sa, "the service account")
        .cp_raw()
        .get(&format!("/v1/runtimes/{runtime}?include=mcp_requirements"))
        .await
        .expect("read the runtime's MCP requirements");
    assert_stable!(requirements["mcp_requirements"], @"[]");
}

async fn endpoint(sa: &IntrospectionClient, project: Uuid) {
    let endpoints = sa
        .endpoints()
        .list(&EndpointListParams {
            project: Some(project.into()),
            mcp_server_id: Some(walk::MCP_SERVER_ID.into()),
            ..Default::default()
        })
        .collect_all(0)
        .await
        .expect("list endpoints");
    let endpoint = endpoints.into_iter().next().expect("the grant's endpoint");
    assert_stable!(endpoint, @r#"
    {
      "base_url": "[url]",
      "created_at": "[timestamp]",
      "created_by_member_id": "[uuid]",
      "credential_refs": [],
      "environment": "production",
      "header_names": [],
      "headers": {},
      "host": "example-server.modelcontextprotocol.io",
      "id": "[uuid]",
      "kind": "mcp",
      "metadata": {
        "connector_id": "[uuid]",
        "extra_hosts": [],
        "mcp_server_id": "hello-mcp"
      },
      "name": "hello-mcp",
      "org_id": "[uuid]",
      "project_id": "[uuid]",
      "runtime_group_id": "[uuid]",
      "updated_at": "[timestamp]"
    }
    "#);
    let read = sa
        .endpoints()
        .get(endpoint.id, Some(project.into()))
        .await
        .expect("read the endpoint");
    assert_eq!(read.kind.as_str(), "mcp");
    let renamed = sa
        .endpoints()
        .update(
            endpoint.id,
            &EndpointUpdateParams {
                name: Some("hello-mcp (walk)".into()),
                ..Default::default()
            },
            Some(project.into()),
        )
        .await
        .expect("rename the endpoint");
    assert_eq!(renamed.name, "hello-mcp (walk)");
    api_endpoint(sa, project).await;
}

/// An `api` binding the walk writes itself: created with a literal header,
/// re-scoped to every environment, and deleted. The create upserts by host,
/// so a crashed run's leftover is reused rather than duplicated.
async fn api_endpoint(sa: &IntrospectionClient, project: Uuid) {
    let created = sa
        .endpoints()
        .create(
            &EndpointCreateParams {
                kind: Some(EndpointKind::Api),
                headers: BTreeMap::from([(
                    "X-Walk-Key".to_string(),
                    Secret::new(walk::API_ENDPOINT_HEADER),
                )]),
                environment: Some(Lane::Staging),
                ..EndpointCreateParams::new(walk::API_ENDPOINT_NAME, walk::API_ENDPOINT_HOST)
            },
            Some(project.into()),
        )
        .await
        .expect("create the api endpoint");
    assert_eq!(created.kind, EndpointKind::Api);
    assert_eq!(created.header_names, vec!["X-Walk-Key"]);
    let everywhere = sa
        .endpoints()
        .update(
            created.id,
            &EndpointUpdateParams {
                environment: Some(None),
                ..Default::default()
            },
            Some(project.into()),
        )
        .await
        .expect("serve the api endpoint in every environment");
    assert_eq!(everywhere.environment, None);
    sa.endpoints()
        .delete(created.id, Some(project.into()))
        .await
        .expect("delete the api endpoint");
}

/// Complete the OAuth grant the way the MCP scenario harness does: follow
/// the provider's redirects by hand, and route the registered callback host
/// (a public name that does not reach a local stack) to the Control Plane
/// the walk talks to. These requests are the walk's own, not the SDK's, so
/// they go straight to the recorder or the replay rather than the proxy,
/// which cannot see into the provider's `https`.
async fn follow_authorization(w: &Walk, authorize_url: &str, return_url: &str) -> url::Url {
    let base = w.settings.base_api_url.clone();
    let mut next = authorize_url.to_string();
    for _ in 0..12 {
        let url = url::Url::parse(&next).expect("an absolute URL");
        if url.path() == "/v1/oauth/connections/callback" {
            next = format!("{base}{}?{}", url.path(), url.query().unwrap_or_default());
        }
        if next.starts_with(return_url) {
            return url::Url::parse(&next).expect("the return URL");
        }
        let mut headers = Vec::new();
        if next.starts_with(&base) {
            headers.push((
                "cookie".into(),
                format!("intro_cp_session={}", w.settings.cp_session),
            ));
        }
        let reply = w
            .backend
            .exchange(Exchange {
                method: "GET".into(),
                url: next.clone(),
                headers,
                body: Vec::new(),
            })
            .await;
        let location = reply
            .headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("location"))
            .map(|(_, v)| v.clone());
        let location = match (location, reply.status) {
            (Some(location), _) => location,
            (None, 200) => continue_link(&next, &String::from_utf8_lossy(&reply.body)),
            (None, status) => {
                panic!("authorization stopped at {next} with {status} and no redirect")
            }
        };
        next = url::Url::parse(&next)
            .and_then(|u| u.join(&location))
            .expect("a redirect resolves")
            .to_string();
    }
    panic!("authorization did not reach the return URL within 12 redirects");
}

/// The MCP reference server's consent page is one "Continue" link to its
/// mock upstream IdP, whose page completes from a script that navigates to
/// its callback with a fixed code and the browser's user id; the walk's user
/// id is fixed too, so the grant is the same subject every run.
fn continue_link(at: &str, page: &str) -> String {
    let link = regex::Regex::new(r#"<a\s+href="([^"]+)"[^>]*class="btn-primary""#).expect("regex");
    if let Some(found) = link.captures(page) {
        return found[1].to_string();
    }
    let state = regex::Regex::new(r"searchParams\.set\('state',\s*'([^']+)'\)").expect("regex");
    let found = state
        .captures(page)
        .filter(|_| page.contains("/mock-upstream-idp/callback"))
        .unwrap_or_else(|| panic!("authorization stopped at {at}: a page with no continue link"));
    let mut callback = url::Url::parse(at)
        .and_then(|u| u.join("/mock-upstream-idp/callback"))
        .expect("the mock IdP's callback");
    callback
        .query_pairs_mut()
        .append_pair("state", &found[1])
        .append_pair("code", "mock-auth-code")
        .append_pair("userId", "sdk-walk-user");
    callback.to_string()
}
