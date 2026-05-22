# `web` — HTTP response helpers

General-purpose helpers shared across the route handlers in `spec/api.md` (implemented under `src/routes/`). They standardize the success/error envelope and a couple of small utilities.

Successful responses are `{ data, code: "ok" }` with an appropriate HTTP status. Error responses are `{ error, code }` with an appropriate HTTP status, where `code` is a short machine-readable string (e.g. `subdomain-exists`) and `error` is a human-readable message.

## `pub struct ApiError(pub Box<Response>)`

A boxed `axum` `Response` that any handler can return as its error type. Implements `IntoResponse` and `From<Response>`, so the error builders below compose with `?`, `.map_err(...)`, and explicit `Err(...)`.

## `pub type ApiResult = Result<Response, ApiError>`

The return type of every route handler. Success builders return `ApiResult` so they sit at the end of a handler without an `Ok(..)` wrap; error builders return `ApiError`.

## Response bodies

- `DataResponse<T> { data: T, code: "ok" }` - the success envelope
- `ErrorResponse { error: String, code: String }` - the error envelope

## Success builders (return `ApiResult`)

- `res<T>(status, data)` - `{ data, code: "ok" }` with `status`
- `ok<T>(data)` - `res(200, data)`
- `created<T>(data)` - `res(201, data)`

## Error builders (return `ApiError`)

- `err(status, code, message)` - the base `{ error, code }` builder
- `unauthorized(reason)` - `401`, `code = "unauthorized"`
- `forbidden(message)` - `403`, `code = "forbidden"`
- `not_found(message)` - `404`, `code = "not-found"`
- `bad_request(code, message)` - `400` with the given `code`
- `unprocessable(code, message)` - `422` with the given `code`
- `internal(reason)` - `500`, `code = "internal"`

## Utilities

- `parse_bool_default(value: i64, default: i64) -> i64` - returns `value` if it is `0` or `1`, otherwise `default`. Used to normalize boolean-ish relay flags.
- `map_unique_error(err: &anyhow::Error) -> Option<&'static str>` - recognizes sqlite UNIQUE constraint violations so callers can translate them into `422`s instead of `500`s. Returns `pubkey-exists` or `subdomain-exists` when the violated column message matches, else `None`.
