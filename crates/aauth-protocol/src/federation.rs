//! Four-party access (AAuth §Access Server Federation,
//! `#access-server-federation`): a resource token whose `aud` is the
//! resource's own Access Server is redeemed there by the person's Person
//! Server.
//!
//! This module is the Person Server's side of the wire, and the message
//! shapes both sides share. [`federate`] makes one exchange: the signed
//! `POST` to the Access Server's `auth_token_endpoint`
//! (`#ps-to-as-token-request`), or, once it has answered `202`, one signed
//! `GET` of its `Location` (`#deferred-responses`). It does no I/O but
//! through the [`Fetch`] it is handed and reads no clock but `now`, so a
//! durable workflow can run each call as one step and poll between them.
//!
//! The caller decides first: a Person Server weighs the resource token
//! against the person's mission and budget before it federates, and calls
//! only the Access Server a resource token it has verified names.

use bytes::Bytes;
use http::{HeaderMap, Method, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use url::Url;

use crate::discovery::{Fetch, Fetched};
use crate::httpsig::{self, Outgoing, SignatureKey};
use crate::identifier::{is_server_identifier, same_origin};
use crate::jwk::{Algorithm, Jwks};
use crate::jwt;
use crate::keys::SigningKey;
use crate::metadata::{dwk, metadata_url, AccessServerMetadata};
use crate::requirement::{self, Requirement};
use crate::token::TokenKind;
use crate::Budget;

/// The body of a Person Server's token request to an Access Server
/// (`#ps-to-as-token-request`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthTokenRequest {
    /// The resource token the agent brought, `aud` the Access Server.
    pub resource_token: String,
    /// The agent token that signed the agent's request to the Person Server.
    pub agent_token: String,
    /// The token the resource saw, which the resource token's
    /// `presented_jti` names.
    pub presented_token: String,
    /// A sub-agent's agent token, passed through for a parent-mediated
    /// authorization (`#sub-agents`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subagent_token: Option<String>,
    /// The agent's `upstream_token`, passed through for call chaining
    /// (`#call-chaining`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upstream_token: Option<String>,
    /// The most the Person Server will allow ([draft-hardt-aauth-budgets]
    /// `#as-token-endpoint`): the resource token's `budget`, in its unit and
    /// scale, with an amount no higher. The Access Server may lower it, never
    /// raise it; omitted, it issues no `budget` claim.
    ///
    /// [draft-hardt-aauth-budgets]: https://github.com/dickhardt/AAuth
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget: Option<Budget>,
}

impl AuthTokenRequest {
    /// The three required tokens, and neither optional one.
    pub fn new(
        resource_token: impl Into<String>,
        agent_token: impl Into<String>,
        presented_token: impl Into<String>,
    ) -> Self {
        Self {
            resource_token: resource_token.into(),
            agent_token: agent_token.into(),
            presented_token: presented_token.into(),
            subagent_token: None,
            upstream_token: None,
            budget: None,
        }
    }

    /// Allow at most `budget`.
    pub fn with_budget(mut self, budget: Option<Budget>) -> Self {
        self.budget = budget;
        self
    }
}

/// A token endpoint's direct grant: `200 {auth_token, expires_in}`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthTokenResponse {
    /// The auth token.
    pub auth_token: String,
    /// Seconds until it expires.
    pub expires_in: i64,
}

/// One exchange with an Access Server.
#[derive(Clone, Copy, Debug)]
pub struct Federate<'a> {
    /// The Access Server: the `aud` of a resource token the caller verified.
    pub access_server: &'a str,
    /// What is sent to it.
    pub tokens: &'a AuthTokenRequest,
    /// The JWK thumbprint of the agent's key, which the auth token must bind.
    pub agent_jkt: &'a str,
    /// Where the Access Server said to poll, once it answered `202`. Set, the
    /// exchange is a `GET` of it; unset, the `POST` to its
    /// `auth_token_endpoint`.
    pub location: Option<&'a str>,
}

/// Who federates: the Person Server, signing as itself.
#[derive(Clone, Copy)]
pub struct FederationContext<'a> {
    /// The Person Server's identifier: the `Signature-Key` `id`, and the
    /// `ps` an auth token must name.
    pub issuer: &'a str,
    /// Its signing key, published in its `aauth-person.json` JWKS.
    pub key: &'a SigningKey,
    /// How the Access Server is reached. Give it an SSRF guard, as
    /// `ReqwestFetch::guarded` is.
    pub fetch: &'a dyn Fetch,
    /// Seconds since the epoch.
    pub now: i64,
}

