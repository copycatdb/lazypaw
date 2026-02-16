#![allow(dead_code)]
//! Error types and HTTP status mapping.
//!
//! Provides a PostgREST-compatible error format and maps SQL Server
//! errors to appropriate HTTP status codes.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Serialize;

/// PostgREST-compatible error body.
#[derive(Debug, Serialize)]
pub struct ApiError {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

/// The main error type for lazypaw.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Bad request: {0}")]
    BadRequest(String),

    #[error("Unauthorized: {0}")]
    Unauthorized(String),

    #[error("Forbidden: {0}")]
    Forbidden(String),

    #[error("Conflict: {0}")]
    Conflict(String),

    #[error("SQL error: {0}")]
    Sql(String),

    #[error("Pool error: {0}")]
    Pool(String),

    #[error("Internal error: {0}")]
    Internal(String),

    #[error("Single object expected but got {0} rows")]
    SingleObjectExpected(usize),
}

impl Error {
    pub fn status_code(&self) -> StatusCode {
        match self {
            Error::NotFound(_) => StatusCode::NOT_FOUND,
            Error::BadRequest(_) => StatusCode::BAD_REQUEST,
            Error::Unauthorized(_) => StatusCode::UNAUTHORIZED,
            Error::Forbidden(_) => StatusCode::FORBIDDEN,
            Error::Conflict(_) => StatusCode::CONFLICT,
            Error::Sql(msg) => sql_error_to_status(msg),
            Error::Pool(_) => StatusCode::SERVICE_UNAVAILABLE,
            Error::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
            Error::SingleObjectExpected(_) => StatusCode::NOT_ACCEPTABLE,
        }
    }

    pub fn code(&self) -> &str {
        match self {
            Error::NotFound(_) => "PGRST116",
            Error::BadRequest(_) => "PGRST100",
            Error::Unauthorized(_) => "PGRST301",
            Error::Forbidden(_) => "PGRST302",
            Error::Conflict(_) => "PGRST209",
            Error::Sql(_) => "PGRST200",
            Error::Pool(_) => "PGRST503",
            Error::Internal(_) => "PGRST500",
            Error::SingleObjectExpected(_) => "PGRST116",
        }
    }

    pub fn to_api_error(&self) -> ApiError {
        ApiError {
            code: self.code().to_string(),
            message: self.to_string(),
            details: match self {
                Error::Sql(msg) => Some(msg.clone()),
                _ => None,
            },
            hint: None,
        }
    }
}

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        let status = self.status_code();
        let body = serde_json::to_string(&self.to_api_error()).unwrap_or_default();
        (
            status,
            [(
                axum::http::header::CONTENT_TYPE,
                "application/json; charset=utf-8",
            )],
            body,
        )
            .into_response()
    }
}

/// Map SQL Server error messages to HTTP status codes.
fn sql_error_to_status(msg: &str) -> StatusCode {
    let upper = msg.to_uppercase();
    if upper.contains("VIOLATION OF PRIMARY KEY")
        || upper.contains("VIOLATION OF UNIQUE KEY")
        || upper.contains("CANNOT INSERT DUPLICATE")
        || upper.contains("UNIQUE CONSTRAINT")
    {
        StatusCode::CONFLICT
    } else if upper.contains("FOREIGN KEY") {
        StatusCode::CONFLICT
    } else if upper.contains("PERMISSION DENIED") || upper.contains("ACCESS DENIED") {
        StatusCode::FORBIDDEN
    } else if upper.contains("LOGIN FAILED") {
        StatusCode::UNAUTHORIZED
    } else if upper.contains("INVALID OBJECT NAME") || upper.contains("DOES NOT EXIST") {
        StatusCode::NOT_FOUND
    } else if upper.contains("CONVERSION FAILED")
        || upper.contains("SYNTAX ERROR")
        || upper.contains("INCORRECT SYNTAX")
    {
        StatusCode::BAD_REQUEST
    } else {
        StatusCode::INTERNAL_SERVER_ERROR
    }
}
