//! General-purpose HTTP helpers shared across route handlers.
//!
//! Success builders (`res`, `ok`, `created`) return [`ApiResult`] so they
//! can sit at the end of a handler without an `Ok(..)` wrap. Error builders
//! (`err`, `not_found`, `forbidden`, …) return [`ApiError`] so they compose
//! with `.map_err(...)` and with explicit `Err(...)` returns.

use std::fmt::Display;

use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;

pub struct ApiError(pub Box<Response>);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        *self.0
    }
}

impl From<Response> for ApiError {
    fn from(r: Response) -> Self {
        Self(Box::new(r))
    }
}

pub type ApiResult = Result<Response, ApiError>;

#[derive(Serialize)]
pub struct DataResponse<T: Serialize> {
    pub data: T,
    pub code: &'static str,
}

#[derive(Serialize)]
pub struct ErrorResponse {
    pub error: String,
    pub code: String,
}

// --- success builders (return ApiResult) ------------------------------------

pub fn res<T: Serialize>(status: StatusCode, data: T) -> ApiResult {
    Ok((status, Json(DataResponse { data, code: "ok" })).into_response())
}

pub fn ok<T: Serialize>(data: T) -> ApiResult {
    res(StatusCode::OK, data)
}

pub fn created<T: Serialize>(data: T) -> ApiResult {
    res(StatusCode::CREATED, data)
}

// --- error builders (return ApiError) ---------------------------------------

pub fn err(status: StatusCode, code: &str, message: &str) -> ApiError {
    (
        status,
        Json(ErrorResponse {
            error: message.to_string(),
            code: code.to_string(),
        }),
    )
        .into_response()
        .into()
}

pub fn unauthorized(reason: impl Display) -> ApiError {
    err(
        StatusCode::UNAUTHORIZED,
        "unauthorized",
        &reason.to_string(),
    )
}

pub fn forbidden(message: &str) -> ApiError {
    err(StatusCode::FORBIDDEN, "forbidden", message)
}

pub fn not_found(message: &str) -> ApiError {
    err(StatusCode::NOT_FOUND, "not-found", message)
}

pub fn bad_request(code: &str, message: &str) -> ApiError {
    err(StatusCode::BAD_REQUEST, code, message)
}

pub fn unprocessable(code: &str, message: &str) -> ApiError {
    err(StatusCode::UNPROCESSABLE_ENTITY, code, message)
}

pub fn internal(reason: impl Display) -> ApiError {
    err(
        StatusCode::INTERNAL_SERVER_ERROR,
        "internal",
        &reason.to_string(),
    )
}

// --- misc utilities ---------------------------------------------------------

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
