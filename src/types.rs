//! REST-only type definitions for the Introspection SDK.
//!
//! All OpenTelemetry-flavoured types (`TrackOptions`, `FeedbackOptions`,
//! `IdentifyOptions`, `PropertyValue`, attribute/baggage constants,
//! `generate_event_id`, etc.) live in `crate::otel::types` and are
//! gated behind the `otel` Cargo feature.

use std::collections::HashMap;

/// Advanced options for the REST [`crate::IntrospectionClient`].
///
/// Use this to customise headers, override the DP REST API host, or
/// tweak HTTP behaviour. The OpenTelemetry surfaces
/// (`crate::otel::IntrospectionLogs` and
/// `crate::otel::IntrospectionSpanProcessor`) have their own
/// independent configuration types.
///
/// # Example
///
/// ```rust,no_run
/// use introspection_sdk::{AdvancedOptions, ClientConfig, IntrospectionClient};
///
/// let client = IntrospectionClient::new(
///     ClientConfig::builder()
///         .token("your-token")
///         .advanced(AdvancedOptions {
///             base_api_url: Some("http://localhost:8080".to_string()),
///             additional_headers: Some(
///                 [("X-Custom-Header".to_string(), "value".to_string())]
///                     .into_iter()
///                     .collect(),
///             ),
///             ..Default::default()
///         })
///         .build()
///         .unwrap(),
/// )
/// .unwrap();
/// ```
#[derive(Clone, Debug, Default)]
pub struct AdvancedOptions {
    /// Base URL for the Introspection DP REST API (`/v1/tasks`, `/v1/files`).
    ///
    /// If not provided, uses `INTROSPECTION_BASE_API_URL` or default.
    pub base_api_url: Option<String>,

    /// Additional HTTP headers to include in requests.
    pub additional_headers: Option<HashMap<String, String>>,

    /// Enable debug logging
    pub debug: bool,
}

/// Configuration options for the REST [`crate::IntrospectionClient`].
///
/// # Example
///
/// ```rust
/// use introspection_sdk::ClientConfig;
///
/// let config = ClientConfig::builder()
///     .token("your-token")
///     .build()
///     .unwrap();
/// ```
#[derive(Debug, Clone, Default, derive_builder::Builder)]
#[builder(setter(into, strip_option), default)]
pub struct ClientConfig {
    /// Authentication token (env: `INTROSPECTION_TOKEN`).
    #[builder(setter(into))]
    pub token: Option<String>,

    /// Advanced REST options.
    #[builder(setter(into, strip_option), default)]
    pub advanced: Option<AdvancedOptions>,
}

impl ClientConfig {
    /// Create a new builder for ClientConfig.
    pub fn builder() -> ClientConfigBuilder {
        ClientConfigBuilder::default()
    }

    /// Create a ClientConfig with just a token.
    pub fn with_token(token: impl Into<String>) -> Self {
        Self {
            token: Some(token.into()),
            ..Default::default()
        }
    }

    /// Set advanced options.
    pub fn advanced(mut self, advanced: AdvancedOptions) -> Self {
        self.advanced = Some(advanced);
        self
    }
}

/// Default configuration values shared across the REST surface.
pub mod defaults {
    pub const SERVICE_NAME: &str = "introspection-client";
    /// Default DP REST API base URL (used by `client.tasks` / `client.files`).
    pub const BASE_API_URL: &str = "https://api.introspection.dev";
    /// Default HTTP timeout for REST API calls.
    pub const API_TIMEOUT_SECS: u64 = 30;
    /// Default number of automatic retries on a `429 Too Many Requests`
    /// response for unary REST calls (honouring `Retry-After`). `0` disables
    /// retrying. Streaming has its own resume budget (see
    /// [`crate::api::resumable`]).
    pub const API_MAX_RETRIES: u32 = 2;
    /// Default base step (ms) of the capped-exponential `429` retry backoff.
    pub const API_RETRY_BASE_MS: u64 = 500;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_config_builder() {
        let config = ClientConfig::builder().token("test-token").build().unwrap();

        assert_eq!(config.token, Some("test-token".to_string()));
    }
}
