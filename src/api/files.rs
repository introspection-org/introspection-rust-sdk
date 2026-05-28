//! `client.files.*` — upload / download / list / versions.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use bytes::Bytes;
use futures::stream::Stream;
use reqwest::multipart::{Form, Part};

use crate::api::error::{ApiResult, IntrospectionAPIError};
use crate::api::http::HttpClient;
use crate::api::paginator::Paginator;
use crate::api::schemas::{
    File, FileCreateText, FileListParams, FileType, FileUpdate, PaginationParams,
};

/// Source of bytes for an upload.
#[derive(Debug, Clone)]
pub enum UploadSource {
    /// Read from a local filesystem path.
    Path(PathBuf),
    /// In-memory bytes.
    Bytes(Vec<u8>),
}

impl From<PathBuf> for UploadSource {
    fn from(p: PathBuf) -> Self {
        Self::Path(p)
    }
}

impl From<&Path> for UploadSource {
    fn from(p: &Path) -> Self {
        Self::Path(p.to_path_buf())
    }
}

impl From<Vec<u8>> for UploadSource {
    fn from(b: Vec<u8>) -> Self {
        Self::Bytes(b)
    }
}

impl From<&[u8]> for UploadSource {
    fn from(b: &[u8]) -> Self {
        Self::Bytes(b.to_vec())
    }
}

/// Parameters for a multipart upload.
#[derive(Debug, Clone)]
pub struct FileUpload {
    pub source: UploadSource,
    /// On-wire filename. If omitted and `source` is a [`UploadSource::Path`],
    /// defaults to the path's file name. Required for [`UploadSource::Bytes`].
    pub name: Option<String>,
    pub file_type: Option<FileType>,
    /// Content-Type for the file part. Defaults to a guess from the
    /// filename (or `application/octet-stream`).
    pub content_type: Option<String>,
}

impl FileUpload {
    pub fn from_path(path: impl Into<PathBuf>) -> Self {
        Self {
            source: UploadSource::Path(path.into()),
            name: None,
            file_type: None,
            content_type: None,
        }
    }

    pub fn from_bytes(bytes: impl Into<Vec<u8>>, name: impl Into<String>) -> Self {
        Self {
            source: UploadSource::Bytes(bytes.into()),
            name: Some(name.into()),
            file_type: None,
            content_type: None,
        }
    }

    pub fn with_file_type(mut self, ft: FileType) -> Self {
        self.file_type = Some(ft);
        self
    }

    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    pub fn with_content_type(mut self, ct: impl Into<String>) -> Self {
        self.content_type = Some(ct.into());
        self
    }
}

/// `client.files.versions.*`.
#[derive(Clone)]
pub struct FileVersions {
    http: Arc<HttpClient>,
}

impl FileVersions {
    #[doc(hidden)]
    pub fn new(http: Arc<HttpClient>) -> Self {
        Self { http }
    }

    /// `GET /v1/files/{id}/versions` — paginator over the file's
    /// version chain (newest first).
    pub fn list(&self, file_id: &str, params: &PaginationParams) -> Paginator<File> {
        let path = format!("/v1/files/{}/versions", urlencode(file_id));
        Paginator::new(self.http.clone(), path, params)
            .expect("PaginationParams must serialize to a JSON object")
    }

    /// `GET /v1/files/{id}/versions/{vid}`.
    pub async fn get(&self, file_id: &str, version_id: &str) -> ApiResult<File> {
        let path = format!(
            "/v1/files/{}/versions/{}",
            urlencode(file_id),
            urlencode(version_id)
        );
        self.http.get_json(&path, &()).await
    }

    /// `POST /v1/files/{id}/versions` — multipart upload of a new
    /// version.
    pub async fn create(&self, file_id: &str, upload: FileUpload) -> ApiResult<File> {
        let form = build_upload_form(upload).await?;
        let path = format!("/v1/files/{}/versions", urlencode(file_id));
        self.http.post_multipart(&path, form).await
    }
}

/// `client.files.*`.
#[derive(Clone)]
pub struct Files {
    http: Arc<HttpClient>,
    /// Nested `versions` namespace.
    pub versions: FileVersions,
}

impl Files {
    #[doc(hidden)]
    pub fn new(http: Arc<HttpClient>) -> Self {
        let versions = FileVersions::new(http.clone());
        Self { http, versions }
    }

    /// `GET /v1/files` — paginator over the list endpoint.
    ///
    /// Implements [`futures::Stream`] (auto-paginates) and exposes
    /// [`Paginator::next_page`] for page-at-a-time access.
    pub fn list(&self, params: &FileListParams) -> Paginator<File> {
        Paginator::new(self.http.clone(), "/v1/files", params)
            .expect("FileListParams must serialize to a JSON object")
    }

