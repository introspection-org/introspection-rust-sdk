//! OAuth 2.0 Device Authorization Grant (RFC 8628); see [`DeviceCodeAuth`].

use std::fmt;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::api::error::{ApiResult, IntrospectionAPIError};
use crate::api::http::to_api_error;
use crate::auth::{post_token_form_with, resolve_base_api_url, urlencode_form, OAuthToken};
use crate::types::defaults;

/// The `grant_type` the token endpoint takes a device code under.
pub const DEVICE_CODE_GRANT: &str = "urn:ietf:params:oauth:grant-type:device_code";

/// The first-party CLI client, the only `client_id` the Control Plane lets
/// start the device flow today.
pub const DEVICE_FLOW_CLIENT_ID: &str = "cli";

const DEVICE_CODE_PATH: &str = "/v1/oauth/device/code";
const GRANT_REFRESH_TOKEN: &str = "refresh_token";
const ACCESS_TOKEN_TYPE: &str = "urn:ietf:params:oauth:token-type:access_token";

/// The poll interval when the server names none (RFC 8628 §3.2).
const DEFAULT_INTERVAL_SECS: u64 = 5;
/// `slow_down` adds this much to the interval (RFC 8628 §3.5).
const SLOW_DOWN_BACKOFF_SECS: u64 = 5;
/// `interval: 0` asks for a tight loop against the token endpoint, and a very
/// large one parks the poll on one sleep that outlives the code; neither is
/// honoured.
const MIN_INTERVAL_SECS: u64 = 1;
const MAX_INTERVAL_SECS: u64 = 60;
/// Ceiling on the code lifetime the server reports. No server keeps a code
/// alive this long, and an unbounded one would overflow the deadline.
const MAX_EXPIRES_IN_SECS: u64 = 30 * 60;

/// CP `POST /v1/oauth/device/code` response (RFC 8628 §3.2).
///
/// Show `user_code` and `verification_uri` (or open
/// `verification_uri_complete`), then poll with [`DeviceCodeAuth::wait_for_token`].
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceCodeResponse {
    /// The opaque code the token endpoint is polled with. A secret: whoever
    /// holds it receives the tokens once the member approves.
    pub device_code: String,
    /// The short code the member types on the activation page.
    pub user_code: String,
    /// The activation page.
    pub verification_uri: String,
    /// The activation page with `user_code` already filled in.
    pub verification_uri_complete: String,
    /// Seconds until `device_code` expires.
    pub expires_in: u64,
    /// Seconds to wait between polls.
    #[serde(default = "default_interval")]
    pub interval: u64,
}

impl fmt::Debug for DeviceCodeResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DeviceCodeResponse")
            .field("device_code", &"<redacted>")
            .field("user_code", &self.user_code)
            .field("verification_uri", &self.verification_uri)
            .field("verification_uri_complete", &self.verification_uri_complete)
            .field("expires_in", &self.expires_in)
            .field("interval", &self.interval)
            .finish()
    }
}

fn default_interval() -> u64 {
    DEFAULT_INTERVAL_SECS
}

/// One poll of the token endpoint, classified (RFC 8628 §3.5).
#[derive(Debug, Clone)]
pub enum DeviceTokenPoll {
    /// The member approved: the login's tokens.
    Token(Box<OAuthToken>),
    /// `authorization_pending`: the member has not answered yet.
    Pending,
    /// `slow_down`: poll less often.
    SlowDown,
    /// `access_denied`: the member refused the login.
    AccessDenied,
    /// `expired_token`: the code ran out before the member answered.
    Expired,
}

/// Configuration for [`DeviceCodeAuth`].
#[derive(Debug, Clone, Default, derive_builder::Builder)]
#[builder(setter(into), default)]
pub struct DeviceCodeAuthConfig {
    /// The client starting the flow. Defaults to [`DEVICE_FLOW_CLIENT_ID`].
    #[builder(setter(strip_option))]
    pub client_id: Option<String>,
    /// CP API base URL. Defaults to `INTROSPECTION_BASE_API_URL` or
    /// `https://api.introspection.dev`.
    #[builder(setter(strip_option))]
    pub base_api_url: Option<String>,
}

impl DeviceCodeAuthConfig {
    /// Create a builder.
    pub fn builder() -> DeviceCodeAuthConfigBuilder {
        DeviceCodeAuthConfigBuilder::default()
    }
}

