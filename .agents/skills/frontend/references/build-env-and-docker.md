# Build, TypeScript config, env vars, and Docker (deep detail)

Lookup-depth companion to the SKILL.md "Building and verifying" section.

## Build and typecheck

The only `package.json` scripts are `dev` (`vite`), `build` (`tsc -b && vite build`), and `preview` (`vite preview`). There is no lint or test script — the build *is* the verification gate.

`tsc -b` builds via TypeScript **project references**: the root `tsconfig.json` has `files: []` and references `tsconfig.app.json` (compiles `src`) and `tsconfig.node.json` (compiles `vite.config.ts`). Both set `noEmit: true`, so `tsc -b` is purely a typecheck; the actual emit comes from `vite build`.

The strict flags a change must satisfy (in `tsconfig.app.json`): `strict`, `noUnusedLocals`, `noUnusedParameters`, `erasableSyntaxOnly`, `noFallthroughCasesInSwitch`, `noUncheckedSideEffectImports`, plus `verbatimModuleSyntax` and `moduleDetection: force`. JSX is `preserve` with `jsxImportSource: solid-js`. The `@/*` → `src/*` path alias is declared here (and mirrored in `vite.config.ts`).

## Commands and the package manager

Bun is the package manager: `bun.lock` is present and there is no `package-lock.json` / `yarn.lock` / `pnpm-lock.yaml`.

- `just build-frontend` → `cd frontend && bun i && bun run build` — the canonical way to verify a frontend change compiles.
- `just build` → `build-backend build-frontend` — also compiles the Rust backend, so it needs a working Rust toolchain. Use `build-frontend` to verify only frontend changes.
- `just dev-frontend` → `cd frontend && bun run dev`.
- There is **no** frontend lint or test target in the `justfile`; the aggregate `lint`/`test`/`fmt` tasks are backend-only.

## The four VITE_* env vars

| Var | Declared in `global.d.ts`? | In `.env.template`? | Where it's read | Role |
| --- | --- | --- | --- | --- |
| `VITE_API_URL` | yes | yes (`http://127.0.0.1:2892`) | `api.ts` → `API_URL` | backend base URL for requests; also the NIP-98 `u`-tag value |
| `VITE_RELAY_DOMAIN` | yes | yes (`spaces.coracle.social`) | `subdomain.ts` → `RELAY_DOMAIN` | relay subdomain suffix; must match the backend's `RELAY_DOMAIN` |
| `VITE_PLATFORM_NAME` | yes (optional) | yes (`Caravel`) | `state.ts` → `PLATFORM_NAME` | platform display name (title, login, invoice PDF) |
| `VITE_PORT` | **no** | **no** | `vite.config.ts` only (`Number(env.VITE_PORT) || 5173`) | dev-server port; never reaches client code |

Convention: read each client var once at module top-level into a named `const` typed via `global.d.ts`'s `ImportMetaEnv`, rather than scattering `import.meta.env` reads through components. A new client-readable var must be `VITE_`-prefixed, declared in `ImportMetaEnv` in `src/global.d.ts`, and added to `.env.template` with a default. Non-client build-tool vars (like `VITE_PORT`) are read only in `vite.config.ts` via `loadEnv(mode, process.cwd(), '')`.

`global.d.ts` is the single ambient-declarations file: it also declares `Window.HSStaticMethods` (Preline) and `Window.nostr?` (NIP-07). Extend the existing interfaces there rather than re-declaring elsewhere.

## Build-time bake vs. Docker runtime substitution

`VITE_*` values are **baked into the bundle at build time**, so editing `frontend/.env` requires a rebuild to take effect.

In Docker this is sidestepped: the frontend is built once with sentinel placeholder values (`VITE_API_URL=__VITE_API_URL__`, etc.). The container entrypoint then maps runtime env (`SERVER_URL`, `RELAY_DOMAIN`, `PLATFORM_NAME`) onto those placeholders and `sed`-substitutes them into every `*.js`/`*.html` in `/app/dist` at startup, before serving with `serve -s /app/dist -l 3000`. So one image can be deployed with any config without a rebuild, but runtime config there does *not* require a rebuild — unlike a local `.env` change.

When adding a new build-time-substituted `VITE_` var intended for Docker, mirror the existing pattern: declare the placeholder `ENV` in the build stage and add the matching `sed -e` mapping in the entrypoint.

## Sources

- scripts, no lint/test — `frontend/package.json:6-10`
- project references, `noEmit` — `frontend/tsconfig.json:1-7`
- strict flags + alias — `frontend/tsconfig.app.json:12-30`
- bun lockfile — `frontend/bun.lock` (no other lockfile present)
- `just` targets — `justfile:11-12,32-43`
- env var declarations — `frontend/src/global.d.ts:17-31`
- env defaults — `frontend/.env.template:1-8`
- `VITE_PORT` dev-server port — `frontend/vite.config.ts:6-18`
- Docker placeholder build + entrypoint `sed` — `Dockerfile` frontend-build stage and entrypoint
