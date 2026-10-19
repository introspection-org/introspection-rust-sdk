//! Agent, person and auth tokens: minting, and verification as AAuth
//! §Request-Context Binding prescribes. A port of `@aauth/resource`'s
//! `verifyToken`, in its order: structure first, then the key binding, then
//! the issuer's signature, and only then anything the claims say.

use serde_json::{json, Map, Value};

use crate::discovery::KeyResolver;
use crate::error::TokenError;
use crate::identifier::is_server_identifier;
use crate::jwk::{Algorithm, Jwk};
use crate::jwt;
use crate::keys::SigningKey;
use crate::metadata::dwk;

/// The four AAuth token types, by their JWT `typ`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum TokenKind {
    /// `aa-agent+jwt`: an agent provider vouches for an agent and binds its key.
    Agent,
    /// `aa-person+jwt`: a Person Server names the person an agent acts for,
    /// to one resource. Identity, never authorization.
    Person,
    /// `aa-auth+jwt`: what a resource accepts as authorization.
    Auth,
    /// `aa-resource+jwt`: a resource asks the Person Server for an auth token.
    Resource,
}

impl TokenKind {
    /// The JWT `typ` header value.
    pub fn typ(self) -> &'static str {
        match self {
            TokenKind::Agent => "aa-agent+jwt",
            TokenKind::Person => "aa-person+jwt",
            TokenKind::Auth => "aa-auth+jwt",
            TokenKind::Resource => "aa-resource+jwt",
        }
    }

    /// The kind a JWT `typ` names.
    pub fn from_typ(typ: &str) -> Option<Self> {
        [
            TokenKind::Agent,
            TokenKind::Person,
            TokenKind::Auth,
            TokenKind::Resource,
        ]
        .into_iter()
        .find(|k| k.typ() == typ)
    }

    fn error_code(self) -> &'static str {
        match self {
            TokenKind::Agent => "invalid_agent_token",
            TokenKind::Person => "invalid_person_token",
            TokenKind::Auth => "invalid_auth_token",
            TokenKind::Resource => "invalid_resource_token",
        }
    }
}

/// Mint a token of `kind`, signed by `key` and naming it in `kid` by its
/// thumbprint. `claims` are written as given: the issuer decides them.
///
/// ```
/// use aauth_protocol::token::{mint, TokenKind};
/// use aauth_protocol::SigningKey;
///
/// let provider = SigningKey::generate();
/// let session = SigningKey::generate();
/// let agent_token = mint(&provider, TokenKind::Agent, &serde_json::json!({
///     "iss": "https://agents.example",
///     "dwk": "aauth-agent.json",
///     "sub": "aauth:travel@agents.example",
///     "iat": 1_791_000_000,
///     "exp": 1_791_003_600,
///     "cnf": { "jwk": session.public_jwk() },
/// }));
/// assert_eq!(agent_token.split('.').count(), 3);
/// ```
pub fn mint(key: &SigningKey, kind: TokenKind, claims: &Value) -> String {
    let header = json!({ "typ": kind.typ(), "kid": key.thumbprint() });
    jwt::encode(key, header.as_object().cloned().unwrap_or_default(), claims)
}

/// What [`verify_token`] checks a token against. Build one with
/// [`VerifyTokenOptions::new`].
#[derive(Clone, Debug)]
pub struct VerifyTokenOptions<'a> {
    /// The token, as the `jwt` parameter of `Signature-Key` carried it.
    pub jwt: &'a str,
    /// The thumbprint of the key that signed the HTTP request.
    pub http_signature_thumbprint: &'a str,
    /// This verifier's own server identifier: a person or auth token's `aud`.
    pub resource: &'a str,
    /// The kinds this call site accepts. A person token is identity, so a
    /// resource authorizing a request accepts only [`TokenKind::Auth`].
    pub accept: &'a [TokenKind],
    /// Seconds since the epoch.
    pub now: i64,
    /// How far ahead of `now` a token's `iat` may be.
    pub clock_tolerance: i64,
}

