//! `client.runtimes().update(..)`: the `PATCH /v1/runtimes/{id}` body, backed
//! by `wiremock`.
//!
//! The lane map is the contract that matters. Per lane the wire has three
//! states — key absent (leave the lane), `null` (unpin) and a string (pin to a
//! ref) — and these tests pin the exact JSON for each.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::time::Duration;

use introspection_sdk::api::{HttpClient, HttpConfig};
use introspection_sdk::{LaneRef, RuntimeLlmMode, RuntimeUpdateParams, Runtimes};
use serde_json::{json, Value};
use uuid::Uuid;
use wiremock::matchers::{body_json, method, path, query_param, query_param_is_missing};
use wiremock::{Mock, MockServer, ResponseTemplate};

const RUNTIME_ID: &str = "11111111-1111-1111-1111-111111111111";

fn build_http(server: &MockServer) -> Arc<HttpClient> {
    let cfg = HttpConfig {
        api_url: server.uri(),
        token: "intro_test".to_string(),
        additional_headers: HashMap::new(),
        timeout: Duration::from_secs(5),
        max_retries: 2,
        retry_base: Duration::from_millis(1),
    };
    Arc::new(HttpClient::from_parts(reqwest::Client::new(), cfg))
}

fn wire(params: &RuntimeUpdateParams) -> Value {
    serde_json::to_value(params).unwrap()
}

fn runtime_json() -> Value {
    json!({
        "id": RUNTIME_ID,
        "org_id": "00000000-0000-0000-0000-0000000000aa",
        "project_id": "00000000-0000-0000-0000-0000000000bb",
        "recipe_id": null,
        "runtime_group_id": "00000000-0000-0000-0000-0000000000dd",
        "created_by_member_id": "00000000-0000-0000-0000-0000000000cc",
        "created_at": "2026-10-01T00:00:00Z",
        "updated_at": "2026-10-01T00:00:00Z",
        "name": "Support",
        "slug": "support",
        "environments": ["staging"],
        "environment_ref": {"staging": "pr/12"},
    })
}

#[test]
fn an_empty_update_sends_an_empty_object() {
    assert_eq!(wire(&RuntimeUpdateParams::default()), json!({}));
}

#[test]
fn a_lane_left_out_is_absent_on_the_wire() {
    let params = RuntimeUpdateParams {
        name: Some("Support".into()),
        ..Default::default()
    };
    assert_eq!(wire(&params), json!({"name": "Support"}));
}

#[test]
fn unpin_sends_an_explicit_null() {
    assert_eq!(
        wire(&RuntimeUpdateParams::unpin("staging")),
        json!({"environment_ref": {"staging": null}})
    );
}

#[test]
fn pin_ref_sends_the_ref_as_a_string() {
    assert_eq!(
        wire(&RuntimeUpdateParams::pin_ref("staging", "pr/12")),
        json!({"environment_ref": {"staging": "pr/12"}})
    );
    assert_eq!(
        wire(&RuntimeUpdateParams::pin_ref("production", "4f2a9c1")),
        json!({"environment_ref": {"production": "4f2a9c1"}})
    );
}

#[test]
fn one_map_carries_all_three_lane_states() {
    // `production` pinned, `staging` unpinned, and any other lane untouched
    // because it is absent.
    let params = RuntimeUpdateParams {
        environment_ref: BTreeMap::from([
            ("production".into(), LaneRef::Ref("main".into())),
            ("staging".into(), LaneRef::Unpin),
        ]),
        ..Default::default()
    };
    assert_eq!(
        serde_json::to_string(&params).unwrap(),
        r#"{"environment_ref":{"production":"main","staging":null}}"#
    );
}

#[test]
fn pin_staging_to_this_version_matches_the_cli_exact_pin() {
    assert_eq!(
        wire(&RuntimeUpdateParams::pin_staging_to_this_version()),
        json!({"environment": "staging", "environment_ref": {"staging": null}})
    );
}

