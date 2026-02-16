//! Response formatting: JSON, CSV, Arrow IPC, Arrow JSON.

use crate::error::Error;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

/// Content types we support.
#[derive(Debug, Clone, PartialEq)]
pub enum ResponseFormat {
    Json,
    SingleObjectJson,
    Csv,
    ArrowIpcStream,
    ArrowJson,
}

/// Parse Accept header into a ResponseFormat.
pub fn parse_accept(accept: Option<&str>) -> ResponseFormat {
    let accept = match accept {
        Some(a) => a,
        None => return ResponseFormat::Json,
    };

    if accept.contains("application/vnd.pgrst.object+json") {
        ResponseFormat::SingleObjectJson
    } else if accept.contains("text/csv") {
        ResponseFormat::Csv
    } else if accept.contains("application/vnd.apache.arrow.stream") {
        ResponseFormat::ArrowIpcStream
    } else if accept.contains("application/vnd.apache.arrow+json") {
        ResponseFormat::ArrowJson
    } else {
        ResponseFormat::Json
    }
}

/// Parse Prefer header into preferences.
#[derive(Debug, Clone, Default)]
pub struct Preferences {
    pub return_mode: ReturnMode,
    pub count: bool,
    pub resolution: Option<String>,
    pub tx: TxPreference,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub enum ReturnMode {
    #[default]
    Representation,
    HeadersOnly,
    Minimal,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub enum TxPreference {
    #[default]
    Commit,
    Rollback,
}

pub fn parse_prefer(prefer: Option<&str>) -> Preferences {
    let mut prefs = Preferences::default();

    let prefer = match prefer {
        Some(p) => p,
        None => return prefs,
    };

    for part in prefer.split(',') {
        let part = part.trim();
        if part == "return=representation" {
            prefs.return_mode = ReturnMode::Representation;
        } else if part == "return=headers-only" {
            prefs.return_mode = ReturnMode::HeadersOnly;
        } else if part == "return=minimal" {
            prefs.return_mode = ReturnMode::Minimal;
        } else if part == "count=exact" {
            prefs.count = true;
        } else if part == "resolution=merge-duplicates" {
            prefs.resolution = Some("merge-duplicates".to_string());
        } else if part == "tx=rollback" {
            prefs.tx = TxPreference::Rollback;
        } else if part == "tx=commit" {
            prefs.tx = TxPreference::Commit;
        }
    }

    prefs
}

/// Format rows as JSON array.
pub fn rows_to_json(rows: &[serde_json::Map<String, serde_json::Value>]) -> String {
    serde_json::to_string(rows).unwrap_or_else(|_| "[]".to_string())
}

/// Format rows as CSV.
pub fn rows_to_csv(
    rows: &[serde_json::Map<String, serde_json::Value>],
    columns: &[String],
) -> Result<String, Error> {
    let mut writer = csv::Writer::from_writer(Vec::new());

    // Header
    writer
        .write_record(columns)
        .map_err(|e| Error::Internal(e.to_string()))?;

    // Rows
    for row in rows {
        let record: Vec<String> = columns
            .iter()
            .map(|col| match row.get(col) {
                Some(serde_json::Value::Null) => String::new(),
                Some(serde_json::Value::String(s)) => s.clone(),
                Some(v) => v.to_string(),
                None => String::new(),
            })
            .collect();
        writer
            .write_record(&record)
            .map_err(|e| Error::Internal(e.to_string()))?;
    }

    let data = writer
        .into_inner()
        .map_err(|e| Error::Internal(e.to_string()))?;
    String::from_utf8(data).map_err(|e| Error::Internal(e.to_string()))
}

/// Format an Arrow RecordBatch as IPC stream bytes.
pub fn record_batch_to_ipc(
    batch: &arrow::record_batch::RecordBatch,
) -> Result<Vec<u8>, Error> {
    let mut buf = Vec::new();
    {
        let mut writer =
            arrow_ipc::writer::StreamWriter::try_new(&mut buf, &batch.schema())
                .map_err(|e| Error::Internal(e.to_string()))?;
        writer
            .write(batch)
            .map_err(|e| Error::Internal(e.to_string()))?;
        writer
            .finish()
            .map_err(|e| Error::Internal(e.to_string()))?;
    }
    Ok(buf)
}

/// Format an Arrow RecordBatch as JSON using arrow-json.
pub fn record_batch_to_arrow_json(
    batch: &arrow::record_batch::RecordBatch,
) -> Result<String, Error> {
    let mut buf = Vec::new();
    let mut writer = arrow_json::ArrayWriter::new(&mut buf);
    writer
        .write(batch)
        .map_err(|e| Error::Internal(e.to_string()))?;
    writer
        .finish()
        .map_err(|e| Error::Internal(e.to_string()))?;
    String::from_utf8(buf).map_err(|e| Error::Internal(e.to_string()))
}

/// Build the final HTTP response with appropriate headers.
pub fn build_response(
    body: Vec<u8>,
    content_type: &str,
    status: StatusCode,
    content_range: Option<String>,
    content_location: Option<String>,
) -> Response {
    let mut builder = Response::builder().status(status);

    builder = builder.header("Content-Type", content_type);

    if let Some(range) = content_range {
        builder = builder.header("Content-Range", range);
    }

    if let Some(location) = content_location {
        builder = builder.header("Content-Location", location);
    }

    builder
        .body(axum::body::Body::from(body))
        .unwrap_or_else(|_| {
            (StatusCode::INTERNAL_SERVER_ERROR, "Internal error")
                .into_response()
        })
}