/// What to ask for in [`DeviceCodeAuth::request_code`].
#[derive(Debug, Clone, Default, derive_builder::Builder)]
#[builder(setter(into), default)]
pub struct DeviceCodeParams {
    /// Project slug or id to suggest on the activation page. The member
    /// picks the project when approving; this only preselects it.
    #[builder(setter(strip_option))]
    pub project: Option<String>,
    /// Capability bundles to limit the login to. Not a scope: the Control
    /// Plane intersects it with the member's entitlement, so it can only
    /// narrow. Empty asks for nothing in particular. An unknown name is
    /// refused with `400`.
    pub capabilities: Vec<String>,
}

impl DeviceCodeParams {
    /// Create a builder.
    pub fn builder() -> DeviceCodeParamsBuilder {
        DeviceCodeParamsBuilder::default()
    }
}

/// Device-code login: request a code, show it to the member, and poll the
/// token endpoint until they approve in the browser.
///
/// This is how `introspection login` signs a member in. The tokens it ends
/// with ([`OAuthToken`]) carry `cp_session` for Control Plane routes,
/// `refresh_token` / `session_id` / `org_id` for the refresh grant, and the
/// `cp_url` / `dp_url` / `git_url` / `platform_url` the login resolved.
/// [`Self::refresh`] renews it before it expires, and
/// [`Self::development_credential`] trades it for the short-lived bearer
/// `introspection dev` attaches with.
///
/// ```rust,no_run
/// use introspection_sdk::auth::{DeviceCodeAuth, DeviceCodeAuthConfig, DeviceCodeParams};
///
/// # async fn main_() -> Result<(), Box<dyn std::error::Error>> {
/// let auth = DeviceCodeAuth::new(DeviceCodeAuthConfig::default())?;
/// let device = auth.request_code(&DeviceCodeParams::default()).await?;
/// println!("Open {} and enter {}", device.verification_uri, device.user_code);
/// let token = auth.wait_for_token(&device).await?;
/// println!("signed in to org {:?}", token.org_id);
/// # Ok(()) }
/// ```
#[derive(Debug, Clone)]
pub struct DeviceCodeAuth {
    client_id: String,
    base_api_url: String,
    http: reqwest::Client,
}

