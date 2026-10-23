//! Setup shared by the `repositories()` tests against a mock Data Plane.
//!
//! The Control Plane is a separate mock with nothing mounted, so a request
//! that went to it instead of the Data Plane would 404.

use introspection_sdk::{AdvancedOptions, ClientConfig, IntrospectionClient};
use uuid::Uuid;
use wiremock::MockServer;

pub const REPOSITORY_ID: &str = "22222222-2222-7222-8222-222222222222";

pub fn repository_id() -> Uuid {
    Uuid::parse_str(REPOSITORY_ID).unwrap()
}

/// A client whose Data Plane is `dp`, and the empty Control Plane mock.
pub async fn client(dp: &MockServer) -> (IntrospectionClient, MockServer) {
    let cp = MockServer::start().await;
    let client = IntrospectionClient::new(ClientConfig::with_token("intro_test").advanced(
        AdvancedOptions {
            base_api_url: Some(cp.uri()),
            dp_url: Some(dp.uri()),
            ..Default::default()
        },
    ))
    .unwrap();
    (client, cp)
}
