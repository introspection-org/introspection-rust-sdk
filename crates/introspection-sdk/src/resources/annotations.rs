//! Project-level append-only span annotations.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::api::error::{ApiResult, IntrospectionAPIError};
use crate::api::http::{decode_json, HttpClient};
use crate::api::paginator::Paginator;
use crate::api::schemas::{Paginated, PaginationParams};

#[derive(Debug, Clone, Serialize)]
pub struct AnnotationTarget {
    pub trace_id: String,
    pub span_id: String,
}

#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AnnotationState {
    pub trace_id: String,
    pub span_id: String,
    #[serde(default)]
    pub conversation_id: Option<String>,
    #[serde(default)]
    pub labels: Vec<String>,
    #[serde(default)]
    pub assignee_member_ids: Vec<Uuid>,
    #[serde(default)]
    pub annotator_member_ids: Vec<Uuid>,
    #[serde(default)]
    pub commenter_member_ids: Vec<Uuid>,
    /// The commenters who are people, not agents.
    #[serde(default)]
    pub human_commenter_member_ids: Vec<Uuid>,
    #[serde(default)]
    pub has_comment: bool,
    #[serde(default)]
    pub has_human_comment: bool,
    #[serde(default)]
    pub comment_count: u64,
    #[serde(default)]
    pub latest_comment: Option<String>,
    #[serde(default)]
    pub latest_comment_member_id: Option<Uuid>,
    #[serde(default)]
    pub latest_comment_actor_member_id: Option<Uuid>,
    /// Who wrote the latest comment (`human`, `agent`, ...).
    #[serde(default)]
    pub latest_comment_actor_type: Option<String>,
    pub updated_at: String,
    pub updated_by_member_id: Uuid,
    #[serde(default)]
    pub assignment_event_id: Option<Uuid>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct AnnotationListParams {
    #[serde(flatten)]
    pub pagination: PaginationParams,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include_total: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotated_by_member_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assignee_member_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trace_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub span_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// The annotator as an active Business member's email, resolved to
    /// `annotated_by_member_id` on the Control Plane before the read; never
    /// on the wire. Setting both is refused.
    #[serde(skip)]
    pub annotated_by_email: Option<String>,
    /// The assignee as an email, resolved like `annotated_by_email`.
    #[serde(skip)]
    pub assigned_to_email: Option<String>,
    /// Escape hatch for a filter this SDK build predates: each pair is merged
    /// verbatim onto the query string (a string, bool or number as itself, an
    /// array as a repeated key; a null is dropped, an object is refused).
    /// Prefer the typed field where one exists; on a collision the
    /// passthrough wins.
    #[serde(flatten)]
    pub filters: Option<HashMap<String, serde_json::Value>>,
}

/// One filter bucket of `GET /v1/annotations/facets`: how many annotated
/// spans currently carry `value` along `dimension` (`annotator`,
/// `assignee`, `label` or `status`).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct AnnotationFacet {
    pub dimension: String,
    pub value: String,
    pub count: u64,
}

/// What `GET /v1/annotations/navigation` answers: the annotation at the
/// target with its neighbours in the filtered review order.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AnnotationNavigation {
    #[serde(default)]
    pub previous: Option<AnnotationState>,
    #[serde(default)]
    pub current: Option<AnnotationState>,
    #[serde(default)]
    pub next: Option<AnnotationState>,
    /// The target's 1-based place in the filtered order; `None` when the
    /// filters exclude it.
    #[serde(default)]
    pub current_position: Option<u64>,
    #[serde(default)]
    pub total_count: u64,
}

/// The review order `GET /v1/annotations/navigation` walks: the same filters
/// as [`AnnotationListParams`], by member id.
#[derive(Debug, Clone, Default, Serialize)]
pub struct AnnotationNavigationParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotated_by_member_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assignee_member_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// `backlog`, `assigned` or `reviewed`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
}

#[derive(Debug, Clone)]
pub enum AnnotationMutation {
    Labels(Vec<String>),
    Comment(String),
    /// A reviewer snapshot by email, resolved to member ids on the Control
    /// Plane first.
    ReviewerEmails(Vec<String>),
    /// A reviewer snapshot by member id, sent as is: no Control Plane lookup.
    ReviewerIds(Vec<Uuid>),
}

/// What `POST /v1/annotations` answers: the event's id, and whether the
/// event is already readable. `visible: false` is the server's `202`: the
/// write was accepted but did not reach the read side within its wait, so a
/// read right after may not show it yet.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct AnnotationCreated {
    pub event_id: Uuid,
    pub visible: bool,
}