impl<'a> VerifyTokenOptions<'a> {
    /// Verify `jwt` now, with a 60-second tolerance for the issuer's clock.
    pub fn new(
        jwt: &'a str,
        http_signature_thumbprint: &'a str,
        resource: &'a str,
        accept: &'a [TokenKind],
    ) -> Self {
        Self {
            jwt,
            http_signature_thumbprint,
            resource,
            accept,
            now: crate::now_unix(),
            clock_tolerance: 60,
        }
    }
}

/// A token whose issuer's signature verified, bound to the key that signed
/// the request.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct VerifiedToken {
    /// Which kind it is.
    pub kind: TokenKind,
    /// Its issuer.
    pub iss: String,
    /// The metadata document its issuer's keys were found through.
    pub dwk: String,
    /// The agent identifier, or the person's directed identifier.
    pub sub: String,
    /// The key the token binds, which signed the request.
    pub cnf_jwk: Jwk,
    /// When it was issued.
    pub iat: i64,
    /// When it expires.
    pub exp: i64,
    /// Every claim, for those without an accessor.
    pub claims: Map<String, Value>,
}

impl VerifiedToken {
    /// A non-empty string claim.
    pub fn claim_str(&self, name: &str) -> Option<&str> {
        self.claims
            .get(name)
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
    }

    /// `jti`: the token's identifier at its issuer.
    pub fn jti(&self) -> Option<&str> {
        self.claim_str("jti")
    }

    /// An agent token's Person Server, or an auth token's.
    pub fn ps(&self) -> Option<&str> {
        self.claim_str("ps")
    }

    /// The mission the token was issued under.
    pub fn mission_s256(&self) -> Option<&str> {
        self.claim_str("mission_s256")
    }

    /// The person's organisation, at a multi-tenant Person Server.
    pub fn tenant(&self) -> Option<&str> {
        self.claim_str("tenant")
    }

    /// An auth token's scope.
    pub fn scope(&self) -> Option<&str> {
        self.claim_str("scope")
    }
}

fn aud_matches(aud: Option<&Value>, resource: &str) -> bool {
    match aud {
        Some(Value::String(a)) => a == resource,
        Some(Value::Array(list)) => list.iter().any(|a| a.as_str() == Some(resource)),
        _ => false,
    }
}

fn constant_time_eq(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0u8, |acc, (x, y)| acc | (x ^ y))
            == 0
}

