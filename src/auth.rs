//! Server-side OAuth helpers for machine and federated authentication.
//!
//! These mint a short-lived, project-scoped Introspection access token from
//! the Control Plane `POST /v1/oauth/token`, so server code (CI jobs,
//! hosted-login backends, federation brokers) no longer hand-rolls a
//! form-encoded token POST:
//!
//! * [`service_account_token`] — OAuth 2.0 `client_credentials` grant for a
//!   confidential machine Application. The headless counterpart to a
//!   long-lived API key: the `client_id` / `client_secret` stay server-side
//!   and you re-mint when the token expires (no refresh token is issued).
//! * [`token_exchange`] — RFC 8693 token-exchange: trade an end user's
//!   partner-IdP token for a project-scoped access token for a federated
//!   `customer` member.
//! * [`authorization_code_token`] — RFC 6749 / PKCE `authorization_code`
//!   exchange for the hosted-login callback.
//!
//! Every helper returns the shared [`OAuthToken`] shape, which carries
//! `dp_url` — the Data Plane endpoint the CP resolved for the token's
//! project. A broker hands that straight to a browser client so the SPA
//! connects without separately configured Data Plane URLs.
//!
//! The minted `access_token` is an ordinary CP bearer token, so it drops
//! straight into [`crate::IntrospectionClient`], or use
//! [`crate::IntrospectionClient::from_service_account`] to mint and construct
//! in one call.

use std::collections::HashMap;
use std::env;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::api::encoding::encode_form_component;
use crate::api::error::{ApiResult, IntrospectionAPIError};
use crate::types::defaults;

const TOKEN_PATH: &str = "/v1/oauth/token";
const GRANT_CLIENT_CREDENTIALS: &str = "client_credentials";
const GRANT_TOKEN_EXCHANGE: &str = "urn:ietf:params:oauth:grant-type:token-exchange";
const GRANT_AUTHORIZATION_CODE: &str = "authorization_code";
const SUBJECT_TOKEN_TYPE_ID_TOKEN: &str = "urn:ietf:params:oauth:token-type:id_token";

/// CP `POST /v1/oauth/token` response.
///
/// No refresh token is issued for the machine grants — re-mint (call the
/// helper again) once it expires.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthToken {
    /// Project-scoped RS256 access token (`Authorization: Bearer …`).
    pub access_token: String,
    /// Always `"Bearer"`.
    #[serde(default = "default_token_type")]
    pub token_type: String,
    /// Token lifetime in seconds.
    pub expires_in: i64,
    /// The granted (scope-capped) scope, when the CP returns one.
    #[serde(default)]
    pub scope: Option<String>,
    /// Data Plane API base URL for the token's project, resolved by the CP.
    /// `None` when no deployment resolves; the caller then needs an explicit
    /// DP URL. Hand this to a browser client so it needs no separate
    /// Data Plane configuration.
    #[serde(default)]
    pub dp_url: Option<String>,
    /// Any further fields the CP returned (`project`, `org_id`, …).
    ///
    /// Kept rather than dropped so a field the CP adds is reachable without
    /// waiting on a release of this crate.
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

fn default_token_type() -> String {
    "Bearer".to_string()
}

/// Parameters for [`service_account_token`].
#[derive(Debug, Clone, Default, derive_builder::Builder)]
#[builder(setter(into), default)]
pub struct ServiceAccountTokenParams {
    /// Confidential Application client id (`intro_app_…`).
    pub client_id: String,
    /// Confidential Application client secret (`intro_sk_…`).
    pub client_secret: String,
    /// Project the token is scoped to. Required by the CP `client_credentials`
    /// grant — the minted token is project-scoped and the project must belong
    /// to the Application's organization.
    pub project: String,
    /// Optional space-separated scope. Capped server-side to the
    /// Application's `allowed_scopes`; omit to receive the default scope.
    #[builder(setter(strip_option))]
    pub scope: Option<String>,
    /// CP API base URL. Defaults to `INTROSPECTION_BASE_API_URL` or
    /// `https://api.introspection.dev`.
    #[builder(setter(strip_option))]
    pub base_api_url: Option<String>,
}

impl ServiceAccountTokenParams {
    /// Create a builder.
    pub fn builder() -> ServiceAccountTokenParamsBuilder {
        ServiceAccountTokenParamsBuilder::default()
    }
}

