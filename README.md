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
  -e PLATFORM_LOGO=/caravel.png \
  -e ZOOID_DOMAINS=example.com \
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

### Zooid and TLS

Zooid (the relay engine) should run on its own server with [Caddy](https://caddyserver.com/) as the TLS-terminating reverse proxy. Keeping Zooid separate lets it scale independently and makes custom relay domains work without touching the Caravel host.

**Why Caddy?** Caravel supports tenant-facing custom domains, which require per-domain TLS certificates that are provisioned automatically. Caddy's [on-demand TLS](https://caddyserver.com/docs/automatic-https#on-demand-tls) handles this: it calls a Caravel endpoint before issuing each certificate, so only known domains get one.

#### Zooid server setup

On the Zooid server, create a `Caddyfile`:

```
{
  on_demand_tls {
    ask http://<caravel-host>:2892/domains/check
    interval 2m
    burst 5
  }
}

:443 {
  tls {
    on_demand
  }
  reverse_proxy localhost:3334
}
```

Replace `<caravel-host>` with the hostname or IP of the Caravel server. The `/domains/check` endpoint returns `200` only for a host some existing relay is actually served on — its `<subdomain>.<ZOOID_DOMAINS entry>` host, its verified custom domain, or a hostname redirecting to it — and `404` otherwise, so Caddy will only obtain a certificate for domains that have been registered.

Run Caddy and Zooid together, for example with Docker Compose:

```yaml
services:
  caddy:
    image: caddy:2
    ports:
      - "80:80"
      - "443:443"
    volumes:
      - ./Caddyfile:/etc/caddy/Caddyfile
      - caddy_data:/data
    restart: unless-stopped

  zooid:
    image: gitea.coracle.social/coracle/zooid
    environment:
      API_HOST: api.zooid.example.com
      API_WHITELIST: <hex-pubkey-matching-ROBOT_SECRET>
    volumes:
      - zooid_data:/app/data
    restart: unless-stopped

volumes:
  caddy_data:
  zooid_data:
```

Point a wildcard DNS record (`*.<domain>`) for each of your `ZOOID_DOMAINS` at this server's IP. Custom domains are pointed there by tenants via a CNAME to their relay's canonical subdomain; Caravel verifies the CNAME in the background and notifies Zooid once confirmed.

Set `ZOOID_API_URL` in Caravel's environment to the same value as zooid's `API_HOST` value, prefixed with the protocol, e.g. `https://api.zooid.example.com`.

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
| `ZOOID_DOMAINS` | `localhost` | Comma-separated domains relay subdomains may live under |

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