impl DeviceCodeAuth {
    pub fn new(config: DeviceCodeAuthConfig) -> ApiResult<Self> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(defaults::API_TIMEOUT_SECS))
            // The poll waits `interval` seconds between requests, longer than
            // a typical server keep-alive, so a pooled socket is usually dead
            // by the next poll. Every poll opens a fresh connection instead;
            // the request rate is far too low for pooling to matter.
            .pool_max_idle_per_host(0)
            .build()?;
        Ok(Self {
            client_id: config
                .client_id
                .unwrap_or_else(|| DEVICE_FLOW_CLIENT_ID.to_string()),
            base_api_url: resolve_base_api_url(config.base_api_url.as_deref()),
            http,
        })
    }

    /// The CP base URL the flow runs against, without a trailing slash.
    pub fn base_api_url(&self) -> &str {
        &self.base_api_url
    }

    /// `POST /v1/oauth/device/code`: start a login.
    pub async fn request_code(&self, params: &DeviceCodeParams) -> ApiResult<DeviceCodeResponse> {
        let capabilities = params.capabilities.join(",");
        let mut form: Vec<(&str, &str)> = vec![("client_id", &self.client_id)];
        if let Some(project) = params.project.as_deref() {
            form.push(("project", project));
        }
        if !capabilities.is_empty() {
            form.push(("capabilities", &capabilities));
        }
        let res = self
            .http
            .post(format!("{}{DEVICE_CODE_PATH}", self.base_api_url))
            .header(
                reqwest::header::CONTENT_TYPE,
                "application/x-www-form-urlencoded",
            )
            .body(urlencode_form(&form))
            .send()
            .await?;
        let status = res.status();
        if !status.is_success() {
            return Err(to_api_error(res, status).await);
        }
        crate::api::http::decode_json(res).await
    }

    /// One `POST /v1/oauth/token` with the device-code grant.
    ///
    /// The four RFC 8628 answers come back as [`DeviceTokenPoll`]; any other
    /// error (an unknown `device_code` is `invalid_grant`) is an `Err`.
    pub async fn poll_token(&self, device_code: &str) -> ApiResult<DeviceTokenPoll> {
        let form = [
            ("grant_type", DEVICE_CODE_GRANT),
            ("device_code", device_code),
            ("client_id", self.client_id.as_str()),
        ];
        match post_token_form_with(&self.http, &self.base_api_url, &form).await {
            Ok(token) => Ok(DeviceTokenPoll::Token(Box::new(token))),
            Err(err) => match (err.status(), err.code()) {
                (Some(400), Some("authorization_pending")) => Ok(DeviceTokenPoll::Pending),
                (Some(400), Some("slow_down")) => Ok(DeviceTokenPoll::SlowDown),
                (Some(400), Some("access_denied")) => Ok(DeviceTokenPoll::AccessDenied),
                (Some(400), Some("expired_token")) => Ok(DeviceTokenPoll::Expired),
                _ => Err(err),
            },
        }
    }

    /// Poll until the member approves, then return the login's tokens.
    ///
    /// Waits `interval` between polls (bounded to 1–60 s) and five seconds
    /// longer after each `slow_down`. A transport error is retried on the
    /// next poll: one dropped connection should not end a multi-minute
    /// login. Fails with [`IntrospectionAPIError::Timeout`] when the code
    /// expires, and with the server's `400` when the member denies it.
    pub async fn wait_for_token(&self, device: &DeviceCodeResponse) -> ApiResult<OAuthToken> {
        let mut interval =
            Duration::from_secs(device.interval.clamp(MIN_INTERVAL_SECS, MAX_INTERVAL_SECS));
        let deadline = tokio::time::Instant::now()
            + Duration::from_secs(device.expires_in.min(MAX_EXPIRES_IN_SECS));
        let expired = || {
            IntrospectionAPIError::Timeout(
                "the device code expired before the login was approved".to_string(),
            )
        };
        loop {
            tokio::time::sleep(interval).await;
            if tokio::time::Instant::now() >= deadline {
                return Err(expired());
            }
            match self.poll_token(&device.device_code).await {
                Ok(DeviceTokenPoll::Token(token)) => return Ok(*token),
                Ok(DeviceTokenPoll::Pending) => {}
                Err(IntrospectionAPIError::Transport(err)) => {
                    tracing::debug!(error = %err, "device token poll failed; polling again");
                }
                Ok(DeviceTokenPoll::SlowDown) => {
                    interval = (interval + Duration::from_secs(SLOW_DOWN_BACKOFF_SECS))
                        .min(Duration::from_secs(MAX_INTERVAL_SECS));
                }
                Ok(DeviceTokenPoll::AccessDenied) => {
                    return Err(IntrospectionAPIError::Http {
                        message: "the login was denied in the browser".to_string(),
                        status: 400,
                        code: Some("access_denied".to_string()),
                        request_id: None,
                        body: None,
                        retry_after: None,
                    });
                }
                Ok(DeviceTokenPoll::Expired) => return Err(expired()),
                Err(err) => return Err(err),
            }
        }
    }

    /// `POST /v1/oauth/token` with the `refresh_token` grant: renew the
    /// login's access token.
    ///
    /// Keyed on the three values the login returned. The answer may rotate
    /// `refresh_token`; when it leaves it out, keep the one you have (RFC 6749
    /// §5.1). A refresh token that no longer works is a `400` whose
    /// [`IntrospectionAPIError::code`] is `invalid_grant`: sign in again.
    pub async fn refresh(
        &self,
        refresh_token: &str,
        session_id: &str,
        org_id: &str,
    ) -> ApiResult<OAuthToken> {
        let form = [
            ("grant_type", GRANT_REFRESH_TOKEN),
            ("refresh_token", refresh_token),
            ("client_id", self.client_id.as_str()),
            ("session_id", session_id),
            ("org_id", org_id),
        ];
        post_token_form_with(&self.http, &self.base_api_url, &form).await
    }

    /// `POST /v1/oauth/token` with the RFC 8693 token-exchange grant: trade a
    /// login's access token for the short-lived development credential
    /// `introspection dev` attaches with.
    ///
    /// Only a business member's login holding `runtimes:write` and scoped to
    /// a project qualifies. The answer carries `access_token` and
    /// `expires_in`; mint a new one when it expires. A login that does not
    /// qualify is a `400` (`unauthorized_client`, `invalid_target` or
    /// `invalid_grant`) whose `error_description` says why.
    pub async fn development_credential(&self, access_token: &str) -> ApiResult<OAuthToken> {
        let form = [
            ("grant_type", crate::auth::GRANT_TOKEN_EXCHANGE),
            ("client_id", self.client_id.as_str()),
            ("subject_token", access_token),
            ("subject_token_type", ACCESS_TOKEN_TYPE),
        ];
        post_token_form_with(&self.http, &self.base_api_url, &form).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interval_defaults_when_the_cp_omits_it() {
        let device: DeviceCodeResponse = serde_json::from_str(
            r#"{"device_code":"d","user_code":"ABCD-EFGH","verification_uri":"https://app/activate",
               "verification_uri_complete":"https://app/activate?user_code=ABCD-EFGH","expires_in":600}"#,
        )
        .unwrap();
        assert_eq!(device.interval, 5);
        assert!(!format!("{device:?}").contains("\"d\""));
    }
}
