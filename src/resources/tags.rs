//! The project tag catalogue: the values a share, a member tag, or an
//! annotation carries.
//!
//! A catalogue row names a tag people pick from; it grants nothing. The two
//! kinds are separate catalogues with separate scopes, and their fields differ
//! — an `access` tag is an opaque value with no colour, an `annotation` tag is
//! a slug that must have one. [`TagCreate`] is an enum per kind so that rule
//! is unrepresentable rather than validated.

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::api::encoding::encode;
use crate::api::error::{ApiResult, IntrospectionAPIError};
use crate::api::http::HttpClient;
use crate::api::paginator::Paginator;
use crate::api::schemas::PaginationParams;

const MAX_NAME: usize = 128;
const MAX_DESCRIPTION: usize = 2000;

/// Which catalogue a tag belongs to. `access` tags are the values tag shares
/// and member tags carry; `annotation` tags are the annotation labels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TagKind {
    Access,
    Annotation,
}

impl TagKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Access => "access",
            Self::Annotation => "annotation",
        }
    }
}

impl fmt::Display for TagKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Tag {
    pub kind: TagKind,
    pub name: String,
    /// Set on an `annotation` tag; never on an `access` tag.
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// A tag to add. The kind picks the variant, so an `access` tag has no colour
/// to set and an `annotation` tag cannot omit one.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TagCreate {
    Access(AccessTagCreate),
    Annotation(AnnotationTagCreate),
}

/// An access tag's name is opaque and kept verbatim, so it may contain `/`.
#[derive(Debug, Clone, Default, Serialize)]
pub struct AccessTagCreate {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct AnnotationTagCreate {
    /// Slugified by the API, so a display name is accepted here.
    pub name: String,
    /// A six-digit hex colour such as `#f97316`.
    pub color: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

impl TagCreate {
    /// An access tag, named by the opaque value shares and members carry.
    /// Combine with [`AccessTagCreate`]'s struct-update syntax for a
    /// description.
    pub fn access(name: impl Into<String>) -> Self {
        Self::Access(AccessTagCreate {
            name: name.into(),
            description: None,
        })
    }

    /// An annotation tag, which needs a colour.
    pub fn annotation(name: impl Into<String>, color: impl Into<String>) -> Self {
        Self::Annotation(AnnotationTagCreate {
            name: name.into(),
            color: color.into(),
            description: None,
        })
    }

    pub fn kind(&self) -> TagKind {
        match self {
            Self::Access(_) => TagKind::Access,
            Self::Annotation(_) => TagKind::Annotation,
        }
    }
}

/// `None` leaves a field as it is. `Some(None)` on `description` clears it,
/// which is why it nests: the API distinguishes an omitted field from a null.
#[derive(Debug, Clone, Default, Serialize)]
pub struct TagUpdate {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<Option<String>>,
}

impl TagUpdate {
    pub fn color(color: impl Into<String>) -> Self {
        Self {
            color: Some(color.into()),
            description: None,
        }
    }

    pub fn description(description: impl Into<String>) -> Self {
        Self {
            color: None,
            description: Some(Some(description.into())),
        }
    }

    /// Clear the description.
    pub fn clear_description() -> Self {
        Self {
            color: None,
            description: Some(None),
        }
    }
}

/// `kind` is required: the two kinds are separate catalogues, and the API has
/// no listing that spans them.
#[derive(Debug, Clone, Serialize)]
pub struct TagListParams {
    pub kind: TagKind,
    #[serde(flatten)]
    pub pagination: PaginationParams,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub search: Option<String>,
    /// Escape hatch for a filter this SDK build predates: each pair is merged
    /// verbatim onto the query string (a string, bool or number as itself, an
    /// array as a repeated key; a null is dropped, an object is refused).
    /// Prefer the typed field where one exists; on a collision the
    /// passthrough wins.
    #[serde(flatten)]
    pub filters: Option<HashMap<String, serde_json::Value>>,
}

impl TagListParams {
    pub fn new(kind: TagKind) -> Self {
        Self {
            kind,
            pagination: PaginationParams::default(),
            search: None,
            filters: None,
        }
    }
}

#[derive(Clone)]
pub struct Tags {
    http: Arc<HttpClient>,
}

impl Tags {
    pub fn new(http: Arc<HttpClient>) -> Self {
        Self { http }
    }

