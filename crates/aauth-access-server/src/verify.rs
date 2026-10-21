//! What an Access Server checks of a Person Server's token request
//! (`#ps-to-as-token-request`): the resource token is for it and from a
//! resource it decides for, the agent token is from a trusted agent provider
//! and binds the key the resource token names, and the presented token is
//! the one the resource saw, from the Person Server that sent the request
//! (step 3 of `#resource-token-verification`).

use aauth_protocol::discovery::{JwksResolver, KeyResolver};
use aauth_protocol::jwk::{Algorithm, Jwks};
use aauth_protocol::metadata::dwk;
use aauth_protocol::token::{verify_token, TokenKind, VerifiedToken, VerifyTokenOptions};
use aauth_protocol::{jwt, Error};
use async_trait::async_trait;
use serde_json::{Map, Value};

use crate::error::AAuthError;

/// Whose keys an Access Server ever looks up: its own, and those of the
/// parties it is configured to trust, each under its own role's document.
/// Anything else is refused without a fetch.
pub(crate) struct Keys {
    pub issuer: String,
    pub own: Jwks,
    pub resources: Vec<String>,
    pub person_servers: Vec<String>,
    pub agent_providers: Vec<String>,
    pub inner: JwksResolver,
}

#[async_trait]
impl KeyResolver for Keys {
    async fn jwks(&self, issuer: &str, document: &str) -> aauth_protocol::Result<Jwks> {
        if issuer == self.issuer && document == dwk::ACCESS {
            return Ok(self.own.clone());
        }
        let trusted = match document {
            dwk::RESOURCE => &self.resources,
            dwk::PERSON => &self.person_servers,
            dwk::AGENT => &self.agent_providers,
            _ => {
                return Err(Error::Discovery(format!(
                    "{document} is not looked up here"
                )))
            }
        };
        if !trusted.iter().any(|t| t == issuer) {
            return Err(Error::Discovery(format!(
                "{issuer} is not trusted here as a {document} issuer"
            )));
        }
        self.inner.jwks(issuer, document).await
    }
}

/// What the request is checked against.
pub(crate) struct Verifier<'a> {
    pub keys: &'a Keys,
    pub now: i64,
}

fn token_error(parameter: &str, detail: impl Into<String>, expired: bool) -> AAuthError {
    let state = if expired { "expired" } else { "invalid" };
    AAuthError::bad(&format!("{state}_{parameter}_token"), detail)
}

fn text<'a>(claims: &'a Map<String, Value>, name: &str) -> Option<&'a str> {
    claims
        .get(name)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
}

