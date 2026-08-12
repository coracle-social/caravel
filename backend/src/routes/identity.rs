use serde::Serialize;

use crate::api::AuthedPubkey;
use crate::env;
use crate::web::{ApiResult, ok};

#[derive(Serialize)]
struct IdentityResponse {
    pubkey: String,
    is_admin: bool,
}

pub async fn get_identity(AuthedPubkey(pubkey): AuthedPubkey) -> ApiResult {
    let is_admin = env::get().is_admin(&pubkey);
    ok(IdentityResponse { pubkey, is_admin })
}
