floo Config Files

## floo.app.toml - Primary Config Format

  All apps use floo.app.toml. Services are declared inline with type, port, and path:

  [app]
  name = "my-app"

  [services.web]
  type = "web"
  path = "."
  port = 3000
  ingress = "public"                   # public = internet-facing, internal = only other services
  env_file = ".env"
  dev_command = "npm run dev"          # command to run for `floo dev`
  migrate_command = "npx prisma migrate deploy"  # optional, runs after deploy

  Multi-service app (each service in its own subdirectory):

  [app]
  name = "my-app"

  [services.api]
  type = "api"
  path = "./api"
  port = 8080
  ingress = "public"

  [services.web]
  type = "web"
  path = "./web"
  port = 3000
  ingress = "public"

  Background worker (Sidekiq / Celery / Active Job) sharing the web codebase:

  [services.web]
  type = "web"
  path = "."
  port = 3000
  dev_command = "bin/dev"

  [services.worker]
  type = "worker"
  path = "."                          # same build as web -> same Dockerfile + CMD
  port = 3000                          # required even for workers (the runtime needs a port)
  ingress = "internal"                # no public HTTP
  command = "bundle exec sidekiq"     # REQUIRED: without it the worker boots the web command

  Without `command` on a shared-build worker, preflight fails: in production every
  service at the same path runs the same Dockerfile CMD, so the worker would boot
  the web process and silently never drain its queue.

## floo.service.toml - Legacy Single-Service Format

  Still supported for backward compatibility. Single-service apps may use:

  [app]
  name = "my-app"

  [service]
  name = "web"
  port = 3000
  type = "web"
  ingress = "public"

  Prefer floo.app.toml for new apps - it supports managed services (postgres,
  redis, storage), cron jobs, and multi-service apps in one file.

## Service Fields (floo.app.toml inline or floo.service.toml)

  dev_command      - command to run locally for `floo dev`
                     e.g., "npm run dev", "uvicorn app.main:app --reload"

  migrate_command  - optional command run as a one-off job after every
                     deploy (against the dev schema) and after every promote
                     (against the prod schema). Non-fatal: a failure is logged
                     but does not block the deploy from going live.
                     e.g., "alembic upgrade head", "npx prisma migrate deploy"

  command          - optional PRODUCTION start command; overrides the image's
                     Dockerfile CMD. Omit it to run the Dockerfile CMD (default).
                     REQUIRED on a worker that shares a build (same path) with a
                     web/api service, so the worker runs its own process instead
                     of the web command. Runs via `sh -c <command>` as written;
                     prefix `exec` for SIGTERM/graceful shutdown (Docker pattern).
                     e.g., "bundle exec sidekiq", "celery -A app worker"

  domain           - optional custom domain for this service
                     e.g., "api.example.com"

## Inline vs Delegated

  These modes are mutually exclusive per service. If a service has type and
  port inline in floo.app.toml, there must not be a floo.service.toml in
  that service's subdirectory. The CLI fails preflight if both are present.

## Managed Services (in floo.app.toml)

  [managed.default]
  type = "postgres"

  [managed.cache]
  type = "redis"

  [managed.uploads]
  type = "storage"

  Missing declared resources are provisioned by a git-triggered deploy.
  Credentials are injected as env vars. Removing a deployed declaration is
  a tier-3 change that requires human approval and schedules a rescindable
  teardown; it does not delete provider data. Terminal deletion still uses
  the explicit services lifecycle after resolving and confirming the exact
  resource.

  Legacy [postgres], [redis], and [storage] sections remain readable for
  existing apps. Use `floo docs services` and the installed migration help
  before changing a legacy data-bearing declaration.

## Resource Limits (optional)

  Place [resources] in floo.app.toml (app-wide defaults) or set per-service
  fields inside [services.<name>] to override.

  [resources]
  cpu = "1"             # CPU cores (0.25 to 8)
  memory = "512Mi"      # Memory (128Mi to 32Gi)
  max_instances = 3     # Max HTTP autoscale instances (platform default: 3)

  Omitted HTTP min_instances is 1 for paid production and 0 for Free, dev,
  and preview. Set min_instances = 0 explicitly to opt paid production into
  scale-to-zero. The warm default is about $24.64/month for the default HTTP
  shape before request traffic and credits, or about $36.96/month with the
  floo-managed Postgres default shape.

  Global min_instances applies only to web/api services. Workers use the
  per-service `instances` field and never inherit this HTTP autoscaling floor.
  See `floo docs scaling` for exact on-demand, warm, running-worker, and
  paused-worker recipes.