#[derive(Debug, Clone, Default)]
pub struct AnnotationEventOptions {
    /// The event's UUIDv7; generated when `None`. The server deduplicates on
    /// it, so a retried create is applied once.
    pub event_id: Option<Uuid>,
    /// A comment sent in the same event as a [`AnnotationMutation::Labels`],
    /// [`AnnotationMutation::ReviewerEmails`] or
    /// [`AnnotationMutation::ReviewerIds`] snapshot. A comment appends, so
    /// it may ride along with either; with [`AnnotationMutation::Comment`] it
    /// is refused, since an event holds one comment.
    pub comment: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Member {
    id: Uuid,
    email: Option<String>,
    #[serde(default)]
    is_deactivated: bool,
}

#[derive(Clone)]
pub struct Annotations {
    cp_http: Option<Arc<HttpClient>>,
    dp_http: Arc<HttpClient>,
}

impl Annotations {
    pub fn new(cp_http: Arc<HttpClient>, dp_http: Arc<HttpClient>) -> Self {
        Self {
            cp_http: Some(cp_http),
            dp_http,
        }
    }

    /// Annotations over a Data Plane client alone. Every call that names
    /// members by id works; an email (a reviewer snapshot or a list filter)
    /// needs the Control Plane member list and is refused with
    /// [`IntrospectionAPIError::InvalidConfig`] before any request.
    pub fn data_plane(dp_http: Arc<HttpClient>) -> Self {
        Self {
            cp_http: None,
            dp_http,
        }
    }

    /// `GET /v1/annotations` — the folded annotation-state paginator. An
    /// email filter on `params` costs one Control Plane member lookup first,
    /// which is why the read is `async` before the page is fetched.
    pub async fn list(
        &self,
        params: &AnnotationListParams,
    ) -> ApiResult<Paginator<AnnotationState>> {
        let mut params = params.clone();
        if params.annotated_by_member_id.is_some() && params.annotated_by_email.is_some() {
            return Err(validation(
                "Use annotated_by_member_id or annotated_by_email, not both",
                "conflicting_annotation_annotator_filters",
            ));
        }
        if params.assignee_member_id.is_some() && params.assigned_to_email.is_some() {
            return Err(validation(
                "Use assignee_member_id or assigned_to_email, not both",
                "conflicting_annotation_assignee_filters",
            ));
        }
        if let Some(email) = params.annotated_by_email.take() {
            params.annotated_by_member_id = Some(self.resolve_reviewer_ids(vec![email]).await?[0]);
        }
        if let Some(email) = params.assigned_to_email.take() {
            params.assignee_member_id = Some(self.resolve_reviewer_ids(vec![email]).await?[0]);
        }
        Ok(
            Paginator::new(self.dp_http.clone(), "/v1/annotations", &params)
                .expect("AnnotationListParams must serialize to an object"),
        )
    }

    /// `POST /v1/annotations` — append one event: a mutation, plus
    /// `options.comment` alongside a snapshot. Label and reviewer vectors are
    /// complete snapshots. The event carries its id, so a `502` / `503` /
    /// `504` is retried with the same id and applied at most once.
    pub async fn create(
        &self,
        target: AnnotationTarget,
        mutation: AnnotationMutation,
        options: AnnotationEventOptions,
    ) -> ApiResult<AnnotationCreated> {
        let event_id = options.event_id.unwrap_or_else(Uuid::now_v7);
        let mut body = serde_json::json!({
            "trace_id": target.trace_id,
            "span_id": target.span_id,
            "event_id": event_id,
        });
        let object = body.as_object_mut().expect("annotation body is an object");
        if let Some(comment) = options.comment {
            if matches!(mutation, AnnotationMutation::Comment(_)) {
                return Err(validation(
                    "Send one comment per event: use AnnotationMutation::Comment or options.comment, not both",
                    "conflicting_annotation_comments",
                ));
            }
            object.insert("comment".into(), serde_json::Value::String(comment));
        }
        match mutation {
            AnnotationMutation::Labels(labels) => {
                object.insert("labels".into(), serde_json::to_value(labels).unwrap());
            }
            AnnotationMutation::Comment(comment) => {
                object.insert("comment".into(), serde_json::Value::String(comment));
            }
            AnnotationMutation::ReviewerEmails(emails) => {
                let ids = if emails.is_empty() {
                    Vec::new()
                } else {
                    self.resolve_reviewer_ids(emails).await?
                };
                object.insert(
                    "assignee_member_ids".into(),
                    serde_json::to_value(ids).unwrap(),
                );
            }
            AnnotationMutation::ReviewerIds(ids) => {
                object.insert(
                    "assignee_member_ids".into(),
                    serde_json::to_value(ids).unwrap(),
                );
            }
        }
        let res = self
            .dp_http
            .post_json_idempotent_response("/v1/annotations", &body)
            .await?;
        if res.status() == reqwest::StatusCode::NO_CONTENT {
            // A 204 has no body; the id rides in a header, and is the one sent.
            let event_id = res
                .headers()
                .get("x-introspection-event-id")
                .and_then(|value| value.to_str().ok())
                .and_then(|value| Uuid::parse_str(value).ok())
                .unwrap_or(event_id);
            return Ok(AnnotationCreated {
                event_id,
                visible: true,
            });
        }
        decode_json(res).await
    }