    pub fn list(&self, params: &TagListParams) -> Paginator<Tag> {
        Paginator::new(self.http.clone(), "/v1/tags", params)
            .expect("TagListParams must serialize to an object")
    }

    /// Add a tag. An existing one comes back as it is, and a deleted one is
    /// revived.
    pub async fn create(&self, input: TagCreate) -> ApiResult<Tag> {
        let input = normalize(input)?;
        self.http.post_json("/v1/tags", &input).await
    }

    pub async fn get(&self, kind: TagKind, name: &str) -> ApiResult<Tag> {
        #[derive(Serialize)]
        struct Q {}
        self.http.get_json(&path(kind, name), &Q {}).await
    }

    pub async fn update(&self, kind: TagKind, name: &str, input: TagUpdate) -> ApiResult<Tag> {
        if input.color.is_none() && input.description.is_none() {
            return Err(validation(
                "A tag update must name a color or a description",
                "invalid_tag_update",
            ));
        }
        if kind == TagKind::Access && input.color.is_some() {
            return Err(validation(
                "An access tag has no color",
                "invalid_tag_color",
            ));
        }
        let input = TagUpdate {
            color: input.color.map(|color| color.to_lowercase()),
            description: input.description,
        };
        if let Some(Some(description)) = input.description.as_ref() {
            check_description(description)?;
        }
        if let Some(color) = input.color.as_deref() {
            check_color(color)?;
        }
        self.http.patch_json(&path(kind, name), &input).await
    }

    /// Remove a tag from the catalogue. Shares, member tags and annotations
    /// keep theirs.
    pub async fn delete(&self, kind: TagKind, name: &str) -> ApiResult<()> {
        self.http.delete_empty(&path(kind, name)).await
    }
}

/// An access tag's name may contain `/`, so it is encoded even though the API
/// matches it as a path.
fn path(kind: TagKind, name: &str) -> String {
    format!("/v1/tags/{}/{}", kind.as_str(), encode(name))
}

fn normalize(input: TagCreate) -> ApiResult<TagCreate> {
    match input {
        TagCreate::Access(tag) => {
            let name = tag.name.trim().to_string();
            check_name(&name)?;
            if let Some(description) = tag.description.as_deref() {
                check_description(description)?;
            }
            Ok(TagCreate::Access(AccessTagCreate { name, ..tag }))
        }
        TagCreate::Annotation(tag) => {
            let name = tag.name.trim().to_string();
            let color = tag.color.to_lowercase();
            check_name(&name)?;
            check_color(&color)?;
            if let Some(description) = tag.description.as_deref() {
                check_description(description)?;
            }
            Ok(TagCreate::Annotation(AnnotationTagCreate {
                name,
                color,
                ..tag
            }))
        }
    }
}

fn check_name(name: &str) -> ApiResult<()> {
    if name.is_empty() || name.len() > MAX_NAME {
        return Err(validation(
            "A tag name must contain 1 to 128 characters",
            "invalid_tag_name",
        ));
    }
    Ok(())
}

fn check_color(color: &str) -> ApiResult<()> {
    let valid = color.len() == 7
        && color.starts_with('#')
        && color[1..].bytes().all(|byte| byte.is_ascii_hexdigit());
    if !valid {
        return Err(validation(
            "A tag color must be a six-digit hex color such as #f97316",
            "invalid_tag_color",
        ));
    }
    Ok(())
}

fn check_description(description: &str) -> ApiResult<()> {
    if description.len() > MAX_DESCRIPTION {
        return Err(validation(
            "A tag description must not exceed 2000 characters",
            "invalid_tag_description",
        ));
    }
    Ok(())
}

fn validation(message: &str, code: &str) -> IntrospectionAPIError {
    IntrospectionAPIError::Http {
        message: message.to_string(),
        status: 422,
        code: Some(code.to_string()),
        request_id: None,
        body: Some(serde_json::Value::Null),
        retry_after: None,
    }
}