    /// `POST /v1/files` — multipart binary upload.
    ///
    /// ```rust,no_run
    /// # use introspection_sdk::{ClientConfig, IntrospectionClient, RunRequest};
    /// # use introspection_sdk::api::{FileUpload, FileType};
    /// # use uuid::Uuid;
    /// # async fn run() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = IntrospectionClient::new(ClientConfig::default())?;
    /// let runtime_id: Uuid = std::env::var("INTROSPECTION_RUNTIME_ID")?.parse()?;
    /// let runner = client.runtime(runtime_id).run(RunRequest::default()).await?;
    /// let file = runner.files().upload(
    ///     FileUpload::from_path("input.jsonl").with_file_type(FileType::Upload),
    /// ).await?;
    /// println!("{}", file.id);
    /// # Ok(()) }
    /// ```
    pub async fn upload(&self, upload: FileUpload) -> ApiResult<File> {
        let form = build_upload_form(upload).await?;
        self.http.post_multipart("/v1/files", form).await
    }

    /// `POST /v1/files` (JSON) — create a text/markdown file by content.
    pub async fn create_text(&self, body: &FileCreateText) -> ApiResult<File> {
        self.http.post_json("/v1/files", body).await
    }

    /// `GET /v1/files/{id}`.
    pub async fn get(&self, file_id: &str) -> ApiResult<File> {
        let path = format!("/v1/files/{}", urlencode(file_id));
        self.http.get_json(&path, &()).await
    }

    /// `PATCH /v1/files/{id}`.
    pub async fn update(&self, file_id: &str, body: &FileUpdate) -> ApiResult<File> {
        let path = format!("/v1/files/{}", urlencode(file_id));
        self.http.patch_json(&path, body).await
    }

    /// `DELETE /v1/files/{id}`.
    pub async fn delete(&self, file_id: &str) -> ApiResult<()> {
        let path = format!("/v1/files/{}", urlencode(file_id));
        self.http.delete_empty(&path).await
    }

    /// `GET /v1/files/{id}/content` — read all bytes into memory.
    pub async fn download(&self, file_id: &str) -> ApiResult<Bytes> {
        let path = format!("/v1/files/{}/content", urlencode(file_id));
        self.http.get_bytes(&path).await
    }

    /// `GET /v1/files/{id}/content` — streaming download. Returns a
    /// `Stream<Item = ApiResult<Bytes>>` so callers can write to disk
    /// without buffering the whole file.
    pub async fn download_stream(
        &self,
        file_id: &str,
    ) -> ApiResult<impl Stream<Item = ApiResult<Bytes>>> {
        use futures::StreamExt;
        let path = format!("/v1/files/{}/content", urlencode(file_id));
        let res = self.http.get_stream(&path, None).await?;
        Ok(res
            .bytes_stream()
            .map(|chunk| chunk.map_err(IntrospectionAPIError::from)))
    }
}

async fn build_upload_form(upload: FileUpload) -> ApiResult<Form> {
    let (name, bytes, content_type) =
        materialise(upload.source, upload.name, upload.content_type).await?;
    let part = Part::bytes(bytes)
        .file_name(name.clone())
        .mime_str(&content_type)
        .map_err(|e| {
            IntrospectionAPIError::InvalidConfig(format!(
                "invalid Content-Type `{content_type}`: {e}"
            ))
        })?;
    let mut form = Form::new().part("file", part).text("name", name);
    if let Some(ft) = upload.file_type {
        form = form.text("file_type", ft.as_str().to_string());
    }
    Ok(form)
}

async fn materialise(
    source: UploadSource,
    name: Option<String>,
    content_type: Option<String>,
) -> ApiResult<(String, Vec<u8>, String)> {
    match source {
        UploadSource::Path(p) => {
            let resolved_name = name.unwrap_or_else(|| {
                p.file_name()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "file".to_string())
            });
            let ct = content_type.unwrap_or_else(|| guess_mime(&resolved_name));
            let bytes = tokio::fs::read(&p).await?;
            Ok((resolved_name, bytes, ct))
        }
        UploadSource::Bytes(b) => {
            let resolved_name = name.ok_or_else(|| {
                IntrospectionAPIError::InvalidConfig(
                    "`name` is required when uploading raw bytes".to_string(),
                )
            })?;
            let ct = content_type.unwrap_or_else(|| guess_mime(&resolved_name));
            Ok((resolved_name, b, ct))
        }
    }
}

fn guess_mime(name: &str) -> String {
    mime_guess::from_path(name)
        .first_or_octet_stream()
        .essence_str()
        .to_string()
}

fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for byte in s.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guess_mime_from_extension() {
        assert_eq!(guess_mime("foo.txt"), "text/plain");
        assert_eq!(guess_mime("foo.json"), "application/json");
        assert_eq!(guess_mime("noext"), "application/octet-stream");
    }
}
