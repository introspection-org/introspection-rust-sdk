//! Project-level append-only span annotations and managed labels.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use percent_encoding::{utf8_percent_encode, AsciiSet, CONTROLS};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::api::error::{ApiResult, IntrospectionAPIError};
use crate::api::http::HttpClient;
use crate::api::paginator::Paginator;
use crate::api::schemas::{Paginated, PaginationParams};

const PATH_SEGMENT_ENCODE_SET: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'#')
    .add(b'%')
    .add(b'/')
    .add(b'<')
    .add(b'>')
    .add(b'?')
    .add(b'`')
    .add(b'{')
    .add(b'}');

#[derive(Debug, Clone, Serialize)]
pub struct AnnotationTarget {
    pub trace_id: String,
    pub span_id: String,
}

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
    pub has_comment: bool,
    #[serde(default)]
    pub comment_count: u64,
    #[serde(default)]
    pub latest_comment: Option<String>,
    #[serde(default)]
    pub latest_comment_member_id: Option<Uuid>,
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
}

#[derive(Debug, Clone)]
pub enum AnnotationMutation {
    Labels(Vec<String>),
    Comment(String),
    ReviewerEmails(Vec<String>),
}

#[derive(Debug, Clone, Default)]
pub struct AnnotationEventOptions {
    pub event_id: Option<Uuid>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProjectLabel {
    pub slug: String,
    pub color: String,
    #[serde(default)]
    pub description: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProjectLabelCreate {
    pub slug: String,
    pub color: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProjectLabelUpdate {
    pub description: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ProjectLabelListParams {
    #[serde(flatten)]
    pub pagination: PaginationParams,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub search: Option<String>,
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
    cp_http: Arc<HttpClient>,
    dp_http: Arc<HttpClient>,
}

impl Annotations {
    pub fn new(cp_http: Arc<HttpClient>, dp_http: Arc<HttpClient>) -> Self {
        Self { cp_http, dp_http }
    }

    pub fn list(&self, params: &AnnotationListParams) -> Paginator<AnnotationState> {
        Paginator::new(self.dp_http.clone(), "/v1/annotations", params)
            .expect("AnnotationListParams must serialize to an object")
    }

    /// Append exactly one mutation. Label and reviewer vectors are complete snapshots.
    pub async fn create(
        &self,
        target: AnnotationTarget,
        mutation: AnnotationMutation,
        options: AnnotationEventOptions,
    ) -> ApiResult<()> {
        let mut body = serde_json::json!({
            "trace_id": target.trace_id,
            "span_id": target.span_id,
            "event_id": options.event_id.unwrap_or_else(Uuid::now_v7),
        });
        let object = body.as_object_mut().expect("annotation body is an object");
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
        }
        self.dp_http.post_json_empty("/v1/annotations", &body).await
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
            let page: Paginated<Member> = self
                .cp_http
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

#[derive(Clone)]
pub struct ProjectLabels {
    http: Arc<HttpClient>,
}

impl ProjectLabels {
    pub fn new(http: Arc<HttpClient>) -> Self {
        Self { http }
    }

    pub fn list(&self, params: &ProjectLabelListParams) -> Paginator<ProjectLabel> {
        Paginator::new(self.http.clone(), "/v1/project-labels", params)
            .expect("ProjectLabelListParams must serialize to an object")
    }

    pub async fn create(&self, mut input: ProjectLabelCreate) -> ApiResult<ProjectLabel> {
        input.slug = input.slug.trim().to_string();
        input.color = input.color.to_lowercase();
        validate_label(&input.slug, &input.color, input.description.as_deref())?;
        self.http.post_json("/v1/project-labels", &input).await
    }

    pub async fn get(&self, slug: &str) -> ApiResult<ProjectLabel> {
        #[derive(Serialize)]
        struct Q {}
        let slug = utf8_percent_encode(slug, PATH_SEGMENT_ENCODE_SET);
        self.http
            .get_json(&format!("/v1/project-labels/{slug}"), &Q {})
            .await
    }

    pub async fn update(&self, slug: &str, input: ProjectLabelUpdate) -> ApiResult<ProjectLabel> {
        if input
            .description
            .as_ref()
            .is_some_and(|value| value.len() > 2000)
        {
            return Err(validation(
                "Project label description must not exceed 2000 characters",
                "invalid_project_label_description",
            ));
        }
        let slug = utf8_percent_encode(slug, PATH_SEGMENT_ENCODE_SET);
        self.http
            .patch_json(&format!("/v1/project-labels/{slug}"), &input)
            .await
    }
}

fn validate_label(slug: &str, color: &str, description: Option<&str>) -> ApiResult<()> {
    if slug.is_empty() || slug.len() > 128 {
        return Err(validation(
            "Project label slug must contain 1 to 128 characters",
            "invalid_project_label_slug",
        ));
    }
    let valid_color = color.len() == 7
        && color.starts_with('#')
        && color[1..].bytes().all(|byte| byte.is_ascii_hexdigit());
    if !valid_color {
        return Err(validation(
            "Project label color must be a six-digit hex color such as #f97316",
            "invalid_project_label_color",
        ));
    }
    if description.is_some_and(|value| value.len() > 2000) {
        return Err(validation(
            "Project label description must not exceed 2000 characters",
            "invalid_project_label_description",
        ));
    }
    Ok(())
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
