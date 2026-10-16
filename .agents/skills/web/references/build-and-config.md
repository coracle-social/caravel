# Build, TypeScript config and runtime configuration (deep detail)

Lookup-depth companion to the SKILL.md "Building and verifying" section.

## Build and typecheck

The only `package.json` scripts are `dev` (`vite`), `build` (`tsc -b && vite build`), and `preview` (`vite preview`). There is no lint or test script — the build *is* the verification gate.

`tsc -b` builds via TypeScript **project references**: the root `tsconfig.json` has `files: []` and references `tsconfig.app.json` (compiles `src`) and `tsconfig.node.json` (compiles `vite.config.ts`). Both set `noEmit: true`, so `tsc -b` is purely a typecheck; the emit comes from `vite build`, which bundles the three HTML entries named in `vite.config.ts` into `web/dist`, sharing chunks between them.

The strict flags a change must satisfy (in `tsconfig.app.json`): `strict`, `noUnusedLocals`, `noUnusedParameters`, `erasableSyntaxOnly`, `noFallthroughCasesInSwitch`, `noUncheckedSideEffectImports`, plus `verbatimModuleSyntax` and `moduleDetection: force`. JSX is `preserve` with `jsxImportSource: solid-js`. The `@/*` → `src/*` path alias is declared here (and mirrored in `vite.config.ts`).

## Commands and the package manager

Bun is the package manager: `bun.lock` is present and there is no `package-lock.json` / `yarn.lock` / `pnpm-lock.yaml`.

- `just build-web` → `cd web && bun i && bun run build` — the canonical way to verify a change to the web apps compiles.
- `just build` → `build-backend build-web`.
- `just dev-web` → `cd web && bun run dev`, and `just dev` runs it beside the API.

## Embedding

The `caravel` binary embeds `web/dist` with rust-embed. A release build reads it at compile time, so the Dockerfile builds the web apps first and copies `web/dist` in before compiling. A debug build reads it from disk while running, so `cargo run` serves whatever `bun run build` last produced. Without a build, the binary still compiles, and its pages answer that the web apps aren't built.

## Runtime configuration

The server writes each app's configuration into its page as JSON in `<script id="config" type="application/json">`, escaping `<` so a value can't end the element. The values come from the server's environment:

| Value | Server setting | Read as |
| --- | --- | --- |
| `api_url` | `API_URL` | `API_URL` |
| `relay_domains` | `RELAY_DOMAINS` | `RELAY_DOMAIN`, the first, which new relays are created under |
| `platform_name` | `PLATFORM_NAME` | `PLATFORM_NAME` |
| `platform_logo` | `PLATFORM_LOGO` | `PLATFORM_LOGO` |
| `flotilla_url` | `FLOTILLA_URL` | `FLOTILLA_URL` |
| `relay` (portal only) | the relay the host belongs to | `PORTAL_RELAY` |

`src/shared/lib/config.ts` reads each once into a named constant. Under `vite dev` there is no server page, so each falls back to a `VITE_*` variable from `web/.env` (`VITE_API_URL`, `VITE_RELAY_DOMAIN`, `VITE_PLATFORM_NAME`, `VITE_PLATFORM_LOGO`, `VITE_FLOTILLA_URL`), declared in `ImportMetaEnv` in `src/global.d.ts` and listed in `web/.env.template`; the portal's relay falls back to a `?relay=<id>` query parameter. `VITE_PORT` is read only by `vite.config.ts`, for the dev server's port.

A new configuration value is added in four places: what the server's web settings write into each page, `config.ts`, `global.d.ts`, and `.env.template`.

`global.d.ts` is the single ambient-declarations file: it also declares `Window.HSStaticMethods` (Preline) and `Window.nostr?` (NIP-07). Extend the existing interfaces there rather than re-declaring elsewhere.
