//! `client.members` (CP) — the org's members and the labels on them.
//!
//! A member is any principal in the org: a business user, an agent, or an end
//! customer. Customer members are usually minted by asserting a
//! [`RunnerIdentity`](crate::RunnerIdentity) rather than created here, so the
//! integrator-facing work on this namespace is reading them back and
//! labelling them: `tags` (access-bearing) and `metadata` (grants nothing,
//! filter-only).
//!
//! These are Control Plane routes, gated on the `members:read`,
//! `members:write` and `members:manage` scopes, so they need an org
//! credential that carries them.

use std::sync::Arc;

use serde::Serialize;
use uuid::Uuid;

use crate::api::error::ApiResult;
use crate::api::http::HttpClient;
use crate::api::paginator::Paginator;
use crate::api::schemas::{Member, MemberCreateParams, MemberListParams, MemberUpdateParams};

/// `client.members` namespace. Holds a CP-bound HTTP client.
#[derive(Clone)]
pub struct Members {
    http: Arc<HttpClient>,
}

impl Members {
    #[doc(hidden)]
    pub fn new(http: Arc<HttpClient>) -> Self {
        Self { http }
    }

    /// `GET /v1/members` — paginated. `tag` and `metadata` only ever narrow.
    pub fn list(&self, params: &MemberListParams) -> Paginator<Member> {
        Paginator::new(self.http.clone(), "/v1/members", params)
            .expect("MemberListParams must serialize to a JSON object")
    }

    /// `GET /v1/members/{id}`.
    pub async fn get(&self, member_id: Uuid) -> ApiResult<Member> {
        #[derive(Serialize)]
        struct Q {}
        let path = format!("/v1/members/{}", member_id);
        self.http.get_json(&path, &Q {}).await
    }

    /// `POST /v1/members` — invite a business member by email.
    ///
    /// Requires an admin or owner. Seeding `tags` additionally requires
    /// `members:manage`, since a tag grants access.
    pub async fn create(&self, params: &MemberCreateParams) -> ApiResult<Member> {
        self.http.post_json("/v1/members", params).await
    }

    /// `PATCH /v1/members/{id}` — partial update; requires `members:manage`.
    ///
    /// `tags` and `metadata` each replace the whole value when set and are
    /// left untouched when `None`. An agent member answers 409.
    pub async fn update(&self, member_id: Uuid, params: &MemberUpdateParams) -> ApiResult<Member> {
        let path = format!("/v1/members/{}", member_id);
        self.http.patch_json(&path, params).await
    }
}