    /// `GET /v1/annotations/facets` — the current annotator, assignee,
    /// label and status counts a review screen offers as filters.
    pub async fn facets(&self) -> ApiResult<Vec<AnnotationFacet>> {
        self.dp_http.get_json("/v1/annotations/facets", &()).await
    }

    /// `GET /v1/annotations/navigation` — the annotation on `target` with the
    /// previous and next one in the review order `params` filters.
    pub async fn navigation(
        &self,
        target: &AnnotationTarget,
        params: &AnnotationNavigationParams,
    ) -> ApiResult<AnnotationNavigation> {
        #[derive(Serialize)]
        struct Q<'a> {
            trace_id: &'a str,
            span_id: &'a str,
            #[serde(flatten)]
            params: &'a AnnotationNavigationParams,
        }
        let query = Q {
            trace_id: &target.trace_id,
            span_id: &target.span_id,
            params,
        };
        self.dp_http
            .get_json("/v1/annotations/navigation", &query)
            .await
    }

    async fn resolve_reviewer_ids(&self, emails: Vec<String>) -> ApiResult<Vec<Uuid>> {
        let mut requested = Vec::new();
        let mut seen = HashSet::new();
        for value in emails {
            let email = value.trim().to_lowercase();
            if email.is_empty() {
                return Err(validation(
                    "At least one non-empty reviewer email is required",
                    "invalid_annotation_reviewer_email",
                ));
            }
            if seen.insert(email.clone()) {
                requested.push(email);
            }
        }
        if requested.len() > 64 {
            return Err(validation(
                "At most 64 reviewer emails are allowed",
                "too_many_annotation_reviewers",
            ));
        }
        let Some(cp_http) = self.cp_http.as_ref() else {
            return Err(IntrospectionAPIError::InvalidConfig(
                "Resolving a member email needs a Control Plane client; \
                 pass member ids, or use Annotations::new"
                    .to_string(),
            ));
        };
        let mut matches: HashMap<String, Vec<Uuid>> = requested
            .iter()
            .map(|email| (email.clone(), Vec::new()))
            .collect();
        let mut next: Option<String> = None;
        loop {
            #[derive(Serialize)]
            struct Query<'a> {
                limit: u32,
                member_type: &'a str,
                #[serde(skip_serializing_if = "Option::is_none")]
                next: Option<&'a str>,
            }
            let page: Paginated<Member> = cp_http
                .get_json(
                    "/v1/members",
                    &Query {
                        limit: 1000,
                        member_type: "business",
                        next: next.as_deref(),
                    },
                )
                .await?;
            for member in page.records {
                if member.is_deactivated {
                    continue;
                }
                if let Some(email) = member.email.map(|value| value.trim().to_lowercase()) {
                    if let Some(ids) = matches.get_mut(&email) {
                        ids.push(member.id);
                    }
                }
            }
            next = page.next.filter(|value| !value.is_empty());
            if next.is_none() {
                break;
            }
        }
        let mut ids = Vec::new();
        for email in requested {
            match &matches[&email][..] {
                [] => {
                    return Err(http_error(
                        404,
                        format!("No active domain expert found for '{email}'"),
                        "annotation_reviewer_not_found",
                        serde_json::json!({"email": email}),
                    ))
                }
                [id] => ids.push(*id),
                many => {
                    return Err(http_error(
                        409,
                        format!("Multiple active domain experts found for '{email}'"),
                        "annotation_reviewer_ambiguous",
                        serde_json::json!({"email": email, "member_ids": many}),
                    ))
                }
            }
        }
        Ok(ids)
    }
}

fn validation(message: &str, code: &str) -> IntrospectionAPIError {
    http_error(422, message.to_string(), code, serde_json::Value::Null)
}

fn http_error(
    status: u16,
    message: String,
    code: &str,
    body: serde_json::Value,
) -> IntrospectionAPIError {
    IntrospectionAPIError::Http {
        message,
        status,
        code: Some(code.to_string()),
        request_id: None,
        body: Some(body),
        retry_after: None,
    }
}
