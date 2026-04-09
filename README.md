# Caravel

A multi-tenant platform for hosting Nostr community relays, built on top of [zooid](https://github.com/coracle-social/zooid).

## Quick Start (Local Development)

### Prerequisites

- [Rust](https://rustup.rs/) (for the backend)
- [Bun](https://bun.sh/) (for the frontend)
- [just](https://github.com/casey/just) (task runner)
- [onchange](https://www.npmjs.com/package/onchange) (`npm i -g onchange`, used by `just dev` for backend file watching)
- [Docker](https://www.docker.com/) (for zooid)

### 1. Start zooid

Zooid is the relay engine that Caravel manages. The backend authenticates to zooid's API using [NIP-98](https://github.com/nostr-protocol/nips/blob/master/98.md), signing requests with a Nostr secret key. Zooid must be configured to accept requests from the corresponding public key.

Generate a keypair to use for this. The **hex secret key** goes in the backend's `ZOOID_API_SECRET`, and the **hex public key** goes in zooid's `API_WHITELIST`.

```sh
docker run -it \
  -p 3334:3334 \
  -e API_HOST=127.0.0.1:3334 \
  -e API_WHITELIST=<hex-pubkey-matching-ZOOID_API_SECRET> \
  -v ./config:/app/config \
  -v ./media:/app/media \
  -v ./data:/app/data \
  ghcr.io/coracle-social/zooid
```

### 2. Configure the backend

Copy the template and fill in the required values:

```sh
cp backend/.env.template backend/.env
```

At minimum for local dev, set:

| Variable | Value | Notes |
|---|---|---|
| `ADMINS` | Your hex pubkey | Gives you admin access in the UI |
| `ZOOID_API_SECRET` | Hex Nostr secret key | The keypair whose pubkey you put in `API_WHITELIST` above |
| `RELAY_DOMAIN` | `localhost` | Base domain appended to relay subdomains |

The rest of the defaults work as-is. `ROBOT_*`, `LIVEKIT_*`, billing, and Stripe vars are optional for basic local development.

### 3. Configure the frontend

```sh
cp frontend/.env.template frontend/.env
```

The defaults (`VITE_API_URL=http://127.0.0.1:3000`) point at the backend and work out of the box.

### 4. Install dependencies and run

```sh
cd frontend && bun install && cd ..
just dev
```

This starts the backend (with auto-reload on file changes) at `http://127.0.0.1:3000` and the frontend at `http://127.0.0.1:5173`.

## Project docs

- Frontend: [frontend/README.md](./frontend/README.md)
- Backend: [backend/README.md](./backend/README.md)