## Environment Overrides (in floo.app.toml)

  Per-environment access_mode values are accepted by the parser but are not
  applied by push deploys today. Set one access_mode under [app] for both
  environments. Per-environment edge policy is supported under
  [environments.dev.edge] and [environments.prod.edge].

## Cron Jobs ([cron.<name>])

  Scheduled jobs are declared in floo.app.toml - never created by the CLI.
  Each [cron.<name>] section becomes a managed cron job that's reconciled
  on every deploy (added, updated, or removed to match config).

  [cron.daily-report]
  schedule = "0 9 * * *"                  # cron expression: 9am UTC daily
  command  = "python -m reports.daily"    # executed inside the service container
  service  = "api"                         # which service's image to run in
  timeout  = 600                            # max seconds (default 300, optional)

  [cron.cleanup]
  schedule = "*/5 * * * *"                 # every 5 minutes
  command  = "node scripts/cleanup.js"
  service  = "worker"

  Fields:

  - schedule (required) - standard cron expression in UTC
  - command  (required) - shell command run inside the target service's container
  - service  (required) - name of a [services.<name>] entry; that image is reused
  - timeout  (optional) - max execution seconds; default 300

  CLI surface (read-only + manual trigger):

    floo cron list --app my-app              # list jobs and last run status
    floo cron show <name> --app my-app       # details for one job
    floo cron run daily-report --app my-app  # trigger one off-schedule

  Long-form guide: https://getfloo.com/docs/guides/cron-jobs
  Full config schema: https://getfloo.com/docs/reference/config-spec

## Environment Variables in Multi-Service Apps

  Custom env values are scoped per service. Managed credentials are attached
  declaratively by handle:

    [services.api.env]
    managed = ["postgres", "redis:cache"]

    [services.worker.env]
    managed = ["redis:cache"]

    [services.web.env]
    managed = []

  Scoping rules:

  - Single-service app: env vars go to the only service (no flag needed)
  - Multi-service app with 1 service: auto-targets that service
  - Multi-service app with 2+ services: --service is REQUIRED

  SECURITY: Secrets set on a frontend service (web, dashboard) end up in
  the container runtime. Build-time vars (VITE_*, NEXT_PUBLIC_*, REACT_APP_*)
  are baked into the JS bundle and visible to end users. Never set backend
  secrets (DATABASE_URL, API keys) on frontend services.

  Recommended pattern for multi-service apps:

    # Backend secrets - api/worker only
    floo env set LINEAR_API_KEY --stdin --secret --service api

    # Frontend config - web only (public, not secret)
    floo env set VITE_API_URL=https://my-app.getfloo.com/api --service web

  List env vars per service:

    floo env list --service api
    floo env list --service web

## Write-Only Secrets (--secret)

  Mark a variable write-only so floo never returns its value in plaintext,
  from any endpoint. Deploys still receive it.

    floo env set STRIPE_KEY --stdin --secret     # value from stdin, write-only
    floo env import .env.production --secret     # every imported var write-only

  What write-only means:

  - `env get` refuses with ENV_VAR_WRITE_ONLY (there is no reveal flag)
  - `env list` shows the row as `******** (write-only)`
  - Exports return `value: null` with `is_secret: true` for the row
  - `floo dev` / `floo run` withhold it and print the withheld key names
  - To change it: set a new value. To make it readable again: unset it,
    then set a fresh value without --secret. A plain `env set` without the
    flag keeps the write-only marker (it never silently downgrades).

  Build-time vars (VITE_*, NEXT_PUBLIC_*, REACT_APP_*) refuse --secret:
  their values are baked into the public JS bundle, so write-only would be
  a false promise.

  Managed service env vars are generated at app scope, then attached to
  services by [services.<name>.env] managed:

    [services.web.env]
    managed = []

    [services.api.env]
    required = ["STRIPE_SECRET_KEY"]
    managed = ["postgres", "redis"]

  If no service declares managed, floo preserves legacy all-service injection.
  Once any service declares it, omitted services receive no managed credentials.
  `floo preflight --json` shows the exact env_injection_plan.

## Commands

  floo init <name>   - generate config files interactively
  floo preflight     - validate config before deploying
