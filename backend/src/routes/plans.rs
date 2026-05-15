use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::Response,
};

use crate::api::Api;
use crate::web::{err, ok};

pub async fn list_plans(State(api): State<Arc<Api>>) -> Response {
    ok(StatusCode::OK, api.query.list_plans())
}

pub async fn get_plan(State(api): State<Arc<Api>>, Path(id): Path<String>) -> Response {
    match api.query.get_plan(&id) {
        Some(plan) => ok(StatusCode::OK, plan),
        None => err(StatusCode::NOT_FOUND, "not-found", "plan not found"),
    }
}
