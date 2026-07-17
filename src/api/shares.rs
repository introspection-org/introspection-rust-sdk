//! Runner-bound read-sharing grants for files and conversations.

use std::sync::Arc;

use crate::api::error::ApiResult;
use crate::api::http::HttpClient;
use crate::api::paginator::Paginator;
use crate::api::schemas::{ResourceShare, ShareCreate, ShareListParams};

#[derive(Clone)]
pub struct Shares {
    http: Arc<HttpClient>,
}

impl Shares {
    #[doc(hidden)]
    pub fn new(http: Arc<HttpClient>) -> Self {
        Self { http }
    }

    pub fn list(&self, params: &ShareListParams) -> Paginator<ResourceShare> {
        Paginator::new(self.http.clone(), "/v1/shares", params)
            .expect("ShareListParams must serialize to a JSON object")
    }

    pub async fn create(&self, body: &ShareCreate) -> ApiResult<ResourceShare> {
        self.http.post_json("/v1/shares", body).await
    }

    pub async fn get(&self, share_id: &str) -> ApiResult<ResourceShare> {
        let path = format!("/v1/shares/{}", urlencode(share_id));
        self.http.get_json(&path, &()).await
    }

    pub async fn delete(&self, share_id: &str) -> ApiResult<()> {
        let path = format!("/v1/shares/{}", urlencode(share_id));
        self.http.delete_empty(&path).await
    }
}

fn urlencode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}
