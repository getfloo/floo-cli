---
name: floo-services
description: >
  Floo managed services: Postgres, Redis, and Storage. Use when adding
  a database, cache, or file storage to a Floo app, or when working with
  DATABASE_URL, REDIS_URL, or STORAGE_BUCKET environment variables.
---

# Floo Managed Services

Floo provisions and manages Postgres, Redis, and Storage as platform services. All credentials are delivered as environment variables — never hardcode them.

## How to Provision

Managed services are declared in `floo.app.toml` and auto-provisioned on the first deploy.

**There is no CLI command to imperatively provision a managed service.** Declare them in `floo.app.toml` and they are auto-provisioned on deploy.

## Managed Service Tiers

All tiers are available on every plan. Choose based on workload, not plan.

| | Basic (default) | Standard | Performance |
|---|---|---|---|
| Postgres connections | 5 | 15 | 50 |
| Statement timeout | 30s | 60s | 120s |
| Idle-in-transaction | 60s | 120s | 300s |
| work_mem | 64 MB | 128 MB | 256 MB |

**When to use each:**
- `basic` — most web apps, single-service apps, CRUD workloads (recommended default)
- `standard` — multiple services sharing one DB, moderate reporting queries
- `performance` — high-concurrency APIs, batch processing, heavy analytics

Redis and Storage tiers default to `basic` — no functional difference today.

## Postgres (Managed)

**Declare in `floo.app.toml`:**

```toml
[postgres]
tier = "basic"    # optional, defaults to basic
```

Then deploy — Floo auto-provisions on first deploy.

**What happens:**
- Floo creates a dedicated schema and role on the managed Postgres instance
- The role has limited privileges — no superuser, no access to other apps' data
- Connection limits and statement timeouts are set per tier
- pgvector extension is enabled for vector/embedding workloads

**Env vars created:** `DATABASE_URL` — a full `postgresql://` connection string with app-specific role credentials.

**How to connect:** Read `DATABASE_URL` from environment. Pass it directly to your ORM or database driver. Do not parse, modify, or reconstruct it.

```python
# Python (any ORM)
import os
database_url = os.environ["DATABASE_URL"]
```

```javascript
// Node.js (any driver)
const databaseUrl = process.env.DATABASE_URL;
```

```rust
// Rust
let database_url = std::env::var("DATABASE_URL")?;
```

**Migrations:** Run through the same `DATABASE_URL` role. The role has DDL privileges on its own schema.

**Rules:**
- NEVER hardcode the connection string in source code
- NEVER use admin/superuser credentials in application code
- NEVER share `DATABASE_URL` between environments (dev vs prod)
- NEVER construct your own connection string — always use the one Floo provides
- NEVER store the connection string in `floo.app.toml` or `floo.service.toml`

## Redis (Managed)

**Declare in `floo.app.toml`:**

```toml
[redis]
```

Then deploy — Floo auto-provisions on first deploy.

**What happens:**
- Floo provisions a dedicated Redis instance via Upstash (serverless Redis)
- TLS-encrypted by default

**Env vars created:** `REDIS_URL` — a `rediss://` connection string (note: `rediss` with double-s means TLS).

**How to connect:** Read `REDIS_URL` from environment. Pass directly to your Redis client.

```python
# Python
import os
redis_url = os.environ["REDIS_URL"]
```

```javascript
// Node.js
const redisUrl = process.env.REDIS_URL;
```

**Rules:**
- NEVER hardcode the Redis URL
- NEVER use `redis://` (non-TLS) — Floo provisions with TLS (`rediss://`)
- NEVER store sensitive application data in Redis without TTL — Redis is a cache, not a durable store

## Storage (Managed)

**Declare in `floo.app.toml`:**

```toml
[storage]
```

Then deploy — Floo auto-provisions on first deploy.

**What happens:**
- Floo creates a storage bucket scoped to your app
- Access is via signed URLs — no direct bucket credentials in your app

**Env vars created:**
- `STORAGE_BUCKET` — the bucket name
- `STORAGE_URL` — the signed URL API endpoint for uploads/downloads

**How to upload/download:** Use `STORAGE_URL` to request signed URLs. Do NOT access the bucket directly.

**Signed URL endpoint** (value of `STORAGE_URL`):

```
POST {STORAGE_URL}
```