impl Verifier<'_> {
    /// `#resource-token-verification` at an Access Server: `ps` is the
    /// Person Server that signed the request. The resource's keys are
    /// fetched only once its `iss` is one this server decides for.
    pub async fn resource_token(
        &self,
        token: &str,
        ps: &str,
    ) -> Result<Map<String, Value>, AAuthError> {
        let invalid = |detail: &str| token_error("resource", detail, false);
        let decoded = jwt::decode(token).map_err(|_| invalid("not a JWT"))?;
        if decoded.typ() != Some(TokenKind::Resource.typ()) {
            return Err(invalid("not a resource token"));
        }
        let resource = decoded.claim_str("iss").unwrap_or_default().to_string();
        if !self.keys.resources.contains(&resource) {
            return Err(invalid(&format!(
                "this Access Server does not decide for {resource}"
            )));
        }
        if decoded.claim_str("dwk") != Some(dwk::RESOURCE) {
            return Err(invalid("dwk is not aauth-resource.json"));
        }
        let jwks = self
            .keys
            .jwks(&resource, dwk::RESOURCE)
            .await
            .map_err(|e| invalid(&e.to_string()))?;
        let algorithm = decoded
            .alg()
            .and_then(Algorithm::from_name)
            .ok_or_else(|| invalid("unsupported alg"))?;
        let key = match decoded.kid() {
            Some(kid) => jwks.keys.iter().find(|k| k.kid.as_deref() == Some(kid)),
            None if jwks.keys.len() == 1 => jwks.keys.first(),
            None => None,
        }
        .ok_or_else(|| invalid("no matching key in the resource's JWKS"))?;
        if key.alg.as_deref().is_some_and(|a| a != algorithm.as_str())
            || !decoded.verify_signature(key, algorithm)
        {
            return Err(invalid("signature verification failed"));
        }
        let rt = decoded.claims;
        let exp = rt.get("exp").and_then(Value::as_i64).unwrap_or(i64::MIN);
        if self.now >= exp {
            return Err(token_error("resource", "it has expired", true));
        }
        if rt
            .get("nbf")
            .and_then(Value::as_i64)
            .is_some_and(|nbf| self.now < nbf)
        {
            return Err(invalid("it is not yet valid"));
        }
        if rt
            .get("iat")
            .and_then(Value::as_i64)
            .is_some_and(|iat| iat > self.now + 60)
        {
            return Err(AAuthError::bad(
                "clock_skew",
                "its iat is ahead of this clock",
            ));
        }
        if text(&rt, "aud") != Some(self.keys.issuer.as_str()) {
            return Err(invalid("its aud is not this Access Server"));
        }
        if text(&rt, "ps") != Some(ps) {
            return Err(invalid("its ps is not the Person Server that sent it"));
        }
        for claim in ["sub", "presented_jti", "agent_jkt"] {
            if text(&rt, claim).is_none() {
                return Err(invalid(&format!("{claim} is required")));
            }
        }
        Ok(rt)
    }

    /// The agent's token, from a trusted agent provider, bound to the key
    /// the resource token names.
    pub async fn agent_token(
        &self,
        token: &str,
        rt: &Map<String, Value>,
        ps: &str,
    ) -> Result<VerifiedToken, AAuthError> {
        let provider = jwt::decode(token)
            .ok()
            .and_then(|d| d.claim_str("iss").map(str::to_string))
            .unwrap_or_default();
        if !self.keys.agent_providers.contains(&provider) {
            return Err(token_error(
                "agent",
                format!("{provider} is not an agent provider this Access Server trusts"),
                false,
            ));
        }
        let agent = verify_token(
            &VerifyTokenOptions {
                now: self.now,
                ..VerifyTokenOptions::new(
                    token,
                    text(rt, "agent_jkt").unwrap_or_default(),
                    &self.keys.issuer,
                    &[TokenKind::Agent],
                )
            },
            self.keys,
        )
        .await
        .map_err(|e| match e.code {
            "key_binding_failed" => {
                token_error("resource", "agent_jkt is not the agent token's key", false)
            }
            "token_expired" => token_error("agent", e.detail, true),
            _ => token_error("agent", e.detail, false),
        })?;
        if agent.ps().is_some_and(|named| named != ps) {
            return Err(token_error(
                "agent",
                "it names another Person Server",
                false,
            ));
        }
        Ok(agent)
    }

    /// The token the resource saw: a person token from the Person Server that
    /// sent the request, or an auth token this Access Server issued for its
    /// people, for the resource and the agent's key, and matching the
    /// resource token exactly.
    pub async fn presented_token(
        &self,
        token: &str,
        rt: &Map<String, Value>,
        ps: &str,
    ) -> Result<VerifiedToken, AAuthError> {
        let invalid = |detail: &str| token_error("presented", detail, false);
        let decoded = jwt::decode(token).map_err(|_| invalid("not a JWT"))?;
        let kind = match decoded.typ().and_then(TokenKind::from_typ) {
            Some(kind @ (TokenKind::Person | TokenKind::Auth)) => kind,
            _ => return Err(invalid("neither a person token nor an auth token")),
        };
        let expected_issuer = if kind == TokenKind::Person {
            ps
        } else {
            self.keys.issuer.as_str()
        };
        if decoded.claim_str("iss") != Some(expected_issuer) {
            return Err(invalid(
                "issued by neither the Person Server that sent it nor this Access Server",
            ));
        }
        let presented = verify_token(
            &VerifyTokenOptions {
                now: self.now,
                ..VerifyTokenOptions::new(
                    token,
                    text(rt, "agent_jkt").unwrap_or_default(),
                    text(rt, "iss").unwrap_or_default(),
                    &[kind],
                )
            },
            self.keys,
        )
        .await
        .map_err(|e| token_error("presented", e.detail, e.code == "token_expired"))?;
        let claims = &presented.claims;
        let person_server = if kind == TokenKind::Auth {
            presented.ps()
        } else {
            Some(presented.iss.as_str())
        };
        if text(claims, "jti") != text(rt, "presented_jti")
            || person_server != text(rt, "ps")
            || text(claims, "sub") != text(rt, "sub")
            || text(claims, "mission_s256") != text(rt, "mission_s256")
            || text(claims, "tenant") != text(rt, "tenant")
        {
            return Err(token_error(
                "resource",
                "it does not match the presented token",
                false,
            ));
        }
        Ok(presented)
    }
}
