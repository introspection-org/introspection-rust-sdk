//! The resource's side: authenticating a signed request, minting resource
//! tokens, and challenging for what is missing.

use serde_json::{json, Value};

use crate::budget::Budget;
use crate::discovery::KeyResolver;
use crate::error::{Error, Result, SignatureError};
use crate::httpsig::{self, Incoming, Verified, VerifyOptions};
use crate::identifier::is_server_identifier;
use crate::keys::SigningKey;
use crate::metadata::dwk;
use crate::requirement::Requirement;
use crate::token::{mint, verify_token, TokenKind, VerifiedToken, VerifyTokenOptions};

/// A resource token's default lifetime: the spec says it SHOULD NOT exceed
/// five minutes.
pub const DEFAULT_RESOURCE_TOKEN_LIFETIME: i64 = 300;

/// A request whose signature verified, and the token it presented.
#[derive(Clone, Debug)]
pub struct Authenticated {
    /// The HTTP signature, and the key that made it.
    pub signature: Verified,
    /// The token, verified against its issuer's keys and bound to that key.
    pub token: VerifiedToken,
}

/// Verify a request's signature, then the token it presents in
/// `Signature-Key`, as one of the `accept`ed kinds, for `resource` (this
/// server's own identifier). What a resource does with every request.
///
/// Only the `jwt` scheme is accepted, and `resolver` is consulted only for
/// the token's issuer: a `jwks_uri` signature, which would have this server
/// fetch whatever the unauthenticated caller names, is refused without a
/// fetch. Verify other schemes with [`httpsig::verify`] directly.
pub async fn authenticate(
    request: &Incoming<'_>,
    resource: &str,
    accept: &[TokenKind],
    options: &VerifyOptions,
    resolver: &dyn KeyResolver,
) -> Result<Authenticated> {
    let signature = httpsig::verify(request, options, &crate::discovery::NoKeys).await?;
    let Some(jwt) = signature.jwt() else {
        return Err(crate::SignatureError::new("unsupported_scheme", "sign with sig=jwt").into());
    };
    let token = verify_token(
        &VerifyTokenOptions {
            now: options.now,
            ..VerifyTokenOptions::new(jwt, &signature.thumbprint, resource, accept)
        },
        resolver,
    )
    .await?;
    Ok(Authenticated { signature, token })
}

/// What a resource token asks the Person Server for. Build one with
/// [`ResourceTokenParams::new`].
#[derive(Clone, Debug)]
pub struct ResourceTokenParams<'a> {
    /// This resource's server identifier.
    pub resource: &'a str,
    /// The person or auth token this resource verified on the request.
    pub presented: &'a VerifiedToken,
    /// Thumbprint of the key that signed the request.
    pub agent_jkt: &'a str,
    /// What the agent asks to do, as this resource names it.
    pub scope: &'a str,
    /// What the request costs, when it spends from a mission's budget.
    pub budget: Option<&'a Budget>,
    /// An R3 proposal: the URI of a document describing the request, and its
    /// `s256`. The Person Server shows its `display.summary` to the approver.
    pub r3: Option<(&'a str, &'a str)>,
    /// When the presented token's mission ends: no token outlives it.
    pub mission_expires_at: Option<i64>,
    /// This resource's own Access Server, when it has one (four-party): the
    /// token's `aud` (AAuth §Resource Token, `#resource-token`). Unset, the
    /// `aud` is the Person Server that issued the presented token.
    pub access_server: Option<&'a str>,
    /// Seconds since the epoch.
    pub now: i64,
    /// Seconds the token is good for.
    pub lifetime: i64,
}

impl<'a> ResourceTokenParams<'a> {
    /// A resource token for `scope`, issued now for
    /// [`DEFAULT_RESOURCE_TOKEN_LIFETIME`].
    pub fn new(
        resource: &'a str,
        presented: &'a VerifiedToken,
        agent_jkt: &'a str,
        scope: &'a str,
    ) -> Self {
        Self {
            resource,
            presented,
            agent_jkt,
            scope,
            budget: None,
            r3: None,
            mission_expires_at: None,
            access_server: None,
            now: crate::now_unix(),
            lifetime: DEFAULT_RESOURCE_TOKEN_LIFETIME,
        }
    }

    /// The request costs `budget`.
    pub fn budget(mut self, budget: &'a Budget) -> Self {
        self.budget = Some(budget);
        self
    }

    /// The request is described at `uri`, whose bytes hash to `s256`.
    pub fn r3(mut self, uri: &'a str, s256: &'a str) -> Self {
        self.r3 = Some((uri, s256));
        self
    }

