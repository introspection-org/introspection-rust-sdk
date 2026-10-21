//! The Person Server's side of four-party access, against a scripted Access
//! Server: draft `-11` §Access Server Federation (`#access-server-federation`).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use aauth_protocol::discovery::{Fetch, Fetched, JwksResolver, StaticFetch};
use aauth_protocol::federation::{
    access_server_metadata, federate, AccessOutcome, AuthTokenRequest, Federate, FederationContext,
    FederationError,
};
use aauth_protocol::httpsig::{self, Incoming, Signer, VerifyOptions};
use aauth_protocol::metadata::AccessServerMetadata;
use aauth_protocol::resource::{resource_token, AuthTokenIssuers, ResourceTokenParams};
use aauth_protocol::token::{mint, verify_token, TokenKind, VerifiedToken, VerifyTokenOptions};
use aauth_protocol::{jwt, Budget, Requirement, SigningKey};
use async_trait::async_trait;
use bytes::Bytes;
use http::{HeaderMap, HeaderValue, Method, Request, StatusCode};
use serde_json::{json, Value};

const PS: &str = "https://ps.example";
const AS: &str = "https://as.resource.example";
const RESOURCE: &str = "https://resource.example";
const NOW: i64 = 1_791_000_000;

/// An Access Server that answers each token request or poll with the next
/// scripted response, and records what it was sent.
#[derive(Default)]
struct Scripted {
    documents: Mutex<HashMap<String, Value>>,
    answers: Mutex<Vec<Fetched>>,
    sent: Mutex<Vec<Request<Bytes>>>,
}

impl Scripted {
    fn with_metadata(as_key: &SigningKey) -> Arc<Self> {
        let scripted = Arc::new(Self::default());
        // The example of #access-server-metadata.
        scripted.document(
            format!("{AS}/.well-known/aauth-access.json"),
            json!({
                "issuer": AS,
                "name": "Example Access Server",
                "auth_token_endpoint": format!("{AS}/token"),
                "jwks_uri": format!("{AS}/.well-known/jwks.json"),
            }),
        );
        scripted.document(format!("{AS}/.well-known/jwks.json"), json!(as_key.jwks()));
        scripted
    }

    fn document(&self, url: String, document: Value) {
        self.documents.lock().unwrap().insert(url, document);
    }

    fn answer(&self, status: StatusCode, headers: &[(&str, &str)], body: Value) {
        let mut map = HeaderMap::new();
        for (name, value) in headers {
            map.insert(
                http::HeaderName::from_bytes(name.as_bytes()).unwrap(),
                HeaderValue::from_str(value).unwrap(),
            );
        }
        self.answers.lock().unwrap().push(Fetched {
            status,
            headers: map,
            body: Bytes::from(body.to_string()),
        });
    }

    fn sent(&self) -> Vec<Request<Bytes>> {
        self.sent.lock().unwrap().drain(..).collect()
    }
}

#[async_trait]
impl Fetch for Scripted {
    async fn get(&self, url: &str, _headers: HeaderMap) -> aauth_protocol::Result<Fetched> {
        let document = self.documents.lock().unwrap().get(url).cloned();
        Ok(Fetched {
            status: if document.is_some() {
                StatusCode::OK
            } else {
                StatusCode::NOT_FOUND
            },
            headers: HeaderMap::new(),
            body: Bytes::from(document.map(|d| d.to_string()).unwrap_or_default()),
        })
    }

    async fn send(&self, request: Request<Bytes>) -> aauth_protocol::Result<Fetched> {
        self.sent.lock().unwrap().push(request);
        Ok(self.answers.lock().unwrap().remove(0))
    }
}

struct Case {
    ps_key: SigningKey,
    as_key: SigningKey,
    agent: SigningKey,
    fetch: Arc<Scripted>,
    tokens: AuthTokenRequest,
    rt: serde_json::Map<String, Value>,
    presented_exp: i64,
}

impl Case {
    async fn new() -> Self {
        Self::costing(None).await
    }

