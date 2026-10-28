//! The API's envelope: `{"data": …, "code": "ok"}` on success, and
//! `{"error": …, "code": …}` on failure.

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use caravel_util::error::Error;
use serde::Serialize;

pub type ApiResult = Result<Response, ApiError>;

/// A core error on its way out as a response.
#[derive(Debug)]
pub struct ApiError(pub Error);

impl<E: Into<Error>> From<E> for ApiError {
    fn from(error: E) -> Self {
        Self(error.into())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, code) = match &self.0 {
            Error::Unauthorized(_) => (StatusCode::UNAUTHORIZED, "unauthorized"),
            Error::Forbidden(_) => (StatusCode::FORBIDDEN, "forbidden"),
            Error::NotFound(_) => (StatusCode::NOT_FOUND, "not-found"),
            Error::Conflict { code, .. } => (StatusCode::BAD_REQUEST, *code),
            Error::Invalid { code, .. } => (StatusCode::UNPROCESSABLE_ENTITY, *code),
            Error::Internal(cause) => {
                tracing::error!(error = ?cause, "request failed");
                return error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal",
                    "internal error",
                );
            }
        };

        error(status, code, &self.0.to_string())
    }
}

pub fn ok<T: Serialize>(data: T) -> ApiResult {
    Ok(envelope(StatusCode::OK, data))
}

pub fn created<T: Serialize>(data: T) -> ApiResult {
    Ok(envelope(StatusCode::CREATED, data))
}

fn envelope<T: Serialize>(status: StatusCode, data: T) -> Response {
    (
        status,
        Json(serde_json::json!({ "data": data, "code": "ok" })),
    )
        .into_response()
}

pub fn error(status: StatusCode, code: &str, message: &str) -> Response {
    (
        status,
        Json(serde_json::json!({ "error": message, "code": code })),
    )
        .into_response()
}