/// Verify an agent, person or auth token presented in `Signature-Key`.
///
/// In order: the `typ` is recognised and accepted here; the claims every
/// token of that kind needs are present; its `cnf.jwk` is the key that
/// signed the HTTP request; the issuer's key, found through `resolver`,
/// signed it; and only then its expiry, `iat` and `aud`. Nothing in the
/// claims is believed before the signature verifies, so `token_expired`
/// always means the named issuer minted a token that has run out.
///
/// Revocation is not checked here.
pub async fn verify_token(
    options: &VerifyTokenOptions<'_>,
    resolver: &dyn KeyResolver,
) -> Result<VerifiedToken, TokenError> {
    if options.accept.is_empty() {
        return Err(TokenError::new(
            "invalid_configuration",
            "accept names no token kinds",
        ));
    }
    if !is_server_identifier(options.resource) {
        return Err(TokenError::new(
            "invalid_configuration",
            format!("resource {} is not a server identifier", options.resource),
        ));
    }
    let decoded = jwt::decode(options.jwt)
        .map_err(|_| TokenError::new("malformed_token", "not a well-formed JWT"))?;
    let typ = decoded.typ().unwrap_or_default();
    let kind = TokenKind::from_typ(typ)
        .filter(|k| *k != TokenKind::Resource)
        .ok_or_else(|| {
            TokenError::new("unsupported_token_type", format!("unknown JWT typ {typ:?}"))
        })?;
    if !options.accept.contains(&kind) {
        return Err(TokenError::new(
            "token_type_not_accepted",
            format!("a {typ} is not accepted here"),
        ));
    }
    let code = kind.error_code();
    let alg = decoded.alg().unwrap_or_default();
    let algorithm = Algorithm::from_name(alg).ok_or_else(|| {
        TokenError::new(
            code,
            format!("JWT alg {alg:?} is not a fully-specified algorithm this verifier supports"),
        )
    })?;

    let required = |name: &str| {
        decoded
            .claim_str(name)
            .map(str::to_string)
            .ok_or_else(|| TokenError::new(code, format!("missing required claim: {name}")))
    };
    let iss = required("iss")?;
    let token_dwk = required("dwk")?;
    let number = |name: &str| {
        decoded
            .claims
            .get(name)
            .and_then(Value::as_i64)
            .ok_or_else(|| TokenError::new(code, format!("missing required claim: {name}")))
    };
    let iat = number("iat")?;
    let exp = number("exp")?;
    let expected_dwk: &[&str] = match kind {
        TokenKind::Agent => &[dwk::AGENT],
        TokenKind::Person => &[dwk::PERSON],
        _ => &[dwk::PERSON, dwk::ACCESS],
    };
    if !expected_dwk.contains(&token_dwk.as_str()) {
        return Err(TokenError::new(
            code,
            format!("unexpected dwk {token_dwk:?}"),
        ));
    }
    let cnf: Jwk = decoded
        .claims
        .get("cnf")
        .and_then(|c| c.get("jwk"))
        .cloned()
        .and_then(|j| serde_json::from_value(j).ok())
        .ok_or_else(|| TokenError::new(code, "missing required claim: cnf.jwk"))?;
    cnf.check_confirmation_key()
        .map_err(|e| TokenError::new(code, e.to_string()))?;
    let sub = required("sub")?;
    match kind {
        TokenKind::Person => {
            required("aud")?;
            required("jti")?;
        }
        TokenKind::Auth => {
            if decoded.claims.get("aud").is_none() {
                return Err(TokenError::new(code, "missing required claim: aud"));
            }
            required("ps")?;
        }
        _ => {}
    }

    let thumbprint = cnf
        .thumbprint()
        .map_err(|e| TokenError::new(code, e.to_string()))?;
    if !constant_time_eq(&thumbprint, options.http_signature_thumbprint) {
        return Err(TokenError::new(
            "key_binding_failed",
            "cnf.jwk thumbprint does not match the HTTP signature key",
        ));
    }

    if !is_server_identifier(&iss) {
        return Err(TokenError::new(
            code,
            format!("iss is not a server identifier: {iss}"),
        ));
    }
    let jwks = resolver
        .jwks(&iss, &token_dwk)
        .await
        .map_err(|e| TokenError::new(code, e.to_string()))?;
    let key = match decoded.kid() {
        Some(kid) if !kid.is_empty() => jwks
            .keys
            .iter()
            .find(|k| k.kid.as_deref() == Some(kid))
            .ok_or_else(|| TokenError::new(code, format!("no key {kid:?} in the issuer's JWKS")))?,
        _ if jwks.keys.len() == 1 => &jwks.keys[0],
        _ => {
            return Err(TokenError::new(
                code,
                "the JWT names no kid and the issuer publishes more than one key",
            ))
        }
    };
    if key.alg.as_deref().is_some_and(|a| a != alg) {
        return Err(TokenError::new(
            code,
            "the issuer's key alg disagrees with the JWT's",
        ));
    }
    if !decoded.verify_signature(key, algorithm) {
        return Err(TokenError::new(code, "JWT signature verification failed"));
    }
    if options.now >= exp {
        return Err(TokenError::new("token_expired", "token has expired"));
    }
    if decoded
        .claims
        .get("nbf")
        .and_then(Value::as_i64)
        .is_some_and(|nbf| options.now < nbf)
    {
        return Err(TokenError::new(code, "token is not yet valid"));
    }
    if iat > options.now + options.clock_tolerance {
        return Err(TokenError::new(
            "clock_skew",
            format!(
                "token iat is {}s ahead of this verifier's clock",
                iat - options.now
            ),
        ));
    }
    if matches!(kind, TokenKind::Person | TokenKind::Auth)
        && !aud_matches(decoded.claims.get("aud"), options.resource)
    {
        return Err(TokenError::new(
            "aud_mismatch",
            "token aud is not this resource",
        ));
    }
    match kind {
        TokenKind::Agent => {
            if let Some(ps) = decoded.claim_str("ps") {
                if !is_server_identifier(ps) {
                    return Err(TokenError::new(
                        code,
                        format!("ps is not a server identifier: {ps}"),
                    ));
                }
            }
        }
        TokenKind::Person => {
            if decoded.claims.contains_key("scope") || decoded.claims.contains_key("account") {
                return Err(TokenError::new(
                    code,
                    "a person token MUST NOT carry scope or account",
                ));
            }
        }
        TokenKind::Auth => {
            if !decoded.claim_str("ps").is_some_and(is_server_identifier) {
                return Err(TokenError::new(code, "ps is not a server identifier"));
            }
        }
        TokenKind::Resource => {}
    }
    Ok(VerifiedToken {
        kind,
        iss,
        dwk: token_dwk,
        sub,
        cnf_jwk: cnf,
        iat,
        exp,
        claims: decoded.claims,
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::discovery::{JwksResolver, StaticFetch};
    use std::sync::Arc;

    pub(crate) const AP: &str = "https://agents.example";

    pub(crate) fn provider(ap_key: &SigningKey) -> JwksResolver {
        let fetch = StaticFetch::default();
        fetch.insert(
            format!("{AP}/.well-known/aauth-agent.json"),
            json!({ "issuer": AP, "jwks_uri": format!("{AP}/jwks.json") }),
        );
        fetch.insert(
            format!("{AP}/jwks.json"),
            serde_json::to_value(ap_key.jwks()).unwrap(),
        );
        JwksResolver::new(Arc::new(fetch))
    }

    pub(crate) fn agent_token(ap_key: &SigningKey, session: &SigningKey, exp: i64) -> String {
        mint(
            ap_key,
            TokenKind::Agent,
            &json!({
                "iss": AP, "dwk": "aauth-agent.json", "sub": "aauth:travel@agents.example",
                "iat": 1_000, "exp": exp, "cnf": { "jwk": session.public_jwk() },
            }),
        )
    }

    fn options<'a>(jwt: &'a str, thumbprint: &'a str, now: i64) -> VerifyTokenOptions<'a> {
        VerifyTokenOptions {
            jwt,
            http_signature_thumbprint: thumbprint,
            resource: "https://ps.example",
            accept: &[TokenKind::Agent],
            now,
            clock_tolerance: 60,
        }
    }

    #[tokio::test]
    async fn an_agent_token_verifies_against_its_providers_keys() {
        let (ap, session) = (SigningKey::generate(), SigningKey::generate());
        let token = agent_token(&ap, &session, 5_000);
        let verified = verify_token(
            &options(&token, session.thumbprint(), 1_000),
            &provider(&ap),
        )
        .await
        .unwrap();
        assert_eq!(verified.sub, "aauth:travel@agents.example");
        assert_eq!(verified.kind, TokenKind::Agent);
    }

    #[tokio::test]
    async fn a_token_bound_to_another_key_is_refused() {
        let (ap, session) = (SigningKey::generate(), SigningKey::generate());
        let token = agent_token(&ap, &session, 5_000);
        let other = SigningKey::generate();
        let refused = verify_token(&options(&token, other.thumbprint(), 1_000), &provider(&ap))
            .await
            .unwrap_err();
        assert_eq!(refused.code, "key_binding_failed");
    }

    #[tokio::test]
    async fn expiry_is_judged_only_once_the_signature_verifies() {
        let (ap, session) = (SigningKey::generate(), SigningKey::generate());
        let expired = agent_token(&ap, &session, 1_500);
        let refused = verify_token(
            &options(&expired, session.thumbprint(), 2_000),
            &provider(&ap),
        )
        .await
        .unwrap_err();
        assert_eq!(refused.code, "token_expired");

        let forged = agent_token(&SigningKey::generate(), &session, 1_500);
        let refused = verify_token(
            &options(&forged, session.thumbprint(), 2_000),
            &provider(&ap),
        )
        .await
        .unwrap_err();
        assert_eq!(refused.code, "invalid_agent_token");
    }

    #[tokio::test]
    async fn a_kind_not_accepted_here_is_refused() {
        let (ap, session) = (SigningKey::generate(), SigningKey::generate());
        let token = agent_token(&ap, &session, 5_000);
        let mut opts = options(&token, session.thumbprint(), 1_000);
        opts.accept = &[TokenKind::Auth];
        let refused = verify_token(&opts, &provider(&ap)).await.unwrap_err();
        assert_eq!(refused.code, "token_type_not_accepted");
    }
}