Request body:
```json
{
  "method": "PUT",
  "object_path": "uploads/photo.jpg",
  "expiration_seconds": 3600,
  "content_type": "image/jpeg"
}
```

Response:
```json
{
  "url": "https://storage.googleapis.com/...",
  "method": "PUT",
  "expires_in_seconds": 3600,
  "object_path": "uploads/photo.jpg",
  "bucket": "floo-app-..."
}
```

Upload example (using STORAGE_URL env var):
```bash
# 1. Get a signed upload URL
curl -X POST "$STORAGE_URL" \
  -H "Authorization: Bearer <api-key>" \
  -H "Content-Type: application/json" \
  -d '{"method": "PUT", "object_path": "uploads/photo.jpg", "expiration_seconds": 3600}'

# 2. Upload directly to the signed URL
curl -X PUT "<signed_url>" -H "Content-Type: image/jpeg" --data-binary @photo.jpg
```

Download example:
```bash
# 1. Get a signed download URL
curl -X POST "$STORAGE_URL" \
  -H "Authorization: Bearer <api-key>" \
  -H "Content-Type: application/json" \
  -d '{"method": "GET", "object_path": "uploads/photo.jpg", "expiration_seconds": 3600}'

# 2. Download from the signed URL
curl "<signed_url>" -o photo.jpg
```

**Rules:**
- NEVER store GCP service account keys in your application
- NEVER access the bucket directly via GCS client libraries — use signed URLs
- NEVER make the bucket public — signed URLs handle access control

## Cron Jobs (Managed)

**Declare in `floo.app.toml`:**

```toml
[cron.daily-report]
schedule = "0 9 * * *"        # cron expression (9am UTC daily)
command = "python scripts/report.py"
service = "worker"            # which service's image to run in
timeout = 600                 # max execution seconds (default 300)

[cron.cleanup]
schedule = "*/5 * * * *"      # every 5 minutes
command = "node cleanup.js"
service = "api"
```

Then deploy — Floo syncs cron jobs on every deploy (creates new ones, updates changed ones, deletes removed ones).

**What happens:**
- Floo creates a CronJob record for each `[cron.<name>]` section
- The job runs inside the specified service's container image
- Jobs are scoped to the app and environment (dev/prod have separate cron jobs)

**Fields:**
- `schedule` — standard cron expression (required)
- `command` — shell command to execute (required)
- `service` — which service's image to run the command in (required)
- `timeout` — max execution time in seconds, default 300 (optional)

**CLI commands (read-only):**

```bash
floo cron list --app my-app              # list all cron jobs and last run status
floo cron run daily-report --app my-app  # manually trigger a cron job
```

**Rules:**
- Cron jobs are defined in config only — no CLI command to create them
- The `service` field must match a service name defined in the same `floo.app.toml`
- Use `floo cron list` to verify jobs were synced after deploy
- Use `floo cron run` to test a job without waiting for its schedule

## Multi-Service Discovery

In multi-service apps, each service runs on a separate hostname. Floo automatically injects discovery env vars so services can find each other:

- `API_URL` — URL of the `api` service (injected into `web` and `worker`)
- `WEB_URL` — URL of the `web` service (injected into `api` and `worker`)
- `WORKER_INTERNAL_URL` — URL of the `worker` service
- `VITE_API_URL` — same as `API_URL`, prefixed for Vite/React frontends
- `FLOO_ALLOWED_ORIGINS` — comma-separated list of public service URLs (for CORS)

**CRITICAL: Do not use relative paths like `/api/v1/...` in frontend code.** In multi-service apps, the API is on a different hostname than the web frontend. Use the discovery env var instead:

```typescript
// WRONG — hits the web service, not the API
const API_BASE = "/api/v1";

// RIGHT — uses the injected discovery URL
const API_BASE = `${import.meta.env.VITE_API_URL || ""}/api/v1`;
```

For server-side code (Node.js, Python), use `API_URL` or `WEB_URL` from `process.env` / `os.environ`.

## General Rules

- All managed service credentials arrive as environment variables
- Use `floo env list --app my-app` to see what's provisioned
- Use `floo services list --app my-app` to see service status and tiers
- Managed services are per-environment — dev and prod have separate instances
- For local development, set the same env var names in your `.env` file pointing to local services