    /// Address the token to this resource's own Access Server, which the
    /// Person Server then federates with (AAuth `#ps-as-federation`).
    pub fn access_server(mut self, access_server: &'a str) -> Self {
        self.access_server = Some(access_server);
        self
    }
}

/// Mint a resource token for a `requirement=auth-token` challenge, addressed
/// to this resource's Access Server when it has one, else to the Person
/// Server that issued the presented token, and carrying the presented
/// token's `sub`, `mission_s256` and `tenant` unchanged.
pub fn resource_token(key: &SigningKey, params: &ResourceTokenParams<'_>) -> Result<String> {
    if !is_server_identifier(params.resource) {
        return Err(Error::Invalid(format!(
            "{} is not a server identifier",
            params.resource
        )));
    }
    if params
        .access_server
        .is_some_and(|access_server| !is_server_identifier(access_server))
    {
        return Err(Error::Invalid(
            "the Access Server is not a server identifier".into(),
        ));
    }
    if params.scope.is_empty() {
        return Err(Error::Invalid("a resource token needs a scope".into()));
    }
    let presented = params.presented;
    // An auth token names its Person Server in `ps`; a person token's is its `iss`.
    let ps = presented.ps().unwrap_or(presented.iss.as_str());
    let jti = presented.jti().ok_or_else(|| {
        Error::Invalid("the presented token has no jti, so a resource token cannot name it".into())
    })?;
    let exp = match params.mission_expires_at {
        Some(end) => (params.now + params.lifetime).min(end),
        None => params.now + params.lifetime,
    };
    if exp <= params.now {
        return Err(Error::Invalid("the mission has already ended".into()));
    }
    let mut claims = json!({
        "iss": params.resource,
        "dwk": dwk::RESOURCE,
        "aud": params.access_server.unwrap_or(ps),
        "ps": ps,
        "jti": crate::random_id(),
        "sub": presented.sub,
        "presented_jti": jti,
        // The pre-rename alias of `presented_jti`, for Person Servers not yet on -11.
        "person_token_jti": jti,
        "agent_jkt": params.agent_jkt,
        "scope": params.scope,
        "iat": params.now,
        "exp": exp,
    });
    if let Some(mission) = presented.mission_s256() {
        claims["mission_s256"] = json!(mission);
    }
    if let Some(tenant) = presented.tenant() {
        claims["tenant"] = json!(tenant);
    }
    if let Some(budget) = params.budget {
        claims["budget"] = serde_json::to_value(budget).unwrap_or(Value::Null);
    }
    if let Some((uri, s256)) = params.r3 {
        claims["r3_uri"] = json!(uri);
        claims["r3_s256"] = json!(s256);
    }
    Ok(mint(key, TokenKind::Resource, &claims))
}

/// The headers of a `401` asking for `requirement`.
pub fn challenge(requirement: &Requirement) -> Vec<(String, String)> {
    vec![(
        crate::requirement::HEADER.to_string(),
        requirement.to_header(),
    )]
}

/// Whose auth tokens a resource accepts.
#[derive(Clone, Copy, Debug)]
pub enum AuthTokenIssuers<'a> {
    /// Three-party: each of these Person Servers issues its own, `dwk`
    /// `aauth-person.json` with `ps` its `iss`.
    PersonServers(&'a [String]),
    /// Four-party: only this resource's Access Server issues them, `dwk`
    /// `aauth-access.json`, for the people of `person_servers`. One a Person
    /// Server minted itself would skip the Access Server's policy, so it is
    /// refused.
    AccessServer {
        /// The Access Server's identifier.
        access_server: &'a str,
        /// The Person Servers whose people it issues for: an auth token's `ps`.
        person_servers: &'a [String],
    },
}

impl AuthTokenIssuers<'_> {
    /// Whether `token`, verified by [`authenticate`], is an auth token one
    /// of these issued, answered as `invalid_jwt` when it is not. A token of
    /// any other kind passes: its issuer is the caller's to check.
    pub fn check(&self, token: &VerifiedToken) -> Result<(), SignatureError> {
        if token.kind != TokenKind::Auth {
            return Ok(());
        }
        let ps = token.ps().unwrap_or_default();
        let issued_here = match self {
            Self::PersonServers(person_servers) => {
                token.dwk == dwk::PERSON && ps == token.iss && person_servers.contains(&token.iss)
            }
            Self::AccessServer {
                access_server,
                person_servers,
            } => {
                token.iss == *access_server
                    && token.dwk == dwk::ACCESS
                    && person_servers.iter().any(|p| p == ps)
            }
        };
        if issued_here {
            Ok(())
        } else {
            Err(SignatureError::new(
                "invalid_jwt",
                format!("{} does not issue the auth tokens accepted here", token.iss),
            ))
        }
    }
}
