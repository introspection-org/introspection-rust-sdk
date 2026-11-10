//! Project-level append-only span annotations.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use bytes::Bytes;
use futures::{Stream, StreamExt};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::api::error::{ApiResult, IntrospectionAPIError};
use crate::api::http::{decode_json, HttpClient};
use crate::api::paginator::Paginator;
use crate::api::schemas::{Paginated, PaginationParams};
use crate::api::telemetry::{parse_json_lines, JSON_LINES_ACCEPT};

const EXPORT_PATH: &str = "/v1/annotations/export";

#[derive(Debug, Clone, Serialize)]
pub struct AnnotationTarget {
    pub trace_id: String,
    pub span_id: String,
}

/// One comment on an annotated span.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct AnnotationComment {
    pub event_id: Uuid,
    pub comment: String,
    /// The author.
    pub member_id: Uuid,
    #[serde(default)]
    pub actor_member_id: Option<Uuid>,
    /// What wrote it on the author's behalf (`agent`, ...).
    #[serde(default)]
    pub actor_member_type: Option<String>,
    pub created_at: String,
}

/// One span's current annotation state.
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
    /// The members whose comment completed a review. A Data Plane that
    /// predates the rename sends it as `human_commenter_member_ids`.
    #[serde(default, alias = "human_commenter_member_ids")]
    pub reviewer_member_ids: Vec<Uuid>,
    #[serde(default)]
    pub comment_count: u64,
    pub updated_at: String,
    pub updated_by_member_id: Uuid,
    #[serde(default)]
    pub assignment_event_id: Option<Uuid>,
    /// Every comment on the span, oldest first, on a row of
    /// [`Annotations::export`]; `None` on the list and on navigation.
    #[serde(default)]
    pub comments: Option<Vec<AnnotationComment>>,
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

/// The filters of `GET /v1/annotations/export`: those of the list, with no
/// page to ask for.
#[derive(Debug, Clone, Default, Serialize)]
pub struct AnnotationExportParams {
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
    /// Every label a row must carry, sent as a repeated `label`.
    #[serde(rename = "label", skip_serializing_if = "Vec::is_empty")]
    pub labels: Vec<String>,
    /// `backlog`, `assigned` or `reviewed`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    /// The annotator as an email, resolved as on [`AnnotationListParams`].
    #[serde(skip)]
    pub annotated_by_email: Option<String>,
    /// The assignee as an email, resolved like `annotated_by_email`.
    #[serde(skip)]
    pub assigned_to_email: Option<String>,
}

/// One filter bucket of `GET /v1/annotations/facets`: how many annotated
/// spans currently carry `value` along `dimension` (`annotator`,
/// `assignee`, `label`, `status` or `review_queue`).
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct AnnotationFacet {
    pub dimension: String,
    pub value: String,
    pub count: u64,
    /// Set, with the three fields after it, on the `label` and
    /// `review_queue` buckets, which summarize a dataset.
    #[serde(default)]
    pub conversation_count: Option<u64>,
    #[serde(default)]
    pub comment_count: Option<u64>,
    #[serde(default)]
    pub reviewer_member_ids: Option<Vec<Uuid>>,
    #[serde(default)]
    pub updated_at: Option<String>,
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

#[derive(Debug, Deserialize, Serialize)]
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
        params.annotated_by_member_id = self
            .member_filter(
                params.annotated_by_member_id,
                params.annotated_by_email.take(),
                "Use annotated_by_member_id or annotated_by_email, not both",
                "conflicting_annotation_annotator_filters",
            )
            .await?;
        params.assignee_member_id = self
            .member_filter(
                params.assignee_member_id,
                params.assigned_to_email.take(),
                "Use assignee_member_id or assigned_to_email, not both",
                "conflicting_annotation_assignee_filters",
            )
            .await?;
        Ok(
            Paginator::new(self.dp_http.clone(), "/v1/annotations", &params)
                .expect("AnnotationListParams must serialize to an object"),
        )
    }

    /// `GET /v1/annotations/export` — every row the list returns for these
    /// filters, unpaginated, each decoded as its line arrives. An email
    /// filter costs the same member lookup as in [`Self::list`]. A Data
    /// Plane that predates the route answers `404` or `406`.
    pub async fn export(
        &self,
        params: &AnnotationExportParams,
    ) -> ApiResult<impl Stream<Item = ApiResult<AnnotationState>>> {
        let response = self.export_response(params).await?;
        Ok(parse_json_lines(
            response.bytes_stream(),
            EXPORT_PATH.to_string(),
        ))
    }

    /// `GET /v1/annotations/export` as the JSON-lines bytes the server sent,
    /// each chunk passed through as it arrives.
    pub async fn export_stream(
        &self,
        params: &AnnotationExportParams,
    ) -> ApiResult<impl Stream<Item = ApiResult<Bytes>>> {
        let response = self.export_response(params).await?;
        Ok(response
            .bytes_stream()
            .map(|chunk| chunk.map_err(IntrospectionAPIError::from)))
    }

    async fn export_response(
        &self,
        params: &AnnotationExportParams,
    ) -> ApiResult<reqwest::Response> {
        let mut params = params.clone();
        params.annotated_by_member_id = self
            .member_filter(
                params.annotated_by_member_id,
                params.annotated_by_email.take(),
                "Use annotated_by_member_id or annotated_by_email, not both",
                "conflicting_annotation_annotator_filters",
            )
            .await?;
        params.assignee_member_id = self
            .member_filter(
                params.assignee_member_id,
                params.assigned_to_email.take(),
                "Use assignee_member_id or assigned_to_email, not both",
                "conflicting_annotation_assignee_filters",
            )
            .await?;
        self.dp_http
            .get_raw(EXPORT_PATH, &params, Some(JSON_LINES_ACCEPT))
            .await
    }

    /// A member filter given as an id or as an email, as the id.
    async fn member_filter(
        &self,
        id: Option<Uuid>,
        email: Option<String>,
        conflict: &str,
        code: &str,
    ) -> ApiResult<Option<Uuid>> {
        let Some(email) = email else {
            return Ok(id);
        };
        if id.is_some() {
            return Err(validation(conflict, code));
        }
        Ok(Some(self.resolve_reviewer_ids(vec![email]).await?[0]))
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

#[cfg(test)]
mod tests {
    use super::*;
    use futures::TryStreamExt;

    #[tokio::test]
    async fn export_rows_join_a_line_split_across_chunks() {
        let chunks = [
            "{\"trace_id\":\"t\",\"span_id\":\"a\",\"updated_at\":\"2026-08-25T12:00:00Z\",\"updated_by_member_id\":\"00000000-0000-0000-0000-0000000000cc\"}\n{\"trace_id\":\"t\",\"span",
            "_id\":\"b\",\"labels\":[\"x\"],\"updated_",
            "at\":\"2026-08-25T12:00:00Z\",\"updated_by_member_id\":\"00000000-0000-0000-0000-0000000000cc\"}\n",
        ]
        .map(|chunk| Ok::<_, reqwest::Error>(Bytes::from_static(chunk.as_bytes())));

        let rows: Vec<AnnotationState> =
            parse_json_lines(futures::stream::iter(chunks), EXPORT_PATH.to_string())
                .try_collect()
                .await
                .unwrap();

        let spans: Vec<_> = rows.iter().map(|row| row.span_id.as_str()).collect();
        assert_eq!(spans, ["a", "b"]);
        assert_eq!(rows[1].labels, ["x"]);
    }
}
