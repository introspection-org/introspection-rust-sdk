//! Apache Arrow IPC-stream decode for the telemetry list reads.
//!
//! Gated behind the `arrow` Cargo feature. When a caller requests the Arrow
//! format ([`ARROW_STREAM_ACCEPT`]), the DP responds with an Arrow IPC
//! *stream* of values only and moves the pagination metadata into response
//! headers. [`decode_arrow_response`] reads those headers and decodes the
//! stream into [`RecordBatch`]es, exposing the same
//! `count` / `total_count` / `next` shape as the JSON [`Paginated`] envelope.
//!
//! [`Paginated`]: crate::api::schemas::Paginated

use std::io::Cursor;

use arrow::record_batch::RecordBatch;
use reqwest::header::HeaderMap;
use reqwest::Response;

use crate::api::error::{ApiResult, IntrospectionAPIError};

/// `Accept` header value that selects the Arrow IPC-stream response.
pub const ARROW_STREAM_ACCEPT: &str = "application/vnd.apache.arrow.stream";

/// A decoded Arrow page: the record batches plus the pagination metadata the
/// DP moves into response headers for the Arrow format. Mirrors the JSON
/// [`Paginated`](crate::api::schemas::Paginated) envelope — `next` is the
/// opaque cursor to fetch the following page.
#[derive(Debug, Clone, Default)]
pub struct ArrowPage {
    /// Decoded record batches (values only — the schema rides in the stream).
    pub batches: Vec<RecordBatch>,
    /// Opaque cursor for the next page (`X-Next-Cursor`), if any.
    pub next: Option<String>,
    /// Row count in this page (`X-Result-Count`), if present.
    pub count: Option<u64>,
    /// Total row count across pages (`X-Total-Count`), if the DP computed it.
    pub total_count: Option<u64>,
    /// Whether the DP truncated the result (`X-Truncated`).
    pub truncated: bool,
}

impl ArrowPage {
    /// Total number of rows across all decoded batches.
    pub fn num_rows(&self) -> usize {
        self.batches.iter().map(RecordBatch::num_rows).sum()
    }
}

/// Decode a 2xx Arrow-stream [`Response`] into an [`ArrowPage`].
pub(crate) async fn decode_arrow_response(res: Response) -> ApiResult<ArrowPage> {
    let next = header_str(res.headers(), "x-next-cursor");
    let count = header_u64(res.headers(), "x-result-count");
    let total_count = header_u64(res.headers(), "x-total-count");
    let truncated = header_str(res.headers(), "x-truncated")
        .map(|v| v.eq_ignore_ascii_case("true") || v == "1")
        .unwrap_or(false);

    let bytes = res.bytes().await?;
    let batches = if bytes.is_empty() {
        Vec::new()
    } else {
        let reader =
            arrow::ipc::reader::StreamReader::try_new(Cursor::new(bytes), None).map_err(|e| {
                IntrospectionAPIError::Decode(format!("failed to open Arrow stream: {e}"))
            })?;
        let mut batches = Vec::new();
        for batch in reader {
            batches.push(batch.map_err(|e| {
                IntrospectionAPIError::Decode(format!("failed to decode Arrow batch: {e}"))
            })?);
        }
        batches
    };

    Ok(ArrowPage {
        batches,
        next,
        count,
        total_count,
        truncated,
    })
}

fn header_str(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
}

fn header_u64(headers: &HeaderMap, name: &str) -> Option<u64> {
    header_str(headers, name).and_then(|v| v.parse().ok())
}
