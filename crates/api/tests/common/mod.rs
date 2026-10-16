//! A test API: the router over a test core, and helpers to sign requests and
//! read responses.

#![allow(dead_code)]

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use caravel_core::Core;
use caravel_core::testing::{self, API_URL};
use coracle_lib::keys::SecretKey;
use http_body_util::BodyExt;
use serde_json::Value;
use tower::ServiceExt;

pub struct TestApi {
    pub core: Core,
    pub admin: SecretKey,
    router: Router,
}

pub struct Response {
    pub status: StatusCode,
    pub body: Value,
}

impl TestApi {
    pub async fn new() -> Self {
        let admin = SecretKey::generate();
        let core = testing::core(&admin.public_key().to_hex()).await;
        let router = caravel_api::router(core.clone());
        Self {
            core,
            admin,
            router,
        }
    }

    pub async fn request(
        &self,
        method: &str,
        path: &str,
        authorization: Option<&str>,
        body: Option<Value>,
    ) -> Response {
        let mut request = Request::builder().method(method).uri(path);
        if let Some(authorization) = authorization {
            request = request.header("Authorization", authorization);
        }
        let body = match body {
            Some(body) => {
                request = request.header("Content-Type", "application/json");
                Body::from(body.to_string())
            }
            None => Body::empty(),
        };

        let response = self
            .router
            .clone()
            .oneshot(request.body(body).expect("invalid request"))
            .await
            .expect("router failed");
        let status = response.status();
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("unreadable body")
            .to_bytes();
        let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);

        Response { status, body }
    }

    /// A request with a raw body, returning the raw response.
    pub async fn raw(
        &self,
        method: &str,
        path: &str,
        authorization: &str,
        body: Vec<u8>,
    ) -> (StatusCode, axum::http::HeaderMap, axum::body::Bytes) {
        let request = Request::builder()
            .method(method)
            .uri(path)
            .header("Authorization", authorization)
            .body(Body::from(body))
            .expect("invalid request");
        let response = self
            .router
            .clone()
            .oneshot(request)
            .await
            .expect("router failed");
        let (parts, body) = response.into_parts();
        let bytes = body.collect().await.expect("unreadable body").to_bytes();
        (parts.status, parts.headers, bytes)
    }

    /// A bearer authorization for a fresh session of `secret`'s pubkey.
    pub async fn bearer(&self, secret: &SecretKey) -> String {
        let auth = testing::http_auth(
            secret,
            &format!("{API_URL}/session"),
            "POST",
            self.core.clock.now(),
        );
        let response = self.request("POST", "/session", Some(&auth), None).await;
        assert_eq!(response.status, StatusCode::CREATED, "{:?}", response.body);
        format!(
            "Bearer {}",
            response.body["data"]["token"].as_str().expect("no token")
        )
    }

    /// Make `secret`'s pubkey a tenant without reaching Stripe.
    pub async fn create_tenant(&self, secret: &SecretKey) -> String {
        let pubkey = secret.public_key().to_hex();
        testing::insert_tenant(&self.core, &pubkey).await;
        pubkey
    }
}

/// The keys of a JSON object, sorted.
pub fn keys(value: &Value) -> Vec<String> {
    let mut keys: Vec<String> = value
        .as_object()
        .expect("not an object")
        .keys()
        .cloned()
        .collect();
    keys.sort();
    keys
}
