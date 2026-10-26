use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use introspection_sdk::api::{HttpClient, HttpConfig};
use introspection_sdk::{
    Aauth, ApprovalDecision, ApprovalKind, ApprovalListParams, ApprovalStatus, Budget, DecidedBy,
    MissionListParams,
};
use serde_json::json;
use uuid::Uuid;
use wiremock::matchers::{body_json, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const APPROVAL_ID: &str = "019fc000-0000-7000-8000-0000000000a1";
const MISSION_ID: &str = "019fc000-0000-7000-8000-0000000000b1";
const ORG_ID: &str = "019fc000-0000-7000-8000-0000000000c1";
const PROJECT_ID: &str = "019fc000-0000-7000-8000-0000000000c2";
const SAM: &str = "019fc000-0000-7000-8000-0000000000d1";

fn aauth(server: &MockServer) -> Aauth {
    Aauth::new(Arc::new(HttpClient::from_parts(
        reqwest::Client::new(),
        HttpConfig {
            api_url: server.uri(),
            token: "intro_test".into(),
            additional_headers: HashMap::new(),
            timeout: Duration::from_secs(5),
            max_retries: 0,
            retry_base: Duration::from_millis(1),
        },
    )))
}

fn approval(status: &str, decided_by: Option<&str>) -> serde_json::Value {
    json!({
        "id": APPROVAL_ID,
        "org_id": ORG_ID,
        "project_id": PROJECT_ID,
        "kind": "mission",
        "status": status,
        "agent_id": "aauth:travel@cp.example",
        "member_id": SAM,
        "mission_id": null,
        "description": "Sydney, up to $3,000",
        "resource": null,
        "budget": null,
        "summary": null,
        "decided_budget": null,
        "decided_by_member_id": decided_by,
        "decided_at": null,
        "expires_at": "2026-10-22T12:00:00Z",
        "created_at": "2026-10-20T12:00:00Z",
        "updated_at": "2026-10-20T12:00:00Z"
    })
}

#[tokio::test]
async fn pending_approvals_are_listed_by_status() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/aauth/approvals"))
        .and(query_param("status", "pending"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "records": [approval("pending", None)],
            "next": null,
            "count": 1
        })))
        .expect(1)
        .mount(&server)
        .await;

    let approvals = aauth(&server)
        .approvals()
        .list(&ApprovalListParams::pending())
        .collect_all(0)
        .await
        .unwrap();

    assert_eq!(approvals.len(), 1);
    assert_eq!(approvals[0].kind, ApprovalKind::Mission);
    assert_eq!(approvals[0].status, ApprovalStatus::Pending);
    assert_eq!(approvals[0].member_id, Uuid::parse_str(SAM).unwrap());
}

#[tokio::test]
async fn a_decision_names_the_approver_by_identity() {
    let server = MockServer::start().await;
    let dana = "019fc000-0000-7000-8000-0000000000d2";
    Mock::given(method("PATCH"))
        .and(path(format!("/v1/aauth/approvals/{APPROVAL_ID}")))
        .and(body_json(json!({
            "status": "approved",
            "budget": {"amount": 300000, "unit": "USD", "decimals": 2},
            "decided_by": {"identity": {"user_id": "dana@acme.example"}}
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(approval("approved", Some(dana))))
        .expect(1)
        .mount(&server)
        .await;

    let decision =
        ApprovalDecision::approve(DecidedBy::user("dana@acme.example")).with_budget(Budget {
            amount: 300_000,
            unit: "USD".into(),
            decimals: 2,
        });
    let decided = aauth(&server)
        .approvals()
        .decide(Uuid::parse_str(APPROVAL_ID).unwrap(), &decision)
        .await
        .unwrap();

    assert_eq!(decided.status, ApprovalStatus::Approved);
    assert_eq!(
        decided.decided_by_member_id,
        Some(Uuid::parse_str(dana).unwrap())
    );
}

#[test]
fn an_approver_by_member_id_serializes_as_one() {
    let member = Uuid::parse_str(SAM).unwrap();
    let body = serde_json::to_value(ApprovalDecision::deny(DecidedBy::member(member))).unwrap();
    assert_eq!(
        body,
        json!({"status": "denied", "decided_by": {"member_id": SAM}})
    );
}

#[tokio::test]
async fn a_mission_reads_its_spend_and_log() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/aauth/missions"))
        .and(query_param("member_id", SAM))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "records": [{
                "id": MISSION_ID,
                "org_id": ORG_ID,
                "project_id": PROJECT_ID,
                "s256": "yUysP3ie",
                "mission": "{\"description\":\"Sydney\"}",
                "agent_id": "aauth:travel@cp.example",
                "member_id": SAM,
                "resources": ["https://booking.example"],
                "budget": {"amount": 300000, "unit": "USD", "decimals": 2},
                "spent": 165000,
                "log": [{"at": "2026-10-20T12:05:00Z", "entry": "Booked the flight"}],
                "expires_at": "2026-11-03T12:00:00Z",
                "completed_at": null,
                "created_at": "2026-10-20T12:00:00Z",
                "updated_at": "2026-10-20T12:05:00Z"
            }],
            "next": null,
            "count": 1
        })))
        .expect(1)
        .mount(&server)
        .await;

    let missions = aauth(&server)
        .missions()
        .list(&MissionListParams {
            member_id: Some(Uuid::parse_str(SAM).unwrap()),
            ..MissionListParams::default()
        })
        .collect_all(0)
        .await
        .unwrap();

    assert_eq!(missions[0].spent, 165_000);
    assert_eq!(missions[0].log[0].entry, "Booked the flight");
    assert_eq!(missions[0].budget.as_ref().unwrap().amount, 300_000);
}
