---
name: floo-services
description: >
  Floo managed services: Postgres, Redis, and Storage. Use when adding
  a database, cache, or file storage to a Floo app, or when working with
  DATABASE_URL, REDIS_URL, or STORAGE_BUCKET environment variables.
---

# Floo Managed Services

Floo provisions and manages Postgres, Redis, and Storage as platform services. All credentials are delivered as environment variables — never hardcode them.

## Postgres (Managed)

**Provision:** `floo services add postgres --app my-app` (optionally `--tier basic|standard|performance`)

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

**Provision:** `floo services add redis --app my-app`

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

**Provision:** `floo services add storage --app my-app`

**What happens:**
- Floo creates a GCS bucket scoped to your app
- Access is via signed URLs — no direct bucket credentials in your app

**Env vars created:** `STORAGE_BUCKET` — the bucket name.

**How to upload/download:** Use Floo's signed URL API endpoint, not direct GCS access. The signed URL is time-limited and scoped to a specific object path.

**Rules:**
- NEVER store GCP service account keys in your application
- NEVER access the bucket directly via GCS client libraries — use signed URLs
- NEVER make the bucket public — signed URLs handle access control

## General Rules

- All managed service credentials arrive as environment variables
- Use `floo env list --app my-app` to see what's provisioned
- Use `floo services list --app my-app` to see service status and tiers
- Managed services are per-environment — dev and prod have separate instances
- For local development, set the same env var names in your `.env` file pointing to local services