/// What the Access Server answered, when it answered something the Person
/// Server acts on.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
#[non_exhaustive]
pub enum AccessOutcome {
    /// It issued an auth token, which verified (`#auth-token-delivery`):
    /// relay `token` to the agent unchanged.
    Issued {
        /// What the agent is answered with.
        token: AuthTokenResponse,
        /// The budget it granted, from the token's `budget` claim: what to
        /// spend. Present exactly when the request sent one.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        budget: Option<Budget>,
    },
    /// It is deciding: poll `location` after `retry_after` seconds, and tell
    /// the agent `requirement` meanwhile.
    Pending {
        /// The pending URL, absolute, on the Access Server's own origin.
        location: String,
        /// [`Requirement::Approval`] or [`Requirement::Interaction`]; the
        /// others are not served yet (see [`FederationError::Refused`]).
        requirement: Requirement,
        /// Its `Retry-After`, in seconds, when it sent a readable one.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        retry_after: Option<u64>,
    },
    /// It asked to be called again later (`429` or `503`,
    /// `#deferred-responses`): repeat the same exchange after `retry_after`
    /// seconds. Nothing about the request has changed.
    Retry {
        /// Seconds to wait.
        retry_after: u64,
    },
}

/// Why no auth token came back.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[non_exhaustive]
pub enum FederationError {
    /// A terminal answer the agent is told as is (`#auth-token-delivery`):
    /// the Access Server's own error and status, or a refusal this Person
    /// Server makes, such as an expired presented token or a requirement it
    /// does not serve.
    #[error("{error}: {detail}")]
    Refused {
        /// The HTTP status to answer the agent with.
        status: u16,
        /// The error code.
        error: String,
        /// A human-readable explanation.
        detail: String,
    },
    /// No verifiable answer: the Access Server could not be reached, answered
    /// with something malformed, or issued a token that does not verify.
    /// The agent is answered `502 as_unreachable`.
    #[error("as_unreachable: {detail}")]
    Unreachable {
        /// What went wrong, for logs.
        detail: String,
    },
}

impl FederationError {
    fn refused(status: StatusCode, error: &str, detail: impl Into<String>) -> Self {
        Self::Refused {
            status: status.as_u16(),
            error: error.to_string(),
            detail: detail.into(),
        }
    }

    fn unreachable(detail: impl Into<String>) -> Self {
        Self::Unreachable {
            detail: detail.into(),
        }
    }

    /// The status the agent is answered with.
    pub fn status(&self) -> StatusCode {
        match self {
            Self::Refused { status, .. } => {
                StatusCode::from_u16(*status).unwrap_or(StatusCode::BAD_GATEWAY)
            }
            Self::Unreachable { .. } => StatusCode::BAD_GATEWAY,
        }
    }

    /// The error code the agent is answered with.
    pub fn error(&self) -> &str {
        match self {
            Self::Refused { error, .. } => error,
            Self::Unreachable { .. } => "as_unreachable",
        }
    }

    /// The explanation the agent is answered with.
    pub fn detail(&self) -> &str {
        match self {
            Self::Refused { detail, .. } | Self::Unreachable { detail } => detail,
        }
    }
}

/// `{access_server}/.well-known/aauth-access.json` (`#ps-as-federation`),
/// which must name `access_server` as its `issuer` (`#metadata-documents`)
/// and give `https` URLs for its `auth_token_endpoint` and `jwks_uri`.
pub async fn access_server_metadata(
    access_server: &str,
    fetch: &dyn Fetch,
) -> Result<AccessServerMetadata, FederationError> {
    if !is_server_identifier(access_server) {
        return Err(FederationError::unreachable(format!(
            "{access_server} is not a server identifier"
        )));
    }
    let url = metadata_url(access_server, dwk::ACCESS);
    let fetched = fetch
        .get(&url, HeaderMap::new())
        .await
        .map_err(|e| FederationError::unreachable(format!("{url}: {e}")))?;
    if !fetched.status.is_success() {
        return Err(FederationError::unreachable(format!(
            "{url} answered {}",
            fetched.status
        )));
    }
    let metadata: AccessServerMetadata = serde_json::from_slice(&fetched.body)
        .map_err(|e| FederationError::unreachable(format!("{url} is unreadable: {e}")))?;
    if metadata.issuer != access_server {
        return Err(FederationError::unreachable(format!(
            "{url} names another issuer"
        )));
    }
    if [&metadata.auth_token_endpoint, &metadata.jwks_uri]
        .iter()
        .any(|endpoint| Url::parse(endpoint).map_or(true, |u| u.scheme() != "https"))
    {
        return Err(FederationError::unreachable(format!(
            "{url} gives an endpoint that is not https"
        )));
    }
    Ok(metadata)
}

