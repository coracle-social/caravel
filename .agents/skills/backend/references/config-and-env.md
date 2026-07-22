# Config and the env singleton (deep detail)

This is the lookup-depth companion to the SKILL.md "config" section: the full variable surface, the `require_*` helpers, the `DATABASE_URL` rewrite, and the stale-README traps. All of it lives in `env.rs`, `db.rs`, `main.rs`, `api.rs`, `.env.template`, and the README.

## The variable surface

`Env` is `#[derive(Clone)]` with 23 fields plus a parsed `keys: Keys`. The full grouped surface:

- **server** — `SERVER_URL`, `SERVER_PORT` (u16), `SERVER_ADMIN_PUBKEYS` (csv), `SERVER_ALLOW_ORIGINS` (csv), `DATABASE_URL`
- **robot identity** — `ROBOT_SECRET` (parsed into `Keys`), `ROBOT_NAME`, `ROBOT_DESCRIPTION`, `ROBOT_PICTURE`, `ROBOT_WALLET`, `ROBOT_OUTBOX_RELAYS`, `ROBOT_INDEXER_RELAYS`, `ROBOT_MESSAGING_RELAYS`
- **zooid/livekit** — `ZOOID_API_URL`, `ZOOID_DOMAINS` (csv), `LIVEKIT_URL`, `LIVEKIT_API_KEY`, `LIVEKIT_API_SECRET`
- **blossom S3** — `BLOSSOM_S3_ENDPOINT`, `BLOSSOM_S3_REGION`, `BLOSSOM_S3_BUCKET`, `BLOSSOM_S3_ACCESS_KEY`, `BLOSSOM_S3_SECRET_KEY`
- **billing** — `STRIPE_SECRET_KEY`

Source: `env.rs:22-82`, `.env.template:1-38`.

## The require_* helpers

- `require_str` reads the var, panics `"{key} is required"` if unset, trims it, and panics again if the trimmed value is empty.
- `require_u16` calls `require_str` then `.parse()`, panicking `"{key} is invalid"` on a parse failure.
- `require_csv` uses `std::env::var(key).unwrap_or_default()`, splits on `,`, trims each element, drops empties, and panics `"{key} is required"` if the result is empty — so an *unset* csv var and a *present-but-all-blank* one both say "required", which can mislead debugging.

`ROBOT_SECRET` is parsed via `Keys::parse(...).expect(...)`, so an invalid key panics too. Pick the helper by type: scalars use `require_str`, the port uses `require_u16`, lists use `require_csv`. Source: `env.rs:53-55,110-140`.

## Field semantics worth noting

- `SERVER_URL` is the NIP-98 host-affinity value — the `u` tag must equal it exactly or every authenticated request 401s.
- `SERVER_PORT` binds `127.0.0.1` only (localhost, not `0.0.0.0`).
- `SERVER_ADMIN_PUBKEYS` is checked by membership for `is_admin`.
- `SERVER_ALLOW_ORIGINS` is parsed into `CorsLayer` `HeaderValue`s, with unparseable origins **silently dropped** via `filter_map(...ok())` — a typo won't error at startup, it just won't allow that origin and surfaces later as a browser CORS block.
- No var is trailing-slash-trimmed at load (`zooid_api_url` is trimmed at use). There is no frontend base URL in the env at all — each tenant carries its own `return_url`, supplied when the tenant is created, and DM links are built against that.

Source: `api.rs:104,158-202`, `main.rs:44-65`, `env.rs:62`.

## DATABASE_URL normalization

`normalize_sqlite_url` rewrites a relative `sqlite://<rel>` to `sqlite://{CARGO_MANIFEST_DIR}/<rel>` — and `CARGO_MANIFEST_DIR` is baked in at **compile** time via `env!`, so the path resolves relative to the build-time backend crate dir, not the process cwd. `:memory:`, absolute, and non-sqlite URLs pass through unchanged; the parent dir is `create_dir_all`'d; connection uses `create_if_missing` plus WAL plus `./migrations`. In the Docker image the backend is compiled with `WORKDIR /app`, so `CARGO_MANIFEST_DIR` = `/app`; the documented **relative** `DATABASE_URL=sqlite://data/caravel.db` therefore resolves to `/app/data/caravel.db`, lining up with the `-v my-caravel-data:/app/data` volume mount. The resolution comes from the build-time crate dir, not the runtime working directory, and the deployment URL is relative (not absolute). Source: `db.rs:70-108`, `Dockerfile:6`, `README.md:18,23`.

## Adding a config var, and doing crypto through Env

Adding a var is four coordinated edits: an `Env` field, a load line in `Env::load` with the right `require_*` helper, README docs, and `.env.template`. Do crypto/auth through `Env` methods (`encrypt`/`decrypt`, `make_auth`), not by reaching for `keys` ad hoc. Normalize URL-shaped config at the edge the way existing code does. Source: `env.rs:22-107`.

## Stale-README traps to ignore

- The README's local-dev table uses `ADMINS` (the real var is `SERVER_ADMIN_PUBKEYS`) and `ZOOID_API_SECRET` (no such var — zooid auth is `ROBOT_SECRET` via NIP-98).
- The README docker example sets `PLATFORM_NAME`, which is a frontend VITE var, not a backend `Env` field.
- Trust `env.rs` and `.env.template`, not the README.
- `.env` is gitignored (root `.env` and `**/.env`), as are `data/` and `target/`; there is no backend-level `.gitignore`.

Source: `README.md` vs `env.rs:54,60,76`, `.gitignore:5-8`.

## Sources

- variable surface — `backend/src/env.rs:22-82`, `backend/.env.template:1-38`
- require_* helpers — `backend/src/env.rs:53-55,110-140`
- field semantics — `backend/src/api.rs:104,158-202`, `backend/src/main.rs:44-65`, `backend/src/env.rs:62`
- DATABASE_URL normalization — `backend/src/db.rs:70-108`
- adding a var + crypto via Env — `backend/src/env.rs:22-107`
- stale-README traps — `README.md` vs `backend/src/env.rs:54,60,76`, `.gitignore:5-8`
