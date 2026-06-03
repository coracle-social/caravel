# Caravel

A multi-tenant platform for hosting Nostr community relays, built on top of [zooid](https://github.com/coracle-social/zooid).

## Deployment

Caravel ships as a single Docker image, built from the repository-root [`Dockerfile`](./Dockerfile) and published by [`.gitea/workflows/docker-publish.yml`](./.gitea/workflows/docker-publish.yml) as `gitea.coracle.social/coracle/caravel`. One container runs both services: the backend API on port `2892` and the frontend on port `3000`.

Both the backend and the frontend are compiled into the image at build time. The frontend is built with placeholder config that the entrypoint replaces with the real values when the container starts, so one image can be deployed with any configuration — no rebuild required.

Caravel needs a reachable [zooid](https://github.com/coracle-social/zooid) instance (the [Local Development](#local-development) section below shows how to run one). Substitute your own values for the placeholders below:

```sh
docker run -d \
  --name caravel \
  -p 2892:2892 \
  -p 3000:3000 \
  -v my-caravel-data:/app/data \
  -e PLATFORM_NAME=Caravel \
  -e RELAY_DOMAIN=example.com \
  -e APP_URL=https://example.com \
  -e ZOOID_API_URL=http://zooid:3334 \
  -e SERVER_URL=https://api.example.com \
  -e SERVER_ADMIN_PUBKEYS=<your-hex-pubkey> \
  -e SERVER_ALLOW_ORIGINS=https://example.com \
  -e ROBOT_SECRET=<hex-nostr-secret-key> \
  -e ROBOT_NAME=Caravel \
  -e ROBOT_DESCRIPTION="Relay manager bot" \
  -e ROBOT_PICTURE=https://example.com/robot.png \
  -e ROBOT_WALLET=<nwc-connection-uri> \
  -e ROBOT_OUTBOX_RELAYS=wss://relay.damus.io,wss://relay.primal.net,wss://nos.lol \
  -e ROBOT_INDEXER_RELAYS=wss://purplepag.es,wss://relay.damus.io,wss://indexer.coracle.social \
  -e ROBOT_MESSAGING_RELAYS=wss://auth.nostr1.com,wss://relay.keychat.io,wss://relay.ditto.pub \
  -e BLOSSOM_S3_ENDPOINT=https://s3.example.com \
  -e BLOSSOM_S3_REGION=us-east-1 \
  -e BLOSSOM_S3_BUCKET=caravel-blossom \
  -e BLOSSOM_S3_ACCESS_KEY=<s3-access-key> \
  -e BLOSSOM_S3_SECRET_KEY=<s3-secret-key> \
  -e LIVEKIT_URL=wss://livekit.example.com \
  -e LIVEKIT_API_KEY=<livekit-api-key> \
  -e LIVEKIT_API_SECRET=<livekit-api-secret> \
  -e STRIPE_SECRET_KEY=<stripe-secret-key> \
  gitea.coracle.social/coracle/caravel
```

Notes:

- Every backend variable above is **required** — the server exits on startup if any is missing or empty. See [`backend/.env.template`](./backend/.env.template) for what each one means.
- `ROBOT_SECRET` is the robot account's hex nostr secret key; its pubkey must be in zooid's `API_WHITELIST` (the backend signs zooid requests with it via NIP-98).
- `SERVER_ALLOW_ORIGINS` must include the frontend's public origin, or browsers will be blocked by CORS.
- The frontend's `VITE_` prefixed env variables are automatically populated from the provided env variables.
- `-v my-caravel-data:/app/data` persists the SQLite database.

To build the image yourself instead of pulling it:

```sh
docker build -t caravel .
```

## Local Development

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
  gitea.coracle.social/coracle/zooid
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

The defaults (`VITE_API_URL=http://127.0.0.1:2892`) point at the backend and work out of the box.

### 4. Install dependencies and run

```sh
cd frontend && bun install && cd ..
just dev
```

This starts the backend (with auto-reload on file changes) at `http://127.0.0.1:2892` and the frontend at `http://127.0.0.1:5173`.

## Project docs

- Frontend: [frontend/README.md](./frontend/README.md)
- Backend: [backend/README.md](./backend/README.md)