    /// A case whose resource token costs `budget`, which the Person Server allows in full.
    async fn costing(budget: Option<Budget>) -> Self {
        let (ps_key, as_key, agent, resource_key) = (
            SigningKey::generate(),
            SigningKey::generate(),
            SigningKey::generate(),
            SigningKey::generate(),
        );
        let presented_exp = NOW + 1_800;
        let person_token = mint(
            &ps_key,
            TokenKind::Person,
            &json!({
                "iss": PS, "dwk": "aauth-person.json", "jti": "pt-1", "aud": RESOURCE,
                "sub": "directed-sam", "mission_s256": "m-1", "tenant": "acme",
                "iat": NOW, "exp": presented_exp, "cnf": { "jwk": agent.public_jwk() },
            }),
        );
        let presented = verified_person_token(&person_token, &ps_key, &agent).await;
        let mut params =
            ResourceTokenParams::new(RESOURCE, &presented, agent.thumbprint(), "read write")
                .access_server(AS);
        if let Some(budget) = &budget {
            params = params.budget(budget);
        }
        let resource_token =
            resource_token(&resource_key, &ResourceTokenParams { now: NOW, ..params }).unwrap();
        let rt = jwt::decode(&resource_token).unwrap().claims;
        Self {
            fetch: Scripted::with_metadata(&as_key),
            tokens: AuthTokenRequest::new(resource_token, "the.agent.token", person_token)
                .with_budget(budget),
            ps_key,
            as_key,
            agent,
            rt,
            presented_exp,
        }
    }

    /// An auth token as the Access Server would issue it, with `changes` applied.
    fn auth_token(&self, changes: Value) -> String {
        let mut claims = json!({
            "iss": AS, "dwk": "aauth-access.json", "jti": "at-1", "aud": RESOURCE,
            "ps": PS, "sub": "directed-sam", "mission_s256": "m-1", "scope": "read",
            "iat": NOW, "exp": NOW + 600, "cnf": { "jwk": self.agent.public_jwk() },
        });
        for (name, value) in changes.as_object().unwrap() {
            claims[name] = value.clone();
        }
        mint(&self.as_key, TokenKind::Auth, &claims)
    }

    async fn federate(&self, location: Option<&str>) -> Result<AccessOutcome, FederationError> {
        federate(
            &Federate {
                access_server: AS,
                tokens: &self.tokens,
                agent_jkt: self.agent.thumbprint(),
                location,
            },
            &FederationContext {
                issuer: PS,
                key: &self.ps_key,
                fetch: self.fetch.as_ref(),
                now: NOW,
            },
        )
        .await
    }
}

/// A person token, verified as the resource that challenged it did.
async fn verified_person_token(
    token: &str,
    ps_key: &SigningKey,
    agent: &SigningKey,
) -> VerifiedToken {
    let fetch = StaticFetch::new();
    fetch.insert(
        format!("{PS}/.well-known/aauth-person.json"),
        json!({ "issuer": PS, "jwks": ps_key.jwks() }),
    );
    verify_token(
        &VerifyTokenOptions {
            now: NOW,
            ..VerifyTokenOptions::new(token, agent.thumbprint(), RESOURCE, &[TokenKind::Person])
        },
        &JwksResolver::new(Arc::new(fetch)),
    )
    .await
    .unwrap()
}

#[test]
fn access_server_metadata_reads_the_drafts_example() {
    // #access-server-metadata, verbatim.
    let metadata: AccessServerMetadata = serde_json::from_value(json!({
        "issuer": "https://as.resource.example",
        "name": "Example Access Server",
        "description": "**Example Access Server** — issues access for the Example resource.",
        "logo_uri": "https://as.resource.example/logo.png",
        "auth_token_endpoint": "https://as.resource.example/token",
        "jwks_uri": "https://as.resource.example/.well-known/jwks.json"
    }))
    .unwrap();
    assert_eq!(
        metadata.auth_token_endpoint,
        "https://as.resource.example/token"
    );
    assert_eq!(metadata.revocation_endpoint, None);
    assert!(serde_json::from_value::<AccessServerMetadata>(json!({ "issuer": AS })).is_err());
}