#[test]
fn yank_and_unyank() {
    assert_eq!(
        wire(&RuntimeUpdateParams::yank(Some("bad build".into()))),
        json!({"yanked": true, "yanked_reason": "bad build"})
    );
    assert_eq!(
        wire(&RuntimeUpdateParams::yank(None)),
        json!({"yanked": true})
    );
    assert_eq!(
        wire(&RuntimeUpdateParams::unyank()),
        json!({"yanked": false})
    );
}

#[test]
fn description_is_set_cleared_or_left() {
    let set = RuntimeUpdateParams {
        description: Some(Some("Answers tickets".into())),
        ..Default::default()
    };
    let clear = RuntimeUpdateParams {
        description: Some(None),
        ..Default::default()
    };
    assert_eq!(wire(&set), json!({"description": "Answers tickets"}));
    assert_eq!(wire(&clear), json!({"description": null}));
    assert_eq!(wire(&RuntimeUpdateParams::default()), json!({}));
}

#[test]
fn every_field_has_its_wire_name() {
    let params = RuntimeUpdateParams {
        name: Some("Support".into()),
        description: Some(Some("Answers tickets".into())),
        llm_mode: Some(RuntimeLlmMode::Byok),
        config_json: Some(HashMap::from([("timeout".into(), json!(30))])),
        environment: Some("staging".into()),
        yanked: Some(true),
        yanked_reason: Some("bad build".into()),
        environment_ref: BTreeMap::from([("staging".into(), LaneRef::Ref("main".into()))]),
    };
    assert_eq!(
        wire(&params),
        json!({
            "name": "Support",
            "description": "Answers tickets",
            "llm_mode": "byok",
            "config_json": {"timeout": 30},
            "environment": "staging",
            "yanked": true,
            "yanked_reason": "bad build",
            "environment_ref": {"staging": "main"},
        })
    );
}

#[test]
fn a_lane_ref_reads_back_from_null_or_a_string() {
    let lanes: BTreeMap<String, LaneRef> =
        serde_json::from_value(json!({"staging": null, "production": "main"})).unwrap();
    assert_eq!(lanes["staging"], LaneRef::Unpin);
    assert_eq!(lanes["production"].as_ref_str(), Some("main"));
}

#[tokio::test]
async fn update_patches_the_runtime_in_the_project() {
    let server = MockServer::start().await;
    Mock::given(method("PATCH"))
        .and(path(format!("/v1/runtimes/{RUNTIME_ID}")))
        .and(query_param("project", "support desk"))
        .and(body_json(json!({"environment_ref": {"staging": "pr/12"}})))
        .respond_with(ResponseTemplate::new(200).set_body_json(runtime_json()))
        .expect(1)
        .mount(&server)
        .await;

    let runtime = Runtimes::new(build_http(&server))
        .update(
            Uuid::parse_str(RUNTIME_ID).unwrap(),
            Some("support desk".into()),
            &RuntimeUpdateParams::pin_ref("staging", "pr/12"),
        )
        .await
        .unwrap();

    assert_eq!(runtime.environments, vec!["staging".to_string()]);
    assert_eq!(
        runtime.environment_ref.unwrap()["staging"],
        "pr/12".to_string()
    );
}

#[tokio::test]
async fn update_leaves_the_project_to_the_credential_and_surfaces_a_rejected_ref() {
    let server = MockServer::start().await;
    Mock::given(method("PATCH"))
        .and(path(format!("/v1/runtimes/{RUNTIME_ID}")))
        .and(query_param_is_missing("project"))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({
            "detail": "A 'pr/' ref must be 'pr/N' with a positive integer.",
        })))
        .expect(1)
        .mount(&server)
        .await;

    let err = Runtimes::new(build_http(&server))
        .update(
            Uuid::parse_str(RUNTIME_ID).unwrap(),
            None,
            &RuntimeUpdateParams::pin_ref("staging", "pr/0"),
        )
        .await
        .unwrap_err();

    assert_eq!(err.status(), Some(400));
}