/// One exchange with the Access Server: the token request, or one poll of
/// where it said to wait. A refusal is [`FederationError::Refused`] with the
/// Access Server's own error and status; anything it answers that cannot
/// be verified is [`FederationError::Unreachable`].
///
/// - `requirement=claims` and `clarification`, and `402` payment, are not
///   served yet: each is refused `403 denied`, naming what the Access Server
///   asked for. Any other `202` requirement this crate does not know is
///   relayed as `approval` and polled through (`#requirement-values`).
/// - `subagent_token` and `upstream_token` are refused `400
///   invalid_request`: this crate cannot verify them, and the Access Server
///   would act on them (`#ps-to-as-token-request`).
pub async fn federate(
    request: &Federate<'_>,
    ctx: &FederationContext<'_>,
) -> Result<AccessOutcome, FederationError> {
    let tokens = request.tokens;
    if tokens.subagent_token.is_some() || tokens.upstream_token.is_some() {
        return Err(FederationError::refused(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "subagent_token and upstream_token are not verified here, so they are not sent",
        ));
    }
    let presented = jwt::decode(&tokens.presented_token).map_err(|_| {
        FederationError::refused(
            StatusCode::BAD_REQUEST,
            "invalid_presented_token",
            "not a JWT",
        )
    })?;
    let presented_exp = presented
        .claims
        .get("exp")
        .and_then(Value::as_i64)
        .unwrap_or(i64::MIN);
    // #ps-to-as-token-request: a Person Server MUST NOT present an expired token.
    if ctx.now >= presented_exp {
        return Err(FederationError::refused(
            StatusCode::BAD_REQUEST,
            "expired_presented_token",
            "the presented token has expired",
        ));
    }
    let resource_token = jwt::decode(&tokens.resource_token)
        .map_err(|_| {
            FederationError::refused(
                StatusCode::BAD_REQUEST,
                "invalid_resource_token",
                "not a JWT",
            )
        })?
        .claims;
    // Budgets #as-token-endpoint: the resource token's unit and scale, and no more.
    if let Some(allowed) = &tokens.budget {
        let offered = Budget::from_value(resource_token.get("budget"))
            .ok()
            .flatten();
        if !offered.is_some_and(|o| o.same_scale(allowed) && allowed.amount <= o.amount) {
            return Err(FederationError::refused(
                StatusCode::INTERNAL_SERVER_ERROR,
                "server_error",
                "the budget sent must be the resource token's, no higher",
            ));
        }
    }

    let metadata = access_server_metadata(request.access_server, ctx.fetch).await?;
    let (url, body) = match request.location {
        Some(location) => (location.to_string(), None),
        None => (
            metadata.auth_token_endpoint.clone(),
            Some(
                serde_json::to_vec(tokens)
                    .map_err(|e| FederationError::unreachable(e.to_string()))?,
            ),
        ),
    };
    if !url.starts_with("https://") {
        return Err(FederationError::unreachable(format!("{url} is not https")));
    }
    let response = send(ctx, &url, body).await?;
    let header = |name: &str| {
        response
            .headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string)
    };
    match response.status {
        StatusCode::OK => {
            let auth_token = serde_json::from_slice::<Value>(&response.body)
                .ok()
                .and_then(|v| {
                    v.get("auth_token")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                })
                .ok_or_else(|| {
                    FederationError::unreachable("it answered 200 without an auth_token")
                })?;
            let delivered = verify_delivered(
                &auth_token,
                &resource_token,
                presented_exp,
                request.agent_jkt,
                tokens.budget.as_ref(),
                &metadata,
                ctx,
            )
            .await?;
            Ok(AccessOutcome::Issued {
                token: AuthTokenResponse {
                    auth_token,
                    expires_in: delivered.exp - ctx.now,
                },
                budget: delivered.budget,
            })
        }
        StatusCode::ACCEPTED => pending(&response, &url),
        // #deferred-responses: slow down, or back off per Retry-After, and keep polling.
        StatusCode::TOO_MANY_REQUESTS | StatusCode::SERVICE_UNAVAILABLE => {
            let asked = header("retry-after").and_then(|v| v.trim().parse::<u64>().ok());
            let retry_after = match (response.status, asked) {
                (_, Some(seconds)) => seconds,
                (StatusCode::TOO_MANY_REQUESTS, None) => DEFAULT_POLL_SECS + 5,
                _ => DEFAULT_POLL_SECS,
            };
            Ok(AccessOutcome::Retry { retry_after })
        }
        StatusCode::PAYMENT_REQUIRED => Err(FederationError::refused(
            StatusCode::FORBIDDEN,
            "denied",
            "the Access Server asks for payment, which this Person Server does not make",
        )),
        status if status.is_client_error() => {
            let body: Value = serde_json::from_slice(&response.body).unwrap_or(Value::Null);
            let Some(error) = body.get("error").and_then(Value::as_str) else {
                return Err(FederationError::unreachable(format!(
                    "it answered {status}"
                )));
            };
            let detail = body.get("detail").and_then(Value::as_str).unwrap_or(error);
            Err(FederationError::refused(
                status,
                error,
                format!("the Access Server refused: {detail}"),
            ))
        }
        status => Err(FederationError::unreachable(format!(
            "it answered {status}"
        ))),
    }
}

