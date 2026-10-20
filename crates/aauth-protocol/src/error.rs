//! Errors. A signature or token that does not verify carries the registered
//! code a peer is answered with; everything else is an [`Error`] variant
//! naming what kind of thing went wrong.

/// An HTTP signature that does not verify, answered with `401` and
/// `Signature-Error: error=<code>` ([`SignatureError::header_value`]).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{code}: {detail}")]
pub struct SignatureError {
    /// The registered code: `invalid_signature`, `invalid_input`,
    /// `invalid_key`, `unknown_key`, `invalid_jwt`, `unsupported_scheme`,
    /// `unsupported_algorithm` or `clock_skew`.
    pub code: &'static str,
    /// A human-readable explanation, for logs. Not part of the protocol.
    pub detail: String,
}

impl SignatureError {
    /// A signature error with `code` and `detail`.
    pub fn new(code: &'static str, detail: impl Into<String>) -> Self {
        Self {
            code,
            detail: detail.into(),
        }
    }

    /// The `Signature-Error` response header value.
    pub fn header_value(&self) -> String {
        format!("error={}", self.code)
    }
}

/// A token that does not verify.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{code}: {detail}")]
pub struct TokenError {
    /// The code: `invalid_agent_token`, `invalid_person_token`,
    /// `invalid_auth_token`, `malformed_token`, `unsupported_token_type`,
    /// `token_type_not_accepted`, `token_expired`, `key_binding_failed`,
    /// `aud_mismatch`, `clock_skew` or `invalid_configuration`.
    ///
    /// `token_expired` and `clock_skew` are reported only once the issuer's
    /// signature has verified, so a presenter can act on them: refresh the
    /// token, or wait out the skew.
    pub code: &'static str,
    /// A human-readable explanation, for logs. Not part of the protocol.
    pub detail: String,
}

impl TokenError {
    /// A token error with `code` and `detail`.
    pub fn new(code: &'static str, detail: impl Into<String>) -> Self {
        Self {
            code,
            detail: detail.into(),
        }
    }
}

/// Everything else that can go wrong.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// An HTTP signature did not verify.
    #[error(transparent)]
    Signature(#[from] SignatureError),
    /// A token did not verify.
    #[error(transparent)]
    Token(#[from] TokenError),
    /// A key that cannot be read or used.
    #[error("invalid key: {0}")]
    Key(String),
    /// A URL that cannot be parsed, or that this client refuses to call.
    #[error("url: {0}")]
    Url(String),
    /// A metadata document or key set that cannot be fetched, read or trusted.
    #[error("discovery: {0}")]
    Discovery(String),
    /// A peer that could not be reached, failed, or answered with something
    /// unusable. Never a decision: a refusal is an answer, not an error.
    #[error("http: {0}")]
    Http(String),
    /// A value that is not what the protocol requires.
    #[error("invalid: {0}")]
    Invalid(String),
}

/// `Result` with this crate's [`Error`].
pub type Result<T, E = Error> = std::result::Result<T, E>;
