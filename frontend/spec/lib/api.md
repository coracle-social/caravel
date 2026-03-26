The api allows the frontend to access the database. Most endpoints are authenticated using NIP 98.

## `class ApiError`

- This is a custom error class with an additional `status` property

## `function makeAuth<T>()`

- If the user is not currently logged in, returns undefined
- Otherwise, builds an NIP 98 auth header with no `method` and `u` set to `VITE_API_URL`. This is non-standard, but intentional in order to prevent repeated signer authorizations.
- This function is memoized over pubkey and expires after 10 minutes.
- Returns the full `Authorization` header value.

## `function callApi<T>(method: string, path: string, body?: T)`

- Uses `makeAuth` to obtain a NIP 98 authorization header.
- Calls the backend api and returns the decoded json or throws an `ApiError`.

## `function get(path: string)`

- Calls `callApi` with `get`

## `function post<T>(path: string, body: T)`

- Calls `callApi` with `post` and body
