//! General-purpose HTTP helpers shared across route handlers.
//!
//! This module owns the wire response envelopes (`ok` / `err`), the
//! `ApiError` type that route handlers return, and a few stateless utilities.

use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;

#[derive(Serialize)]
pub struct OkResponse<T: Serialize> {
    pub data: T,
    pub code: &'static str,
}

#[derive(Serialize)]
pub struct ErrorResponse {
    pub error: String,
    pub code: String,
}

#[derive(Debug)]
pub enum ApiError {
    Unauthorized(anyhow::Error),
    Forbidden(&'static str),
    NotFound(&'static str),
    Client {
        status: StatusCode,
        code: &'static str,
        message: &'static str,
    },
    Internal(String),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        match self {
            Self::Unauthorized(e) => err(StatusCode::UNAUTHORIZED, "unauthorized", &e.to_string()),
            Self::Forbidden(message) => err(StatusCode::FORBIDDEN, "forbidden", message),
            Self::NotFound(message) => err(StatusCode::NOT_FOUND, "not-found", message),
            Self::Client {
                status,
                code,
                message,
            } => err(status, code, message),
            Self::Internal(message) => err(StatusCode::INTERNAL_SERVER_ERROR, "internal", &message),
        }
    }
}

pub fn ok<T: Serialize>(status: StatusCode, data: T) -> Response {
    (status, Json(OkResponse { data, code: "ok" })).into_response()
}

pub fn err(status: StatusCode, code: &str, message: &str) -> Response {
    (
        status,
        Json(ErrorResponse {
            error: message.to_string(),
            code: code.to_string(),
        }),
    )
        .into_response()
}

pub fn parse_bool_default(value: i64, default: i64) -> i64 {
    if value == 0 || value == 1 {
        value
    } else {
        default
    }
}

/// Recognize sqlite UNIQUE constraint violations on known columns so the
/// caller can translate them into 422 responses instead of opaque 500s.
pub fn map_unique_error(err: &anyhow::Error) -> Option<&'static str> {
    let sqlx_err = err.downcast_ref::<sqlx::Error>()?;
    let sqlx::Error::Database(db_err) = sqlx_err else {
        return None;
    };
    if db_err.message().contains("pubkey") {
        return Some("pubkey-exists");
    }
    if db_err.message().contains("subdomain") {
        return Some("subdomain-exists");
    }
    None
}
