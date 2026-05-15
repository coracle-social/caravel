use std::sync::Arc;

use axum::extract::State;
use serde::Serialize;

use crate::api::{Api, AuthedPubkey};
use crate::web::{ApiResult, ok};

#[derive(Serialize)]
struct IdentityResponse {
    pubkey: String,
    is_admin: bool,
}

pub async fn get_identity(
    State(api): State<Arc<Api>>,
    AuthedPubkey(pubkey): AuthedPubkey,
) -> ApiResult {
    let is_admin = api.is_admin(&pubkey);
    ok(IdentityResponse { pubkey, is_admin })
}