/// The polling interval without a `Retry-After` (`#deferred-responses`).
const DEFAULT_POLL_SECS: u64 = 5;

/// A request to `url`, signed as the Person Server (`#keying-material`):
/// `sig=jwks_uri;id=<issuer>;dwk="aauth-person.json"`.
async fn send(
    ctx: &FederationContext<'_>,
    url: &str,
    body: Option<Vec<u8>>,
) -> Result<Fetched, FederationError> {
    let method = if body.is_some() {
        Method::POST
    } else {
        Method::GET
    };
    let signed = httpsig::sign(
        ctx.key,
        &SignatureKey::JwksUri {
            id: ctx.issuer.to_string(),
            dwk: dwk::PERSON.to_string(),
            kid: ctx.key.thumbprint().to_string(),
        },
        &Outgoing {
            method: method.as_str(),
            url,
            content_type: Some("application/json"),
            body: body.as_deref(),
            created: ctx.now,
        },
    )
    .map_err(|e| FederationError::unreachable(e.to_string()))?;
    let mut builder = http::Request::builder().method(method).uri(url);
    for (name, value) in signed {
        builder = builder.header(name, value);
    }
    let request = builder
        .body(Bytes::from(body.unwrap_or_default()))
        .map_err(|e| FederationError::unreachable(e.to_string()))?;
    ctx.fetch
        .send(request)
        .await
        .map_err(|e| FederationError::unreachable(format!("{url}: {e}")))
}

/// A `202` (`#deferred-responses`): where to poll, on the origin of the
/// server that answered, and what it waits on.
fn pending(response: &Fetched, requested: &str) -> Result<AccessOutcome, FederationError> {
    let header = |name: &str| {
        response
            .headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string)
    };
    let requirement = header(requirement::HEADER).and_then(|raw| Requirement::parse(&raw));
    if let Some(requirement @ (Requirement::Claims | Requirement::Clarification)) = &requirement {
        return Err(FederationError::refused(
            StatusCode::FORBIDDEN,
            "denied",
            format!(
                "the Access Server asks for requirement={}, which this Person Server does not serve",
                requirement.name()
            ),
        ));
    }
    // #requirement-values: anything else a 202 may carry that this Person
    // Server cannot act on is polled through, and the agent told approval.
    let requirement = match requirement {
        Some(interaction @ Requirement::Interaction { .. }) => interaction,
        _ => Requirement::Approval,
    };
    let location = header("location")
        .and_then(|location| Url::parse(requested).ok()?.join(&location).ok())
        .filter(|location| same_origin(requested, location.as_str()))
        .ok_or_else(|| {
            FederationError::unreachable("its pending Location is not on its own origin")
        })?;
    Ok(AccessOutcome::Pending {
        location: location.to_string(),
        requirement,
        retry_after: header("retry-after").and_then(|v| v.trim().parse().ok()),
    })
}

fn scopes(value: Option<&Value>) -> Vec<&str> {
    value
        .and_then(Value::as_str)
        .map(|s| s.split(' ').filter(|s| !s.is_empty()).collect())
        .unwrap_or_default()
}

/// What a delivered auth token was issued for.
struct Delivered {
    exp: i64,
    budget: Option<Budget>,
}

/// The longest an auth token may live (`#auth-token-structure`).
const MAX_AUTH_TOKEN_SECS: i64 = 60 * 60;