/// Parameters for [`token_exchange`].
#[derive(Debug, Clone, Default, derive_builder::Builder)]
#[builder(setter(into), default)]
pub struct TokenExchangeParams {
    /// The end user's subject token (e.g. a partner-IdP `id_token`).
    pub subject_token: String,
    /// The federated Application's `client_id` (public client — no secret).
    pub client_id: String,
    /// Project the minted token is scoped to.
    pub project: String,
    /// The subject token's type URI. Defaults to
    /// `urn:ietf:params:oauth:token-type:id_token`.
    #[builder(setter(strip_option))]
    pub subject_token_type: Option<String>,
    /// Optional space-separated scope, capped server-side.
    #[builder(setter(strip_option))]
    pub scope: Option<String>,
    /// CP API base URL. Defaults to `INTROSPECTION_BASE_API_URL` or
    /// `https://api.introspection.dev`.
    #[builder(setter(strip_option))]
    pub base_api_url: Option<String>,
}

impl TokenExchangeParams {
    /// Create a builder.
    pub fn builder() -> TokenExchangeParamsBuilder {
        TokenExchangeParamsBuilder::default()
    }
}

/// Parameters for [`authorization_code_token`].
#[derive(Debug, Clone, Default, derive_builder::Builder)]
#[builder(setter(into), default)]
pub struct AuthorizationCodeParams {
    /// The authorization code returned to the redirect URI.
    pub code: String,
    /// Public SPA Application `client_id` (PKCE — no secret).
    pub client_id: String,
    /// The redirect URI the code was issued for (must match the authorize
    /// call).
    pub redirect_uri: String,
    /// The PKCE `code_verifier` paired with the authorize-step challenge.
    pub code_verifier: String,
    /// CP API base URL. Defaults to `INTROSPECTION_BASE_API_URL` or
    /// `https://api.introspection.dev`.
    #[builder(setter(strip_option))]
    pub base_api_url: Option<String>,
}

impl AuthorizationCodeParams {
    /// Create a builder.
    pub fn builder() -> AuthorizationCodeParamsBuilder {
        AuthorizationCodeParamsBuilder::default()
    }
}

pub(crate) fn resolve_base_api_url(base_api_url: Option<&str>) -> String {
    let resolved = base_api_url
        .map(str::to_string)
        .or_else(|| env::var("INTROSPECTION_BASE_API_URL").ok())
        .unwrap_or_else(|| defaults::BASE_API_URL.to_string());
    resolved.trim_end_matches('/').to_string()
}

/// Encode the grant parameters as `application/x-www-form-urlencoded`.
///
/// Hand-rolled on the crate's existing percent encoder rather than through
/// reqwest's `.form()`, which needs a `serde_urlencoded` feature this crate
/// does not enable.
fn urlencode_form(form: &[(&str, &str)]) -> String {
    form.iter()
        .map(|(k, v)| format!("{}={}", encode_form_component(k), encode_form_component(v)))
        .collect::<Vec<_>>()
        .join("&")
}

async fn post_token_form(base_api_url: &str, form: &[(&str, &str)]) -> ApiResult<OAuthToken> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(defaults::API_TIMEOUT_SECS))
        .build()?;
    let res = client
        .post(format!("{base_api_url}{TOKEN_PATH}"))
        .header(
            reqwest::header::CONTENT_TYPE,
            "application/x-www-form-urlencoded",
        )
        .body(urlencode_form(form))
        .send()
        .await?;
    let status = res.status();
    if !status.is_success() {
        return Err(crate::api::http::to_api_error(res, status).await);
    }
    res.json::<OAuthToken>()
        .await
        .map_err(|e| IntrospectionAPIError::Decode(e.to_string()))
}

