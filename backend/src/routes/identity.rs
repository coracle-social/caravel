use std::sync::Arc;

use axum::{extract::State, http::StatusCode, response::Response};
use serde::Serialize;

use crate::api::{Api, AuthedPubkey};
use crate::web::ok;

#[derive(Serialize)]
struct IdentityResponse {
    pubkey: String,
    is_admin: bool,
}

pub async fn get_identity(
    State(api): State<Arc<Api>>,
    AuthedPubkey(pubkey): AuthedPubkey,
) -> Response {
    let is_admin = api.is_admin(&pubkey);
    ok(StatusCode::OK, IdentityResponse { pubkey, is_admin })
}
