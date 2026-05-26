use std::sync::Arc;

use axum::extract::{Path, State};

use crate::api::Api;
use crate::query;
use crate::web::{ApiResult, not_found, ok};

pub async fn list_plans(State(_api): State<Arc<Api>>) -> ApiResult {
    ok(query::list_plans())
}

pub async fn get_plan(State(_api): State<Arc<Api>>, Path(id): Path<String>) -> ApiResult {
    match query::get_plan(&id) {
        Some(plan) => ok(plan),
        None => Err(not_found("plan not found")),
    }
}