#[tokio::test]
async fn metadata_must_name_the_access_server_and_keep_its_endpoints_on_its_origin() {
    let as_key = SigningKey::generate();
    let fetch = Scripted::with_metadata(&as_key);
    assert_eq!(
        access_server_metadata(AS, fetch.as_ref())
            .await
            .unwrap()
            .auth_token_endpoint,
        format!("{AS}/token")
    );
    // #metadata-documents: issuer must match where the document was fetched.
    fetch.document(
        format!("{AS}/.well-known/aauth-access.json"),
        json!({ "issuer": "https://as.other.example", "auth_token_endpoint": format!("{AS}/token"), "jwks_uri": format!("{AS}/jwks") }),
    );
    let refused = access_server_metadata(AS, fetch.as_ref())
        .await
        .unwrap_err();
    assert_eq!(refused.error(), "as_unreachable");
    // #access-server-metadata: its URLs are https, on whichever host.
    fetch.document(
        format!("{AS}/.well-known/aauth-access.json"),
        json!({ "issuer": AS, "auth_token_endpoint": "https://token.resource.example/token", "jwks_uri": format!("{AS}/jwks") }),
    );
    assert!(access_server_metadata(AS, fetch.as_ref()).await.is_ok());
    fetch.document(
        format!("{AS}/.well-known/aauth-access.json"),
        json!({ "issuer": AS, "auth_token_endpoint": "http://as.resource.example/token", "jwks_uri": format!("{AS}/jwks") }),
    );
    let refused = access_server_metadata(AS, fetch.as_ref())
        .await
        .unwrap_err();
    assert_eq!(refused.status(), StatusCode::BAD_GATEWAY);
}

