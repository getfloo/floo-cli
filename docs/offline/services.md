floo Services

An app contains one or more services. Each service is independently
deployable. floo distinguishes two kinds by lifecycle:

  App services      - your code and shape, declared in floo.app.toml
  Managed services  - postgres/redis/storage, declared in floo.app.toml
                      with explicit CLI lifecycle actions for stored data

The split matters: git owns auditable intent, but a deploy never infers
permission to destroy stored data. Removing a managed-service declaration
does not delete the resource. Destruction is always an explicit CLI action
with confirmation for the exact app, environment, type, and name.

## App Services (your code)

  web     - HTTP server facing the internet (default for apps with a frontend)
  api     - HTTP server for backend APIs
  worker  - background process (no incoming HTTP traffic)

  Availability and cost/latency choices are declared with `min_instances` for
  web/api and `instances` for workers. See: floo docs scaling

  Paid production web/api services default to one warm instance when
  min_instances is omitted. The default bills continuously. Set
  min_instances = 0 to opt into scale-to-zero; Free, dev, and preview default
  to zero.

  Declare inline in floo.app.toml with type, port, and path. A deploy applies
  the declared service shape. It does not infer destructive intent for a
  production service merely because a declaration disappeared.

  See: floo docs config

## Managed Services (postgres, redis, storage)

  Managed services are stateful. Declare auditable intent in floo.app.toml:

    [managed.primary]
    type = "postgres"
    tier = "basic"

    [managed.cache]
    type = "redis"

    [managed.uploads]
    type = "storage"

  A git-triggered deploy provisions a declared service that is missing and
  applies supported declared settings such as tier. Omission is orphan-safe:
  it never destroys an existing database, cache, or bucket.

  Read and explicit lifecycle surfaces:

    floo services show primary --app <name>       # inspect
    floo services list --app <name>               # see everything
    floo services add postgres --app <name>       # explicit provision
    floo services remove postgres --app <name>    # tier-3 destructive

  `floo services add` and `floo services remove` also update
  .floo/services.lock. Commit that audit record when using the operational
  surface. The manifest remains the source for declared intent; the live
  platform is authoritative for whether stateful data exists.

  Connection credentials are injected at runtime, never stored in the
  lock file or in your repo:
    postgres → DATABASE_URL + PGHOST/PGPORT/PGDATABASE/PGUSER/PGPASSWORD
    redis    → REDIS_URL
    storage  → STORAGE_BUCKET (read/write via the GCS SDK over ADC)

  Your app reaches storage with the native GCS SDK (Rails Active Storage
  `:google` in proxy mode). floo runs your container as a service account
  with read/write on the bucket, so ADC just works - no key file, no
  project id. STORAGE_URL is a floo operator endpoint, not your app's
  runtime path, and S3-compatible SDKs are not supported.
  Full guide: https://getfloo.com/docs/guides/cloud-storage

  Managed Storage buckets keep noncurrent object versions for 30 days.
  To recover an overwritten or deleted object:

    floo storage versions uploads/report.json --app <name>
    floo storage restore uploads/report.json --generation <generation> --app <name>

  Add --env prod for the production bucket. Restores copy the selected
  generation back to the live object path and are audited.

  Postgres ships with pgvector enabled. The `vector` type resolves
  unqualified: use it in migrations and queries with no CREATE EXTENSION
  and no schema prefix. Rails (`t.vector`), Django, SQLAlchemy, and Prisma
  all emit the bare type. Full guide: https://getfloo.com/docs/guides/databases

  Preview database branches are preview-owned managed Postgres branches.
  Inspect them from the terminal:

    floo db branches list <preview-slug> --app <name>
    floo db branches show <preview-slug> --app <name> --name default
    floo db branches reset <preview-slug> --app <name> --yes

  Reset drops and recreates only the preview branch. Dev and prod databases
  are untouched, and JSON output never includes plaintext credentials.

  In multi-service apps, attach credentials by managed handle:
    [services.api.env]
    managed = ["postgres:primary", "redis:cache"]

    [services.web.env]
    managed = []

  Single-service apps can use top-level [env] managed = [] in
  floo.service.toml to opt out of managed credentials entirely.

## Managed Service Tiers

  All tiers are available on every plan. Only Postgres tiers have
  functional differences today:

                Basic (default)   Standard        Performance
  Connections   5                 15              50
  Query timeout 30s               60s             120s
  Idle timeout  60s               120s            300s
  work_mem      64 MB             128 MB          256 MB

  Start with basic. Upgrade to standard for multi-service apps or
  reporting queries. Use performance for high-concurrency workloads.

  Set `tier = "standard"` in the matching [managed.<name>] declaration.
  For an explicit operational provision, inspect `floo services add --help`.

## Legacy [postgres] / [redis] / [storage] in floo.app.toml

  Older top-level sections remain readable for compatibility. Do not rename
  or remove a data-bearing declaration from memory. Run `floo preflight
  --json`, inspect `floo services list --json`, and follow the installed
  `floo services migrate --help` flow so the existing resource identity is
  preserved.

## Cron Jobs

  cron     - scheduled tasks that run inside a service's container
             Declare as [cron.<name>] sections in floo.app.toml with
             schedule, command, service. Still config-driven because
             crons are stateless reconcilable resources.

  [cron.daily-report]
  schedule = "0 9 * * *"
  command = "python scripts/report.py"
  service = "web"
  timeout = 600

## Routing

Multi-service apps share a single hostname with path-based routing. Each
environment gets its own subdomain (prod has no suffix, dev appends -dev):

  Prod:  app-name.on.getfloo.com/       → web service
         app-name.on.getfloo.com/api/   → api service
  Dev:   app-name-dev.on.getfloo.com/   → web (dev)
         app-name-dev.on.getfloo.com/api/ → api (dev)

This is automatic - no configuration needed. The gateway strips the /api
prefix before forwarding, so your FastAPI routes stay at the root:

  React: fetch("/api/users")
    → gateway routes to api service at /users
    → FastAPI handler: @app.get("/users")

Your API code does NOT need /api prefixes. The gateway handles it.
All services share the same origin, so cookies and auth work without CORS.

## localhost fallback diagnostics

Source validation treats localhost URLs as non-blocking warnings:

  - Explicit Node or Vite development guard: no warning.
  - Runtime-configured URL that may fall back to localhost:
    unverified_localhost_fallback (production value not proven from source).
  - No recognized development-only guard:
    hardcoded_localhost_fallback.

For production service-to-service calls, use the injected discovery variable
such as API_URL or a relative gateway path such as /api. Keep a localhost
default behind an explicit development-only guard.

## Service Types

  web     - serves the frontend (HTML/JS/CSS). Gets the root path (/).
  api     - serves the backend API. Gets the /api/ path prefix.
  worker  - background process (no incoming HTTP traffic).

  The only difference between web and api is the routing path. Both are
  HTTP servers, both can access managed services (postgres, redis, etc).

## The audit loop: every change ends with floo preflight

  Before calling any state change done, run `floo preflight` to confirm
  the resulting state matches intent. Unexpected diffs = silent
  corruption; investigate before pushing. The skill rule (see
  `.claude/skills/floo/SKILL.md`) makes this non-negotiable for agents.

## Commands

  floo services list --app <name>            - list all services (app + managed)
  floo services show <service> --app <name>  - details (no credentials in output)
  floo services add <type> --app <name>      - provision a managed service
  floo services remove <type> --app <name>   - permanently destroy (tier-3)
  floo services migrate --app <name>         - move legacy TOML → CLI state