/// Mint a project-scoped CP access token from service-account credentials.
///
/// `client_id` (`intro_app_…`) and `client_secret` (`intro_sk_…`) come from a
/// confidential machine Application; `project` scopes the token (the project
/// must belong to the Application's organization). `scope` is capped
/// server-side to the Application's allowed scopes.
///
/// See [`crate::IntrospectionClient::from_service_account`] to mint and
/// construct a client in one call.
///
/// # Example
///
/// ```rust,no_run
/// use introspection_sdk::auth::{service_account_token, ServiceAccountTokenParams};
///
/// # async fn main_() -> Result<(), Box<dyn std::error::Error>> {
/// let token = service_account_token(
///     ServiceAccountTokenParams::builder()
///         .client_id(std::env::var("INTRO_SA_CLIENT_ID")?)
///         .client_secret(std::env::var("INTRO_SA_CLIENT_SECRET")?)
///         .project(std::env::var("INTRO_PROJECT")?)
///         .build()?,
/// )
/// .await?;
/// println!("{} (dp: {:?})", token.access_token, token.dp_url);
/// # Ok(()) }
/// ```
pub async fn service_account_token(params: ServiceAccountTokenParams) -> ApiResult<OAuthToken> {
    let base = resolve_base_api_url(params.base_api_url.as_deref());
    let mut form: Vec<(&str, &str)> = vec![
        ("grant_type", GRANT_CLIENT_CREDENTIALS),
        ("client_id", &params.client_id),
        ("client_secret", &params.client_secret),
        ("project", &params.project),
    ];
    if let Some(scope) = params.scope.as_deref() {
        form.push(("scope", scope));
    }
    post_token_form(&base, &form).await
}

/// RFC 8693 token-exchange against CP `POST /v1/oauth/token`.
///
/// Trade an end user's partner-IdP token (`subject_token`, an `id_token` by
/// default) for a project-scoped access token for a federated `customer`
/// member. `client_id` is the federated (public) Application's id. Run this
/// server-side in a broker — the subject token should not be re-handled in
/// the browser.
pub async fn token_exchange(params: TokenExchangeParams) -> ApiResult<OAuthToken> {
    let base = resolve_base_api_url(params.base_api_url.as_deref());
    let subject_token_type = params
        .subject_token_type
        .as_deref()
        .unwrap_or(SUBJECT_TOKEN_TYPE_ID_TOKEN);
    let mut form: Vec<(&str, &str)> = vec![
        ("grant_type", GRANT_TOKEN_EXCHANGE),
        ("subject_token", &params.subject_token),
        ("subject_token_type", subject_token_type),
        ("client_id", &params.client_id),
        ("project", &params.project),
    ];
    if let Some(scope) = params.scope.as_deref() {
        form.push(("scope", scope));
    }
    post_token_form(&base, &form).await
}

/// RFC 6749 / PKCE `authorization_code` exchange.
///
/// Run this in your backend so the hosted-login callback does not hand-roll
/// the token POST. `client_id` is the public SPA Application;
/// `code_verifier` pairs with the authorize-step challenge; `redirect_uri`
/// must match the authorize call.
pub async fn authorization_code_token(params: AuthorizationCodeParams) -> ApiResult<OAuthToken> {
    let base = resolve_base_api_url(params.base_api_url.as_deref());
    let form: Vec<(&str, &str)> = vec![
        ("grant_type", GRANT_AUTHORIZATION_CODE),
        ("code", &params.code),
        ("client_id", &params.client_id),
        ("redirect_uri", &params.redirect_uri),
        ("code_verifier", &params.code_verifier),
    ];
    post_token_form(&base, &form).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_api_url_is_trimmed_and_defaulted() {
        assert_eq!(
            resolve_base_api_url(Some("https://cp.example.com/")),
            "https://cp.example.com"
        );
    }

    #[test]
    fn token_type_defaults_when_the_cp_omits_it() {
        let token: OAuthToken =
            serde_json::from_str(r#"{"access_token":"a","expires_in":3600}"#).unwrap();
        assert_eq!(token.token_type, "Bearer");
        assert_eq!(token.scope, None);
        assert_eq!(token.dp_url, None);
    }

    #[test]
    fn unmodelled_fields_survive_the_round_trip() {
        // The CP returns `project` / `org_id` alongside the modelled
        // fields. Dropping them would put a field the CP already sends out
        // of reach until this crate is released again.
        let token: OAuthToken = serde_json::from_str(
            r#"{"access_token":"a","expires_in":3600,"project":"proj_1","org_id":"org_2"}"#,
        )
        .unwrap();
        assert_eq!(token.extra["project"], "proj_1");
        assert_eq!(token.extra["org_id"], "org_2");
    }
}