#[tokio::test]
async fn the_token_request_is_a_signed_post_of_the_three_tokens() {
    let case = Case::new().await;
    case.fetch.answer(
        StatusCode::OK,
        &[],
        json!({ "auth_token": case.auth_token(json!({})), "expires_in": 600 }),
    );
    case.federate(None).await.unwrap();
    let [request] = case.fetch.sent().try_into().unwrap();
    assert_eq!(request.method(), Method::POST);
    assert_eq!(request.uri().to_string(), format!("{AS}/token"));
    // #ps-to-as-token-request: resource_token, agent_token, presented_token.
    let body: Value = serde_json::from_slice(request.body()).unwrap();
    assert_eq!(
        body,
        json!({
            "resource_token": case.tokens.resource_token,
            "agent_token": "the.agent.token",
            "presented_token": case.tokens.presented_token,
        })
    );
    // #keying-material: sig=jwks_uri, id the PS issuer, dwk aauth-person.json.
    let documents = StaticFetch::new();
    documents.insert(
        format!("{PS}/.well-known/aauth-person.json"),
        json!({ "issuer": PS, "jwks": case.ps_key.jwks() }),
    );
    let verified = httpsig::verify(
        &Incoming {
            method: "POST",
            authority: "as.resource.example",
            path: "/token",
            query: None,
            headers: request.headers(),
            body: Some(request.body()),
        },
        &VerifyOptions {
            now: NOW,
            require_content_digest: true,
            ..VerifyOptions::default()
        },
        &JwksResolver::new(Arc::new(documents)),
    )
    .await
    .unwrap();
    match verified.signer {
        Signer::JwksUri { id, dwk, kid } => {
            assert_eq!(id, PS);
            assert_eq!(dwk, "aauth-person.json");
            assert_eq!(kid, case.ps_key.thumbprint());
        }
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn an_auth_token_that_verifies_is_relayed_unchanged() {
    let case = Case::new().await;
    let token = case.auth_token(json!({}));
    case.fetch.answer(
        StatusCode::OK,
        &[],
        json!({ "auth_token": token, "expires_in": 9_999 }),
    );
    match case.federate(None).await.unwrap() {
        AccessOutcome::Issued {
            token: issued,
            budget,
        } => {
            assert_eq!(issued.auth_token, token);
            // From the token's own exp, not what the Access Server said.
            assert_eq!(issued.expires_in, 600);
            assert_eq!(budget, None);
        }
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn an_auth_token_failing_any_delivery_check_is_as_unreachable() {
    // #auth-token-delivery, steps 1-7, and dwk, ps and mission_s256.
    let wrong_key = SigningKey::generate();
    for (changes, what) in [
        (json!({ "iss": "https://as.other.example" }), "iss"),
        (json!({ "aud": "https://other.example" }), "aud"),
        (json!({ "cnf": { "jwk": wrong_key.public_jwk() } }), "cnf"),
        (json!({ "sub": "someone-else" }), "sub"),
        (json!({ "scope": "read admin" }), "scope"),
        (json!({ "exp": NOW + 1_801 }), "exp"),
        (json!({ "dwk": "aauth-person.json" }), "dwk"),
        (json!({ "ps": "https://ps.other.example" }), "ps"),
        (json!({ "mission_s256": "m-2" }), "mission_s256"),
    ] {
        let case = Case::new().await;
        case.fetch.answer(
            StatusCode::OK,
            &[],
            json!({ "auth_token": case.auth_token(changes) }),
        );
        let refused = case.federate(None).await.unwrap_err();
        assert_eq!(refused.error(), "as_unreachable", "{what}");
        assert_eq!(refused.status(), StatusCode::BAD_GATEWAY);
        assert!(
            refused.detail().contains(what),
            "{what}: {}",
            refused.detail()
        );
    }
    // 1. Signed by a key that is not the Access Server's.
    let case = Case::new().await;
    let forged = mint(
        &wrong_key,
        TokenKind::Auth,
        &Value::Object(jwt::decode(&case.auth_token(json!({}))).unwrap().claims),
    );
    case.fetch
        .answer(StatusCode::OK, &[], json!({ "auth_token": forged }));
    assert_eq!(
        case.federate(None).await.unwrap_err().error(),
        "as_unreachable"
    );
}

#[tokio::test]
async fn a_refusal_carries_the_access_servers_error_and_status() {
    let case = Case::new().await;
    case.fetch.answer(
        StatusCode::FORBIDDEN,
        &[],
        json!({ "error": "denied", "detail": "no account" }),
    );
    let refused = case.federate(None).await.unwrap_err();
    assert_eq!(refused.status(), StatusCode::FORBIDDEN);
    assert_eq!(refused.error(), "denied");
    assert!(refused.detail().contains("no account"));

    case.fetch.answer(
        StatusCode::BAD_REQUEST,
        &[],
        json!({ "error": "invalid_resource_token" }),
    );
    let refused = case.federate(None).await.unwrap_err();
    assert_eq!(
        (refused.status(), refused.error()),
        (StatusCode::BAD_REQUEST, "invalid_resource_token")
    );

    // A malformed or failed answer is no answer.
    for status in [StatusCode::BAD_REQUEST, StatusCode::INTERNAL_SERVER_ERROR] {
        case.fetch.answer(status, &[], json!("not a problem"));
        assert_eq!(
            case.federate(None).await.unwrap_err().error(),
            "as_unreachable"
        );
    }
    // 402 is not served yet (#as-token-endpoint).
    case.fetch.answer(
        StatusCode::PAYMENT_REQUIRED,
        &[("location", "/pending/1")],
        json!({}),
    );
    let refused = case.federate(None).await.unwrap_err();
    assert_eq!(
        (refused.status(), refused.error()),
        (StatusCode::FORBIDDEN, "denied")
    );
}

#[tokio::test]
async fn a_pending_answer_is_followed_by_polling_its_location() {
    let case = Case::new().await;
    // #approval-pending: Location and Retry-After.
    case.fetch.answer(
        StatusCode::ACCEPTED,
        &[
            ("location", "/pending/xyz"),
            ("retry-after", "30"),
            ("aauth-requirement", "requirement=approval"),
        ],
        json!({ "status": "pending" }),
    );
    let outcome = case.federate(None).await.unwrap();
    let location = format!("{AS}/pending/xyz");
    assert_eq!(
        outcome,
        AccessOutcome::Pending {
            location: location.clone(),
            requirement: Requirement::Approval,
            retry_after: Some(30),
        }
    );
    // It survives a durable step's journal.
    let journaled: AccessOutcome =
        serde_json::from_value(serde_json::to_value(&outcome).unwrap()).unwrap();
    assert_eq!(journaled, outcome);
    case.fetch.sent();

    case.fetch.answer(
        StatusCode::ACCEPTED,
        &[("location", "/pending/xyz"), ("aauth-requirement", r#"requirement=interaction;url="https://as.resource.example/bind";code="ABCD-1234""#)],
        json!({ "status": "pending" }),
    );
    match case.federate(Some(&location)).await.unwrap() {
        AccessOutcome::Pending { requirement, .. } => assert_eq!(
            requirement,
            Requirement::Interaction {
                url: Some(format!("{AS}/bind")),
                code: "ABCD-1234".into(),
            }
        ),
        other => panic!("{other:?}"),
    }
    // #deferred-responses: a poll is a GET of Location, without the body.
    let [poll] = case.fetch.sent().try_into().unwrap();
    assert_eq!(poll.method(), Method::GET);
    assert_eq!(poll.uri().to_string(), location);
    assert!(poll.body().is_empty());
}

#[tokio::test]
async fn a_pending_answer_off_its_origin_or_asking_for_claims_is_not_followed() {
    let case = Case::new().await;
    // #deferred-responses: Location MUST be on the responding server's origin.
    case.fetch.answer(
        StatusCode::ACCEPTED,
        &[("location", "https://internal.example/pending/1")],
        json!({}),
    );
    assert_eq!(
        case.federate(None).await.unwrap_err().error(),
        "as_unreachable"
    );
    // #requirement-claims and #requirement-clarification are not served yet.
    for requirement in ["requirement=claims", "requirement=clarification"] {
        case.fetch.answer(
            StatusCode::ACCEPTED,
            &[
                ("location", "/pending/1"),
                ("aauth-requirement", requirement),
            ],
            json!({}),
        );
        let refused = case.federate(None).await.unwrap_err();
        assert_eq!(
            (refused.status(), refused.error()),
            (StatusCode::FORBIDDEN, "denied")
        );
    }
    // Nor is a poll sent but over https.
    let refused = case
        .federate(Some("http://as.resource.example/pending/1"))
        .await
        .unwrap_err();
    assert_eq!(refused.error(), "as_unreachable");
}

#[tokio::test]
async fn an_expired_presented_token_is_never_sent() {
    let case = Case::new().await;
    // #ps-to-as-token-request: a PS MUST NOT present an expired token.
    let refused = federate(
        &Federate {
            access_server: AS,
            tokens: &case.tokens,
            agent_jkt: case.agent.thumbprint(),
            location: None,
        },
        &FederationContext {
            issuer: PS,
            key: &case.ps_key,
            fetch: case.fetch.as_ref(),
            now: case.presented_exp,
        },
    )
    .await
    .unwrap_err();
    assert_eq!(
        (refused.status(), refused.error()),
        (StatusCode::BAD_REQUEST, "expired_presented_token")
    );
    assert!(case.fetch.sent().is_empty());
}

#[tokio::test]
async fn a_resource_with_an_access_server_addresses_its_resource_tokens_there() {
    let case = Case::new().await;
    // #resource-token: aud is the AS in four-party; ps stays the person's PS.
    assert_eq!(case.rt["aud"], AS);
    assert_eq!(case.rt["ps"], PS);
    assert_eq!(case.rt["presented_jti"], "pt-1");
    assert_eq!(case.rt["mission_s256"], "m-1");
    assert_eq!(case.rt["tenant"], "acme");
}

#[tokio::test]
async fn a_resource_accepts_only_its_access_servers_auth_tokens_once_it_has_one() {
    let case = Case::new().await;
    let fetch = StaticFetch::new();
    fetch.insert(
        format!("{AS}/.well-known/aauth-access.json"),
        json!({ "issuer": AS, "jwks": case.as_key.jwks() }),
    );
    fetch.insert(
        format!("{PS}/.well-known/aauth-person.json"),
        json!({ "issuer": PS, "jwks": case.ps_key.jwks() }),
    );
    let resolver = JwksResolver::new(Arc::new(fetch));
    let verify = |token: String| {
        let resolver = &resolver;
        let agent = &case.agent;
        async move {
            verify_token(
                &VerifyTokenOptions {
                    now: NOW,
                    ..VerifyTokenOptions::new(
                        &token,
                        agent.thumbprint(),
                        RESOURCE,
                        &[TokenKind::Auth],
                    )
                },
                resolver,
            )
            .await
            .unwrap()
        }
    };
    let from_as = verify(case.auth_token(json!({}))).await;
    let mut claims = jwt::decode(&case.auth_token(json!({}))).unwrap().claims;
    claims.insert("iss".into(), json!(PS));
    claims.insert("dwk".into(), json!("aauth-person.json"));
    let from_ps = verify(mint(&case.ps_key, TokenKind::Auth, &Value::Object(claims))).await;

    let person_servers = [PS.to_string()];
    let four_party = AuthTokenIssuers::AccessServer {
        access_server: AS,
        person_servers: &person_servers,
    };
    assert!(four_party.check(&from_as).is_ok());
    assert_eq!(four_party.check(&from_ps).unwrap_err().code, "invalid_jwt");
    let other = ["https://ps.other.example".to_string()];
    let elsewhere = AuthTokenIssuers::AccessServer {
        access_server: AS,
        person_servers: &other,
    };
    assert!(elsewhere.check(&from_as).is_err());
    let three_party = AuthTokenIssuers::PersonServers(&person_servers);
    assert!(three_party.check(&from_ps).is_ok());
    assert!(three_party.check(&from_as).is_err());
}

fn usd(amount: u64) -> Budget {
    Budget::new(amount, "USD", 2)
}

#[tokio::test]
async fn the_budget_sent_bounds_the_budget_granted() {
    // Budgets draft #as-token-endpoint: the PS sends the ceiling it allows.
    let case = Case::costing(Some(usd(5_000))).await;
    case.fetch.answer(
        StatusCode::OK,
        &[],
        json!({ "auth_token": case.auth_token(json!({ "budget": usd(2_000) })) }),
    );
    let outcome = case.federate(None).await.unwrap();
    let [sent] = case.fetch.sent().try_into().unwrap();
    let body: Value = serde_json::from_slice(sent.body()).unwrap();
    assert_eq!(body["budget"], json!(usd(5_000)));
    // The AS MAY lower it; what it grants is what is spent.
    match outcome {
        AccessOutcome::Issued { budget, .. } => assert_eq!(budget, Some(usd(2_000))),
        other => panic!("{other:?}"),
    }
    // Higher, in another scale, or absent though sent: a token that fails delivery.
    for granted in [
        json!(usd(5_001)),
        json!(Budget::new(2_000, "USD", 6)),
        Value::Null,
    ] {
        case.fetch.answer(
            StatusCode::OK,
            &[],
            json!({ "auth_token": case.auth_token(json!({ "budget": granted })) }),
        );
        let refused = case.federate(None).await.unwrap_err();
        assert_eq!(refused.error(), "as_unreachable");
        assert!(refused.detail().contains("budget"), "{}", refused.detail());
    }
    // None sent, none may be granted.
    let unbudgeted = Case::new().await;
    unbudgeted.fetch.answer(
        StatusCode::OK,
        &[],
        json!({ "auth_token": unbudgeted.auth_token(json!({ "budget": usd(1) })) }),
    );
    assert!(unbudgeted
        .federate(None)
        .await
        .unwrap_err()
        .detail()
        .contains("budget"));
}

#[tokio::test]
async fn an_auth_token_living_over_an_hour_is_refused() {
    // #auth-token-structure: no more than an hour.
    let case = Case::new().await;
    case.fetch.answer(
        StatusCode::OK,
        &[],
        json!({ "auth_token": case.auth_token(json!({ "iat": NOW - 3_000, "exp": NOW + 601 })) }),
    );
    assert!(case
        .federate(None)
        .await
        .unwrap_err()
        .detail()
        .contains("exp"));
}

#[tokio::test]
async fn a_busy_access_server_is_polled_again_later() {
    // #deferred-responses: 503 backs off per Retry-After; 429 slows down by 5s.
    let case = Case::new().await;
    case.fetch.answer(
        StatusCode::SERVICE_UNAVAILABLE,
        &[("retry-after", "7")],
        json!({}),
    );
    assert_eq!(
        case.federate(Some(&format!("{AS}/pending/1")))
            .await
            .unwrap(),
        AccessOutcome::Retry { retry_after: 7 }
    );
    case.fetch.answer(
        StatusCode::TOO_MANY_REQUESTS,
        &[],
        json!({ "error": "slow_down" }),
    );
    assert_eq!(
        case.federate(Some(&format!("{AS}/pending/1")))
            .await
            .unwrap(),
        AccessOutcome::Retry { retry_after: 10 }
    );
}

#[tokio::test]
async fn an_unrecognised_requirement_is_polled_through_as_approval() {
    // #requirement-values: the agent MAY keep polling an unrecognised 202.
    let case = Case::new().await;
    case.fetch.answer(
        StatusCode::ACCEPTED,
        &[
            ("location", "/pending/1"),
            ("aauth-requirement", "requirement=payment-review"),
        ],
        json!({}),
    );
    match case.federate(None).await.unwrap() {
        AccessOutcome::Pending { requirement, .. } => {
            assert_eq!(requirement, Requirement::Approval)
        }
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn a_pending_location_must_be_on_the_origin_that_answered() {
    // #deferred-responses: the origin of the responding server, the endpoint's.
    let case = Case::new().await;
    case.fetch.document(
        format!("{AS}/.well-known/aauth-access.json"),
        json!({ "issuer": AS, "auth_token_endpoint": "https://token.resource.example/token", "jwks_uri": format!("{AS}/.well-known/jwks.json") }),
    );
    case.fetch.answer(
        StatusCode::ACCEPTED,
        &[("location", "/pending/1")],
        json!({}),
    );
    match case.federate(None).await.unwrap() {
        AccessOutcome::Pending { location, .. } => {
            assert_eq!(location, "https://token.resource.example/pending/1")
        }
        other => panic!("{other:?}"),
    }
    case.fetch.answer(
        StatusCode::ACCEPTED,
        &[("location", &format!("{AS}/pending/1"))],
        json!({}),
    );
    assert_eq!(
        case.federate(None).await.unwrap_err().error(),
        "as_unreachable"
    );
}

#[tokio::test]
async fn tokens_this_crate_cannot_verify_are_never_passed_on() {
    // #ps-to-as-token-request: the AS MUST verify upstream_token; nothing here does.
    let mut case = Case::new().await;
    case.tokens.upstream_token = Some("an.upstream.token".into());
    let refused = case.federate(None).await.unwrap_err();
    assert_eq!(
        (refused.status(), refused.error()),
        (StatusCode::BAD_REQUEST, "invalid_request")
    );
    case.tokens.upstream_token = None;
    case.tokens.subagent_token = Some("a.subagent.token".into());
    assert_eq!(
        case.federate(None).await.unwrap_err().error(),
        "invalid_request"
    );
    assert!(case.fetch.sent().is_empty());
}