/// The checks of `#auth-token-delivery` on the auth token the Access Server
/// issued, before the agent gets it, and the budgets draft's on its `budget`.
async fn verify_delivered(
    token: &str,
    resource_token: &Map<String, Value>,
    presented_exp: i64,
    agent_jkt: &str,
    allowed: Option<&Budget>,
    metadata: &AccessServerMetadata,
    ctx: &FederationContext<'_>,
) -> Result<Delivered, FederationError> {
    let wrong =
        |what: &str| FederationError::unreachable(format!("its auth token has the wrong {what}"));
    let decoded = jwt::decode(token).map_err(|_| wrong("form"))?;
    if decoded.typ() != Some(TokenKind::Auth.typ()) {
        return Err(wrong("typ"));
    }
    // 1. The signature, against the Access Server's keys.
    let fetched = ctx
        .fetch
        .get(&metadata.jwks_uri, HeaderMap::new())
        .await
        .map_err(|e| FederationError::unreachable(format!("{}: {e}", metadata.jwks_uri)))?;
    let jwks: Jwks = serde_json::from_slice(&fetched.body)
        .ok()
        .filter(|_| fetched.status.is_success())
        .ok_or_else(|| {
            FederationError::unreachable(format!("{} is unreadable", metadata.jwks_uri))
        })?;
    let algorithm = decoded
        .alg()
        .and_then(Algorithm::from_name)
        .ok_or_else(|| wrong("alg"))?;
    let key = match decoded.kid() {
        Some(kid) => jwks.keys.iter().find(|k| k.kid.as_deref() == Some(kid)),
        None if jwks.keys.len() == 1 => jwks.keys.first(),
        None => None,
    }
    .ok_or_else(|| wrong("kid"))?;
    if key.alg.as_deref().is_some_and(|a| a != algorithm.as_str())
        || !decoded.verify_signature(key, algorithm)
    {
        return Err(wrong("signature"));
    }
    let claims = &decoded.claims;
    let text = |map: &Map<String, Value>, name: &str| {
        map.get(name).and_then(Value::as_str).map(str::to_string)
    };
    let number = |name: &str| claims.get(name).and_then(Value::as_i64);
    let (Some(exp), Some(iat)) = (number("exp"), number("iat")) else {
        return Err(wrong("exp, iat"));
    };
    let aud_is_resource = match (
        claims.get("aud"),
        resource_token.get("iss").and_then(Value::as_str),
    ) {
        (Some(Value::String(aud)), Some(iss)) => aud == iss,
        (Some(Value::Array(auds)), Some(iss)) => auds.iter().any(|a| a.as_str() == Some(iss)),
        _ => false,
    };
    let asked = scopes(resource_token.get("scope"));
    let cnf = claims
        .get("cnf")
        .and_then(|c| c.get("jwk"))
        .and_then(|j| serde_json::from_value::<crate::Jwk>(j.clone()).ok())
        .and_then(|j| j.thumbprint().ok());
    let granted = Budget::from_value(claims.get("budget"));
    // Budgets #as-token-endpoint: present exactly when sent, same scale, no higher.
    let budget_ok = match (allowed, &granted) {
        (None, Ok(None)) => true,
        (Some(allowed), Ok(Some(granted))) => {
            granted.same_scale(allowed) && granted.amount <= allowed.amount
        }
        _ => false,
    };
    let problems: Vec<&str> = [
        // 2. Issued by the Access Server the request went to.
        (
            text(claims, "iss").as_deref() != Some(metadata.issuer.as_str()),
            "iss",
        ),
        (text(claims, "dwk").as_deref() != Some(dwk::ACCESS), "dwk"),
        // 3. For the resource that issued the resource token.
        (!aud_is_resource, "aud"),
        // 4. Bound to the agent's key.
        (cnf.as_deref() != Some(agent_jkt), "cnf"),
        // 5. For the person the resource token names, at this Person Server.
        (
            text(claims, "sub") != text(resource_token, "sub") || text(claims, "sub").is_none(),
            "sub",
        ),
        (text(claims, "ps").as_deref() != Some(ctx.issuer), "ps"),
        (
            text(claims, "mission_s256") != text(resource_token, "mission_s256"),
            "mission_s256",
        ),
        // 6. No broader than asked.
        (
            scopes(claims.get("scope"))
                .iter()
                .any(|s| !asked.contains(s)),
            "scope",
        ),
        // 7. Not past the presented token, nor already expired, nor over an hour.
        (
            exp > presented_exp || exp <= ctx.now || exp - iat > MAX_AUTH_TOKEN_SECS,
            "exp",
        ),
        (!budget_ok, "budget"),
    ]
    .into_iter()
    .filter_map(|(failed, what)| failed.then_some(what))
    .collect();
    if !problems.is_empty() {
        return Err(wrong(&problems.join(", ")));
    }
    Ok(Delivered {
        exp,
        budget: granted.ok().flatten(),
    })
}
