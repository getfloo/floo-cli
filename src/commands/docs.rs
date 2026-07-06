use crate::output;

const OVERVIEW: &str = "\
Floo — Deploy web apps from the terminal.

Floo is a deployment platform. The CLI is the primary interface for deploying,
managing, and observing your apps. All source comes from GitHub — the CLI
never uploads code.

## Core Concepts

- **Apps** are the top-level unit. Each app has a unique name and URL.
- **Services** are deployable components inside an app (web servers, APIs, workers, databases).
- **Deploys** are immutable snapshots of your code, built into containers and deployed to the cloud.

## First Deploy

  1. `floo auth login` — authenticate
  2. `floo init <name>` — scaffold config files (local only)
  3. `floo apps github connect owner/repo` — connect to GitHub (triggers first deploy)
  4. `floo apps show <name>` — see your app's URL and status

  After the first deploy, push to GitHub to deploy: `git push origin main`.
  Watch progress with `floo deploys watch --app <name>`.
  Use `floo redeploy` only to force a redeploy (e.g., after updating env vars).
  Use `floo preflight` to validate config before pushing.

## Learn More

  floo docs golden-path — golden path and decision table
  floo docs quickstart — end-to-end walkthrough
  floo docs build      — stack-specific build journeys (Next.js, Rails, FastAPI, Django, Express)
  floo docs nextjs     — build and deploy a Next.js app on floo (end-to-end)
  floo docs rails      — build and deploy a Rails app on floo (end-to-end)
  floo docs fastapi    — build and deploy a FastAPI app on floo (end-to-end)
  floo docs django     — build and deploy a Django app on floo (end-to-end)
  floo docs express    — build and deploy an Express app on floo (end-to-end)
  floo docs templates  — copy-paste app structures (React+FastAPI, Next.js, etc.)
  floo docs services   — service types and managed services (alias: storage)
  floo docs edge       — edge routes, the IP/CIDR firewall, enforcement order
  floo docs previews   — command-line preview sandboxes for remote branches
  floo docs config     — config file formats with examples (alias: app-toml)
  floo docs cron       — [cron.<name>] schema, schedules, and CLI surface
  floo docs deploy     — detailed deploy flow and runtime detection
  floo docs auth       — add user authentication to your app
  floo docs notifications — control which emails floo sends you
  floo docs feedback   — report bugs, friction, or feature requests
  floo --help          — all available commands
  floo <command> --help — details for a specific command
";

const QUICKSTART: &str = "\
Floo Quickstart — End-to-End Walkthrough

## Prerequisites

  - Your code must be in a **GitHub repository** (public or private).
    Floo pulls source from GitHub — it does not upload local files.
  - The Floo GitHub App must be installed on your GitHub org/account.
    The CLI opens GitHub to grant access during `floo apps github connect`.

## Agents & CI (headless environments)

  Agents and CI pipelines can deploy without a browser:

  1. A human installs the Floo GitHub App on the org (one-time):
     https://github.com/apps/getfloo/installations/new
  2. The agent authenticates: floo auth login --api-key <key>
  3. The agent connects: floo apps github connect owner/repo --no-browser
     (--no-browser errors cleanly if the app is not installed, instead of
     trying to open a browser)
  4. Subsequent deploys: git push triggers automatic deploys via webhook

## 1. Install and Sign Up

  curl -fsSL https://getfloo.com/install.sh | bash
  floo auth login

  Opens a browser to sign up or log in. New users create an account automatically.
  In headless/CI environments: floo auth login --api-key <key>

## 2. Initialize Your Project

  cd my-project
  floo init my-app

  This writes floo.app.toml (with your service declared inline) and a Dockerfile
  locally. No app is registered on the platform yet.

## 3. (Optional) Add Managed Services

  Managed services are authored via the CLI, not floo.app.toml. This
  keeps stateful resources explicit — destroying a database is always an
  intentional command, never a TOML edit.

  floo services add postgres --app my-app
  floo services add redis --app my-app
  floo services add storage --app my-app

  Credentials arrive as runtime env vars (Postgres: DATABASE_URL + PG*,
  Redis: REDIS_URL, Storage: STORAGE_BUCKET + STORAGE_URL). The commands write .floo/services.lock,
  which you should commit so managed-service state changes are visible
  in `git diff` alongside code.

## 4. Validate Config

  floo preflight --json

  Checks config files, service graph, ports, and Dockerfiles locally — no
  auth or GitHub connection required. Fix any warnings before deploying.

## 5. Connect to GitHub and Deploy

  floo apps github connect owner/my-project

  This does three things:
  1. Creates the app on floo (if it doesn't exist)
  2. Connects your GitHub repo as the source
  3. Triggers the first deploy (source pulled from GitHub, built, deployed)

  Use --no-deploy to skip the automatic deploy.

## 6. Check Status

  floo apps show my-app
  floo logs query --app my-app

## 7. Subsequent Deploys

  Push to GitHub — the webhook triggers a deploy automatically:

  git push origin main
  floo deploys watch --app my-app

  Use `floo redeploy --app my-app` only when you need to redeploy without
  a code change (e.g., after updating env vars).

## 8. Local Development

  floo dev --app my-app --service web

  Runs your service locally with live Cloud SQL access and the same env vars
  as the deployed version. Requires dev_command set on the service in floo.app.toml.

## What Creates What

  floo init                — local config files only (no API call)
  floo redeploy            — force a redeploy of an existing app (no code-change required)
  floo apps github connect — creates app if needed, connects GitHub, triggers first deploy
  floo services add        — provisions a managed service (postgres/redis/storage) for the app
  Cron jobs are still declared in floo.app.toml ([cron.<name>]) and
  provisioned automatically on deploy — they're stateless and config-driven.
";

const SERVICES: &str = "\
Floo Services

An app contains one or more services. Each service is independently
deployable. Floo distinguishes two kinds by how they are authored:

  App services      — your code, declared in floo.app.toml (stateless)
  Managed services  — postgres/redis/storage, authored via `floo services`
                      commands (stateful)

The split matters: removing a line from floo.app.toml that deleted a
database would be catastrophic, so managed services are never coupled to
config-file edits. Destruction is always an explicit CLI command with
confirmation.

## App Services (your code)

  web     — HTTP server facing the internet (default for apps with a frontend)
  api     — HTTP server for backend APIs
  worker  — background process (no incoming HTTP traffic)

  Declare inline in floo.app.toml with type, port, and path. Removing a
  service from floo.app.toml tears down the Cloud Run service on next
  deploy — recoverable from code, so declarative semantics are safe here.

  See: floo docs config

## Managed Services (postgres, redis, storage)

  Managed services are stateful. They hold your data and outlive any
  single deploy, so they live on the CLI surface — not in floo.app.toml.

  floo services add postgres --app <name>         # provision
  floo services show postgres --app <name>        # inspect
  floo services list --app <name>                 # see everything
  floo services remove postgres --app <name>      # tier-3 destructive
  floo services migrate --app <name>              # convert legacy TOML to CLI-managed

  On success, `floo services add/remove` updates .floo/services.lock
  (commit this file) so PR reviewers see managed-service state changes
  in `git diff` alongside code changes. The lock file is a record of
  state, not a source — platform is the source of truth.

  Connection credentials are injected at runtime, never stored in the
  lock file or in your repo:
    postgres → DATABASE_URL + PGHOST/PGPORT/PGDATABASE/PGUSER/PGPASSWORD
    redis    → REDIS_URL
    storage  → STORAGE_BUCKET (read/write via the GCS SDK over ADC)

  Your app reaches storage with the native GCS SDK (Rails Active Storage
  `:google` in proxy mode). floo runs your container as a service account
  with read/write on the bucket, so ADC just works — no key file, no
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

  In multi-service apps, attach those credentials per service:
    [services.api.env]
    managed = [\"postgres\", \"redis\"]

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

  Pass --tier on add: floo services add postgres --tier standard

## Legacy [postgres] / [redis] / [storage] in floo.app.toml

  The deprecated TOML surface is still honored during the transition
  window — apps with these sections auto-provision on first deploy and
  emit a deprecation warning on every subsequent deploy. To migrate:

    floo services migrate --app <name>   # zero data impact
    # Then delete the [postgres]/[redis]/[storage] sections from floo.app.toml
    # Commit the updated .floo/services.lock and push.

  The warning stops on the next deploy once the sections are gone.

## Cron Jobs

  cron     — scheduled tasks that run inside a service's container
             Declare as [cron.<name>] sections in floo.app.toml with
             schedule, command, service. Still config-driven because
             crons are stateless reconcilable resources.

  [cron.daily-report]
  schedule = \"0 9 * * *\"
  command = \"python scripts/report.py\"
  service = \"web\"
  timeout = 600

## Routing

Multi-service apps share a single hostname with path-based routing. Each
environment gets its own subdomain (prod has no suffix, dev appends -dev):

  Prod:  app-name.on.getfloo.com/       → web service
         app-name.on.getfloo.com/api/   → api service
  Dev:   app-name-dev.on.getfloo.com/   → web (dev)
         app-name-dev.on.getfloo.com/api/ → api (dev)

This is automatic — no configuration needed. The gateway strips the /api
prefix before forwarding, so your FastAPI routes stay at the root:

  React: fetch(\"/api/users\")
    → gateway routes to api service at /users
    → FastAPI handler: @app.get(\"/users\")

Your API code does NOT need /api prefixes. The gateway handles it.
All services share the same origin, so cookies and auth work without CORS.

## Service Types

  web     — serves the frontend (HTML/JS/CSS). Gets the root path (/).
  api     — serves the backend API. Gets the /api/ path prefix.
  worker  — background process (no incoming HTTP traffic).

  The only difference between web and api is the routing path. Both are
  HTTP servers, both can access managed services (postgres, redis, etc).

## The audit loop: every change ends with floo preflight

  Before calling any state change done, run `floo preflight` to confirm
  the resulting state matches intent. Unexpected diffs = silent
  corruption; investigate before pushing. The skill rule (see
  `.claude/skills/floo/SKILL.md`) makes this non-negotiable for agents.

## Commands

  floo services list --app <name>            — list all services (app + managed)
  floo services show <service> --app <name>  — details (no credentials in output)
  floo services add <type> --app <name>      — provision a managed service
  floo services remove <type> --app <name>   — permanently destroy (tier-3)
  floo services migrate --app <name>         — move legacy TOML → CLI state
";

const EDGE: &str = "\
Floo Edge Routes

Inspect the route table floo's edge is serving for an app. Use this when a
host, path, target service, access mode, or custom domain does not behave the
way you expected.

## List routes

  floo edge routes list --app my-app
  floo edge routes list --app my-app --env prod
  floo edge routes list --app my-app --env preview --json

The route table shows:

  host              Customer-facing floo host or custom domain
  path_prefix       Path prefix matched by the gateway
  environment       dev, prod, preview, or unscoped legacy route
  service           Target app service name and type
  policy            Effective access mode and app API-key requirement
  source            deploy, custom_domain, toml, or system
  source_of_truth   gateway_routes

JSON output is stable for agents:

  floo edge routes list --app my-app --env prod --json 2>/dev/null |
    jq '.data.routes[] | {host, path_prefix, access_mode, api_key_enabled, required_scope}'

The output deliberately omits raw Cloud Run backend URLs. Treat floo hosts and
custom domains as the public contract.

## Edge policy (IP/CIDR firewall, Team plan)

An ordered allow/deny rule list per app + environment, enforced at floo's
edge BEFORE the request body is read and before any managed auth. Rules are
evaluated top to bottom; first match wins; the default action applies when
no rule matches. Previews inherit the dev policy.

  # Office-only allowlist (everything else denied):
  floo edge policy set --env prod --rule allow:203.0.113.0/24 --default-action deny

  # Block one abusive network, allow everyone else:
  floo edge policy set --env prod --rule deny:198.51.100.0/24 --default-action allow

  floo edge policy get --env prod --json
  floo edge policy clear --env prod --yes

Also configurable in floo.app.toml (config wins on the next deploy):

  [edge]
  default_action = \"deny\"

  [[edge.rules]]
  action = \"allow\"
  cidr = \"203.0.113.0/24\"

  [environments.prod.edge]   # per-env override

Denied requests get 403 {\"code\":\"EDGE_POLICY_DENIED\"}; denial counts appear
in `floo analytics` as the rejection breakdown.

## Enforcement order

Requests pass gates in this order — an earlier denial short-circuits:

  1. Cloud provider edge (TLS, volumetric DDoS)   [floo-managed]
  2. Edge policy (this firewall)                  [yours]
  3. Managed auth (access_mode, app API keys)     [yours]
  4. Your app

The edge policy cannot see or bypass auth; auth never runs for a denied IP.

Full reference: https://getfloo.com/docs/cli/edge
";

const PREVIEWS: &str = "\
Floo Preview Sandboxes

Use `floo previews` when an agent needs an isolated, real floo deploy for a
pushed feature branch before opening or relying on a pull request preview.

## Source contract

Preview sandboxes deploy remote GitHub source only:

  git push origin feat/foo
  floo previews up --app my-app --branch feat/foo --wait

The CLI does not upload local dirty files or an archive from your checkout.
Push the branch first, or pass a remote commit/ref when you need an exact
remote revision.

## Lifecycle commands

  floo previews up --app my-app --branch feat/foo --wait --json
  floo previews list --app my-app --json
  floo previews status --app my-app feat/foo --json
  floo previews logs --app my-app feat/foo --follow
  floo previews delete --app my-app feat/foo --yes --json

Preview identifiers can be an exact slug, a preview URL, the source branch,
or `#123` when that PR number resolves to one preview. If the identifier is
ambiguous, use the exact slug from `floo previews list`.

## JSON contract

Non-streaming commands print one JSON object. Automation can rely on:

  app
  preview.slug
  source_branch
  deploy_id
  status
  url
  expires_at
  database_branches
  managed_resource_branches
  dev_prod_untouched: true

`up --wait` watches the deploy returned by the create call and exits non-zero
when that deploy fails.

## Isolation and cleanup

Preview sandboxes use the same managed-resource isolation as pull request
previews. floo-managed Postgres, Redis, and Storage get preview-owned
resources. If isolation cannot be provisioned, the command surfaces
PREVIEW_MANAGED_SERVICE_ISOLATION_UNAVAILABLE instead of falling back to dev
or prod credentials.

Preview managed-resource branches are visible through one command group:

  floo previews resources list <preview> --app <name>
  floo previews resources show <preview> --app <name> --resource redis:default
  floo previews resources reset <preview> --app <name> --resource postgres:default --yes

The resource key is shaped `type:name`, for example `postgres:default`,
`redis:cache`, or `storage:uploads`. Reset is preview-scoped and fails closed
with the API's named blocker when a provider cannot reset that resource yet.

`floo previews delete` tears down preview-owned Cloud Run services, managed
resources, gateway routes, and env vars. Dev and prod are untouched.

## Related commands

  floo db branches list <preview> --app <name>
  floo db branches show <preview> --app <name> --name default
  floo db branches reset <preview> --app <name> --name default --yes

Full guide: https://getfloo.com/docs/cli/previews
";

const CONFIG: &str = "\
Floo Config Files

## floo.app.toml — Primary Config Format

  All apps use floo.app.toml. Services are declared inline with type, port, and path:

  [app]
  name = \"my-app\"

  [services.web]
  type = \"web\"
  path = \".\"
  port = 3000
  ingress = \"public\"                   # public = internet-facing, internal = only other services
  env_file = \".env\"
  dev_command = \"npm run dev\"          # command to run for `floo dev`
  migrate_command = \"npx prisma migrate deploy\"  # optional, runs after deploy

  Multi-service app (each service in its own subdirectory):

  [app]
  name = \"my-app\"

  [services.api]
  type = \"api\"
  path = \"./api\"
  port = 8080
  ingress = \"public\"

  [services.web]
  type = \"web\"
  path = \"./web\"
  port = 3000
  ingress = \"public\"

  Background worker (Sidekiq / Celery / Active Job) sharing the web codebase:

  [services.web]
  type = \"web\"
  path = \".\"
  port = 3000
  dev_command = \"bin/dev\"

  [services.worker]
  type = \"worker\"
  path = \".\"                          # same build as web -> same Dockerfile + CMD
  port = 3000                          # required even for workers (Cloud Run needs a port)
  ingress = \"internal\"                # no public HTTP
  command = \"bundle exec sidekiq\"     # REQUIRED: without it the worker boots the web command

  Without `command` on a shared-build worker, preflight fails: in production every
  service at the same path runs the same Dockerfile CMD, so the worker would boot
  the web process and silently never drain its queue.

## floo.service.toml — Legacy Single-Service Format

  Still supported for backward compatibility. Single-service apps may use:

  [app]
  name = \"my-app\"

  [service]
  name = \"web\"
  port = 3000
  type = \"web\"
  ingress = \"public\"

  Prefer floo.app.toml for new apps — it supports managed services (postgres,
  redis, storage), cron jobs, and multi-service apps in one file.

## Service Fields (floo.app.toml inline or floo.service.toml)

  dev_command      — command to run locally for `floo dev`
                     e.g., \"npm run dev\", \"uvicorn app.main:app --reload\"

  migrate_command  — optional command run as a Cloud Run Job after every
                     deploy (against the dev schema) and after every promote
                     (against the prod schema). Non-fatal: a failure is logged
                     but does not block the deploy from going live.
                     e.g., \"alembic upgrade head\", \"npx prisma migrate deploy\"

  command          — optional PRODUCTION start command; overrides the image's
                     Dockerfile CMD. Omit it to run the Dockerfile CMD (default).
                     REQUIRED on a worker that shares a build (same path) with a
                     web/api service, so the worker runs its own process instead
                     of the web command. Runs via `sh -c <command>` as written;
                     prefix `exec` for SIGTERM/graceful shutdown (Docker pattern).
                     e.g., \"bundle exec sidekiq\", \"celery -A app worker\"

  domain           — optional custom domain for this service
                     e.g., \"api.example.com\"

## Inline vs Delegated

  These modes are mutually exclusive per service. If a service has type and
  port inline in floo.app.toml, there must not be a floo.service.toml in
  that service's subdirectory. The CLI fails preflight if both are present.

## Managed Services (in floo.app.toml)

  [postgres]
  tier = \"basic\"

  [redis]

  [storage]

  Auto-provisioned on first deploy. Credentials injected as env vars.

## Resource Limits (optional)

  Place [resources] in floo.app.toml (app-wide defaults) or set per-service
  fields inside [services.<name>] to override.

  [resources]
  cpu = \"1\"             # CPU cores (0.25 to 8)
  memory = \"512Mi\"      # Memory (128Mi to 32Gi)
  max_instances = 10    # Max autoscale instances

## Environment Overrides (in floo.app.toml)

  [environments.dev]
  access_mode = \"public\"

  [environments.prod]
  access_mode = \"accounts\"

## Cron Jobs ([cron.<name>])

  Scheduled jobs are declared in floo.app.toml — never created by the CLI.
  Each [cron.<name>] section becomes a managed cron job that's reconciled
  on every deploy (added, updated, or removed to match config).

  [cron.daily-report]
  schedule = \"0 9 * * *\"                  # cron expression: 9am UTC daily
  command  = \"python -m reports.daily\"    # executed inside the service container
  service  = \"api\"                         # which service's image to run in
  timeout  = 600                            # max seconds (default 300, optional)

  [cron.cleanup]
  schedule = \"*/5 * * * *\"                 # every 5 minutes
  command  = \"node scripts/cleanup.js\"
  service  = \"worker\"

  Fields:

  - schedule (required) — standard cron expression in UTC
  - command  (required) — shell command run inside the target service's container
  - service  (required) — name of a [services.<name>] entry; that image is reused
  - timeout  (optional) — max execution seconds; default 300

  CLI surface (read-only + manual trigger):

    floo cron list --app my-app              # list jobs and last run status
    floo cron show <name> --app my-app       # details for one job
    floo cron run daily-report --app my-app  # trigger one off-schedule

  Long-form guide: https://getfloo.com/docs/guides/cron-jobs
  Full config schema: https://getfloo.com/docs/reference/config-spec

## Environment Variables in Multi-Service Apps

  Env vars are scoped per service. In multi-service apps, you MUST specify
  which service receives the variable:

    floo env set DATABASE_URL=postgres://... --service api
    floo env set REDIS_URL=redis://... --service api --service worker

  Scoping rules:

  - Single-service app: env vars go to the only service (no flag needed)
  - Multi-service app with 1 service: auto-targets that service
  - Multi-service app with 2+ services: --service is REQUIRED

  SECURITY: Secrets set on a frontend service (web, dashboard) end up in
  the container runtime. Build-time vars (VITE_*, NEXT_PUBLIC_*, REACT_APP_*)
  are baked into the JS bundle and visible to end users. Never set backend
  secrets (DATABASE_URL, API keys) on frontend services.

  Recommended pattern for multi-service apps:

    # Backend secrets — api/worker only
    floo env set DATABASE_URL=postgres://... --service api
    floo env set LINEAR_API_KEY=lin_... --service api
    floo env set REDIS_URL=redis://... --service api --service worker

    # Frontend config — web only (public, not secret)
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
    required = [\"STRIPE_SECRET_KEY\"]
    managed = [\"postgres\", \"redis\"]

  If no service declares managed, floo preserves legacy all-service injection.
  Once any service declares it, omitted services receive no managed credentials.
  `floo preflight --json` shows the exact env_injection_plan.

## Commands

  floo init <name>   — generate config files interactively
  floo preflight     — validate config before deploying
";

const DEPLOY: &str = "\
Floo Deploy Flow

## How Deploys Work

  All source comes from GitHub. The CLI never uploads code.

  1. **Detect runtime** — CLI scans project files to determine language/framework
  2. **Create deploy** — CLI sends metadata to the API; any in-progress deploy
     for the same service is automatically cancelled
  3. **Pull source** — API downloads source from your connected GitHub repo
  4. **Build** — builds container image via Cloud Build
  5. **Migrate** — runs migrate_command (if set) after build, before traffic shifts
  6. **Deploy** — deploys container to Cloud Run
  7. **URL** — returns the live URL for your app

## Deploy Flow

  1. Push to GitHub:     git push origin main
  2. Watch the deploy:   floo deploys watch --app <name>
  3. Done when you see:  ✓ Deployed to https://...

  The push triggers a deploy automatically via GitHub webhook.

## Force Redeploy

  Use `floo redeploy` when you need to redeploy without a code change
  (e.g., after updating env vars):

    floo env set API_KEY=new-value --app myapp --service api
    floo redeploy --app myapp

## First Deploy

  Use `floo apps github connect owner/repo`. This connects GitHub and
  triggers the first deploy in one step. The app is auto-created if
  it doesn't exist.

## Do I Need a Dockerfile?

  Yes — every service deploys from a Dockerfile. Floo does not deploy
  without one.

  You usually do not have to write it yourself. `floo init` detects your
  runtime (Node.js, Python, Go, static) and generates a working Dockerfile
  that you commit alongside your code. Agents run `floo init --json` to
  see what was detected and generated.

  Write your own Dockerfile when you need a custom build (multi-stage,
  system packages, non-standard entrypoints). If a Dockerfile already
  exists, `floo init` leaves it alone.

## Runtime Detection (at `floo init`)

  `floo init` inspects the project directory to generate a Dockerfile:

  Dockerfile       — already present, init leaves it untouched
  package.json     — Node.js (detects Express, Next.js, etc.)
  pyproject.toml   — Python (detects Django, Flask, FastAPI)
  requirements.txt — Python (fallback)
  go.mod           — Go
  index.html       — Static site (lowest priority)

  If detection is low-confidence, init prompts you (or in `--json` mode,
  suggests adding a Dockerfile manually). At deploy time, the API requires
  a Dockerfile in the service path — missing-Dockerfile deploys fail fast.

## Preflight Validation

  floo preflight                   — validate config, detect runtimes, check readiness
  floo preflight --json            — structured output for agents

  Preflight FAILS (exit 1, valid=false) on configs that can't build or run:
  a service path that doesn't exist, a [cron.*] with an invalid schedule, a
  cron job whose service doesn't exist (multi-service apps). It WARNS (exit 0,
  but not a clean green) on things it can't fully verify locally: a
  migrate_command with no reachable database, a required env var not injected
  or present in a local env file. Server-side `floo env set` vars and external
  databases are invisible to local preflight, which is why those warn.

  JSON shape:
    data.valid           — false iff any finding has severity \"error\"
    data.findings[]      — every advisory, typed: {severity (error|warning|
                           info), code, message, path?, hint?}. Filter by
                           severity/code instead of screen-scraping prose.
    data.env_injection_plan — per-service managed attachments, generated env
                           keys (DATABASE_URL + PG*, REDIS_URL, STORAGE_*),
                           required/optional keys, explicit vs implicit mode.
    data.cron[]          — declared [cron.*] entries (name, schedule, command,
                           service, timeout).
    contains_secrets     — top-level marker, true when a secret-shaped var is
                           found in a web service's env file (it may ship to
                           the browser). Harnesses can refuse the payload.

## Redeploy Options

  floo redeploy --app <name>       — redeploy with fresh env vars (no rebuild)
  floo redeploy --app <name> --rebuild  — force a full rebuild from latest commit
  floo redeploy [path]             — full redeploy from local project directory
  floo redeploy --service <name>  — redeploy specific services only
  floo redeploy --sync-env         — re-sync env vars from env_file before redeploying
  floo redeploy --rebuild --skip-migrations  — hotfix path: bypass MIGRATE step

## Deploy History

  floo deploys list --app <name>    — list past deploys without build logs
  floo deploys logs <id> --app <n>  — build logs for a specific deploy
  floo deploys watch --app <name>   — stream deploy progress in real-time
  floo deploys rollback <app> <id>  — rollback to a previous deploy
  floo releases rollback --app <name> --to <id>  — same, alias under releases
";

const AUTH: &str = "\
App Auth — Add User Authentication to Your App

Floo manages user authentication for your deployed apps. Set
access_mode = \"accounts\" and floo's gateway puts a hosted sign-in
flow in front of your app, validates each user's session, and injects
identity headers into every request before it reaches your code.

You write no auth code. No login pages. No OAuth flow. Your app reads
X-Floo-User-Email from the request headers — that is the entire
integration.

## Quickstart

  1. Set the access mode in floo.app.toml:

       [app]
       name = \"my-app\"
       access_mode = \"accounts\"

  2. Deploy:

       git push origin main

  3. Read identity headers in your app code (every authenticated
     request has them):

       X-Floo-User-Email: jane@acme.com
       X-Floo-User-Id:    01HQK4...
       X-Floo-User-Name:  Jane Doe
       X-Floo-User-Role:  member

That is the entire setup. There is no [auth] section to configure,
no callback URLs to register, no client ID to provision, no token
exchange to implement.

## What you get

  - Hosted sign-in page (email magic link, Google, GitHub)
    — branded; gateway redirects unauthenticated visitors to it
  - Session cookie (__floo_session) validated on every request,
    rolled forward as users stay active, revoked on sign-out
  - Identity headers (X-Floo-User-Email/Id/Name/Role) injected on
    every authenticated request
  - GET /__floo/me        — signed-in user JSON {user_id,email,name,role};
                            401 if no session, 403 HTML if access-denied
  - POST|DELETE /__floo/logout — clears floo session and 302s to login.
                            Lands at app root / after re-auth. Other methods
                            (including GET) return 405; SameSite=Lax + GET
                            would have allowed cross-origin drive-by sign-out.
                            Does NOT log the user out of WorkOS — federated
                            SLO is not yet supported.
  - Per-app user list in the dashboard (first-seen, last-active,
    sign-in count)

## Restricting who can sign in

By default, anyone with a valid email can sign in. Restrict access
in the dashboard or in floo.app.toml:

  [auth]
  access_policy = \"domain\"          # \"open\", \"invite\", or \"domain\"
  allowed_domains = [\"acme.com\"]    # required when access_policy = \"domain\"

  - open    — anyone with a valid email
  - invite  — invited users only (manage in dashboard's per-app
              Access tab, or assign on first deploy)
  - domain  — restricted to allowed_domains (Pro+; consumer
              mailboxes like gmail.com rejected by default)

## Access Modes

  public    — no auth, anyone can access (default)
  password  — shared password for simple protection (Pro+)
  accounts  — per-user auth, gateway-managed (Pro+)

Enterprise SSO (SAML/OIDC) is a sales-assisted setup, not a self-serve
access_mode value — email sales@getfloo.com if your team needs it.

Per-environment overrides work too:

  [environments.dev]
  access_mode = \"public\"

For password-protected apps, set access_mode = \"password\" in
floo.app.toml. The platform generates the shared password on the
next deploy. Retrieve it with:

  floo apps password my-app

## Reading the user in your code

Stack-specific examples are in the build journey guides:

  floo docs rails    — Ruby on Rails
  floo docs nextjs   — Next.js (App Router)
  floo docs fastapi  — FastAPI
  floo docs django   — Django
  floo docs express  — Express (Node.js)

The pattern is the same in every stack: read X-Floo-User-Email
from request headers in a middleware or controller hook.

## Local development

For accounts-mode apps, `floo dev --fixture-user EMAIL` starts a
small in-process proxy in front of each service that injects the
same X-Floo-User-* headers the gateway adds in production:

  floo dev --app my-app --fixture-user you@example.com

The output shows two URLs per service — the raw service URL and an
auth-proxied URL. Hit the auth-proxied URL when you want to test
signed-in flows; your app sees the four identity headers exactly
as it would behind the gateway. Hit the raw URL for quick checks
or unauthenticated paths.

Optional flags (with defaults):

  --fixture-id ID      default: dev-fixture-<email-localpart>
  --fixture-name NAME  default: the email
  --fixture-role ROLE  default: member

The proxy only runs when access_mode = \"accounts\". For one-off
curl testing or scripts, send the headers yourself:

  curl -H \"X-Floo-User-Email: you@example.com\" \\
       -H \"X-Floo-User-Id: dev-user-1\" \\
       http://localhost:3000/dashboard

Full docs: https://getfloo.com/docs/guides/app-auth
";

const FEEDBACK: &str = "\
Floo Feedback

Report bugs, friction, feature requests, or general feedback directly from
the CLI. Feedback is routed to the Floo team in real-time.

## Usage

  floo feedback \"your message here\"
  floo feedback --category bug \"deploys fail when Dockerfile is missing\"
  floo feedback --category friction \"env var sync requires a manual redeploy\"
  floo feedback --category feature_request \"add monorepo support\"
  floo feedback --app my-app \"this app crashes on cold start\"

## Categories

  general          — general feedback (default)
  bug              — something is broken
  friction         — a rough edge or confusing workflow
  feature_request  — something you wish existed

## Agent Usage

  Agents should use --json mode. When --json is set, the source is recorded
  as \"agent\" instead of \"cli\" so the team can distinguish human vs agent
  feedback.

  floo feedback --json --category friction \"deploy watch hangs after timeout\"

## Context

  Use --context to attach extra detail (error output, steps to reproduce):

  floo feedback --category bug \"deploy fails\" --context \"error: no Dockerfile found\"
";

const NOTIFICATIONS: &str = "\
Email Notifications — control which emails floo sends you

floo emails you about things that happen to your apps. You choose which
categories land in your inbox; account and security messages always send.

## List your settings

  floo notifications list
  floo notifications list --json     (machine-readable, for agents)

Shows every category, whether it is on or off, and what it covers.

## Turn a category on or off

  floo notifications set deploy_success on    Email me on every successful deploy
  floo notifications set deploy_success off   Stop those (this is the default)
  floo notifications set billing off          Stop spend-cap warning emails

## Categories

  Run `floo notifications list` to see the current categories and their state.
  deploy_success is OFF by default (it is the noisy one); the rest are ON.

## Notes

  Preferences are per-user and account-wide — they apply to your inbox, not to a
  single app. Always-send emails (invites, verification, security approvals,
  and destructive-action warnings) are not configurable.
";

const HOWTO: &str = "\
Floo — Golden Path

## Before You Start

  You need:
  - A project directory with source code (Dockerfile optional — floo auto-detects runtimes)
  - The code pushed to a GitHub repository

  App names must be lowercase, alphanumeric, and may include hyphens (e.g., my-saas-app).
  Replace owner/repo with your GitHub username (or org) and repository name.

  The --app flag is optional when you're in a directory with config files
  (floo.service.toml or floo.app.toml). Use it when running commands from
  outside your project directory.

## First-Time Setup (4 commands)

  1. floo auth login                         # sign up or log in (opens browser)
  2. floo init my-app
  3. floo preflight                          # validate config (local only, no auth)
  4. floo apps github connect owner/repo     # creates app + triggers first deploy

  floo auth login opens a browser. New users create an account automatically.
  In headless/CI environments, use: floo auth login --api-key <key>

  Floo installs a GitHub webhook when you connect. After that, every
  git push triggers a build and deploy automatically.

  Check your deploy succeeded:
  floo apps show my-app

## How to Ship Changes

  git add . && git commit -m \"feat: my change\"
  git push origin main
  floo deploys watch --app my-app

## How to Redeploy Without a Code Change

  floo redeploy --app my-app

  Use this after updating env vars or changing config.
  To force a full rebuild: floo redeploy --app my-app --rebuild

## How to Add Env Vars

  floo env set KEY=value --app my-app
  floo redeploy --app my-app                # pick up new vars

## How to Add a Database

  Run:

  floo services add postgres --app my-app

  Commit the generated lock file and push to GitHub:
  git add .floo/services.lock && git commit -m \"feat: add postgres\"
  git push origin main

  The database is available on the next deploy. Credentials arrive as a
  standard DATABASE_URL plus PGHOST/PGPORT/PGDATABASE/PGUSER/PGPASSWORD.

## How to Add a Custom Domain

  1. Add the domain:

     floo domains add app.example.com --app my-app

  2. The output shows a CNAME record to add at your DNS provider:

     CNAME app.example.com -> my-app.on.getfloo.com

  3. Add that CNAME record in your DNS provider (Cloudflare, Route 53, etc).

  4. Verify the domain in the dashboard (click \"Verify DNS\") or wait for
     the auto-poll to pick it up. Once verified, status changes to active
     and you get a confirmation email.

  For multi-service apps, target a specific service:

  floo domains add api.example.com --app my-app --service api

## How to Roll Back

  floo deploys list --app my-app             # find the deploy ID
  floo deploys rollback my-app <deploy-id>

## How to Debug

  floo logs query --app my-app --since 1h --error
  floo logs tail --app my-app --env prod
  floo logs query --app my-app --service web        # one service (multi-service apps)
  floo logs query --app my-app --cron nightly-report # a specific cron job's output
  floo logs query --app my-app --deployment latest --json
  floo logs query --app my-app --json --limit 100 --cursor \"$NEXT_CURSOR\"
  floo deploys logs <deploy-id> --app my-app

## Decision Table: What Command Do I Run?

  I want to...                          | Run this
  --------------------------------------|----------------------------------------
  Create an account or log in           | floo auth login
  Deploy for the first time             | floo apps github connect owner/repo
  Ship a code change                    | git push origin main
  Validate my config                    | floo preflight (local only, no auth)
  Redeploy after env var change         | floo redeploy --app my-app
  Force rebuild without code change      | floo redeploy --app my-app --rebuild
  Watch a deploy in progress            | floo deploys watch --app my-app
  See deploy history                    | floo deploys list --app my-app
  Roll back to a previous version       | floo deploys rollback my-app <id>
  Set an env var                        | floo env set KEY=val --app my-app
  Add a custom domain                   | floo domains add example.com --app my-app (then add CNAME at DNS provider)
  Verify a custom domain                | floo domains verify example.com --app my-app
  View logs                             | floo logs query --app my-app
  Run locally with prod credentials     | floo dev --app my-app (requires dev_command)
";

const TEMPLATES: &str = "\
Floo Templates — Copy-Paste App Structures

## React + FastAPI (Multi-Service)

  A frontend (React/Vite) + backend (FastAPI) app with a shared database.

### Directory Structure

  my-app/
  ├── floo.app.toml          # single config file — services declared inline
  ├── Dockerfile
  ├── web/                   # React frontend
  │   ├── package.json
  │   └── src/
  └── api/                   # FastAPI backend
      ├── pyproject.toml     # or requirements.txt
      └── app/
          └── main.py

### floo.app.toml (single file for the whole app)

  [app]
  name = \"my-app\"

  [postgres]
  tier = \"basic\"

  [services.web]
  path = \"./web\"
  type = \"web\"
  port = 3000
  ingress = \"public\"
  dev_command = \"npm run dev\"

  [services.api]
  path = \"./api\"
  type = \"api\"
  port = 8080
  ingress = \"public\"
  dev_command = \"uvicorn app.main:app --reload --port 8080\"
  migrate_command = \"alembic upgrade head\"

### api/app/main.py

  from fastapi import FastAPI, Request

  app = FastAPI()

  @app.get(\"/users\")
  async def list_users(request: Request):
      # The gateway routes /api/users → /users (strips the /api prefix)
      # Identity headers are injected when access_mode != \"public\":
      user_email = request.headers.get(\"X-Floo-User-Email\")
      return {\"users\": [], \"requested_by\": user_email}

  @app.get(\"/health\")
  async def health():
      return {\"status\": \"ok\"}

### web/src/App.tsx (React calling the API)

  // In production, the gateway routes /api/* to the api service.
  // No CORS needed — same origin.
  const response = await fetch(\"/api/users\");
  const data = await response.json();

  // For local development, proxy /api/* to the FastAPI dev server.
  // In vite.config.ts:
  //   server: { proxy: { \"/api\": \"http://localhost:8080\" } }

### Deploy

  PREREQUISITE: Your code must be in a GitHub repo. Floo pulls source
  from GitHub — it does not upload local files. Push your code first.

  1. floo auth login
  2. floo init my-app                          # from root directory
  3. floo preflight                            # validate both services
  4. git push origin main                      # push to GitHub first
  5. floo apps github connect owner/my-app     # triggers first deploy
  6. floo env set DATABASE_URL=<url> --service api  # managed postgres auto-sets this
  7. floo apps show my-app                     # get your URL

### Local Development (two terminals)

  Terminal 1 (backend):
    cd api && uvicorn app.main:app --reload --port 8080

  Terminal 2 (frontend):
    cd web && npm run dev
    # vite.config.ts proxy forwards /api/* to localhost:8080

  Or use floo dev for cloud-connected local development:
    floo dev --app my-app --service api    # terminal 1
    floo dev --app my-app --service web    # terminal 2

### Env Vars

  Backend secrets (api service only):
    floo env set DATABASE_URL=postgres://... --service api
    floo env set SECRET_KEY=... --service api

  Frontend config (web service only, public — baked into JS bundle):
    floo env set VITE_API_URL=/api --service web

  SECURITY: Never set backend secrets on the web service.
  Build-time vars (VITE_*, NEXT_PUBLIC_*) are visible to end users.

## Next.js + FastAPI (Multi-Service)

  Same structure as above — declare both services inline in one floo.app.toml.
  Replace the [services.web] entry with a Next.js service:

  [services.web]
  path = \"./web\"
  type = \"web\"
  port = 3000
  ingress = \"public\"
  dev_command = \"npm run dev\"

### Key Differences from React

  - Next.js API routes can also call the FastAPI service internally
  - Use NEXT_PUBLIC_* prefix for client-side env vars (same security rules)
  - Server components can read X-Floo-User-* headers directly

## Single-Service App (Simplest)

  For a standalone app (just a web server, no separate API):

  my-app/
  ├── floo.app.toml
  ├── Dockerfile
  ├── package.json
  └── src/

  floo.app.toml:

  [app]
  name = \"my-app\"

  [services.web]
  path = \".\"
  type = \"web\"
  port = 3000
  ingress = \"public\"
  dev_command = \"npm run dev\"

  Deploy:
    floo auth login
    floo init my-app
    floo apps github connect owner/my-app
";

const BUILD: &str = "\
Floo — Build with your stack

Stack-specific journey guides walk a real app from local code to a live
production URL with a database, per-user auth, and a custom domain. Each
guide is end-to-end with runnable code in your stack.

## Available stack guides

  floo docs nextjs   — build and deploy a Next.js (App Router) app on floo
  floo docs rails    — build and deploy a Ruby on Rails app on floo
  floo docs fastapi  — build and deploy a FastAPI app on floo
  floo docs django   — build and deploy a Django app on floo
  floo docs express  — build and deploy an Express (Node.js) app on floo

  Want a stack added (Go, SvelteKit, Phoenix, etc.)?
  `floo feedback --category feature_request \"docs: add <stack> stack guide\"`

## What a stack guide covers

Every stack guide walks the same arc, with stack-specific code:

  1. Add a Dockerfile (or use the framework's default)
  2. Initialize floo config (floo init)
  3. Connect the GitHub repo and ship the first deploy
  4. Add a Postgres sibling service
  5. Add per-user auth (gateway-managed — zero app code)
  6. Add a custom domain
  7. Run locally with prod credentials

## Why stack guides vs reference docs

Capability guides (floo docs auth, floo docs services, floo docs config)
explain how floo features work. Stack guides show how to use them
end-to-end in a real Rails / Next.js / Django / etc. project.

If you know which capability you need, jump to the capability guide. If
you're starting a new project, start with the stack guide.

Full guides: https://getfloo.com/docs/build/
";

const RAILS: &str = "\
Floo — Build a Rails app on floo

End-to-end Rails journey: deploy, add Postgres, add per-user auth, add a
custom domain. Every step has runnable Ruby code.

## 1. Add a Dockerfile

Rails 7.1+ ships a production Dockerfile from `rails new`. Otherwise:

  bin/rails generate dockerfile

Bind to 0.0.0.0 (not localhost). Cloud Run only routes traffic to
processes bound to all interfaces. Expose the same port you set in
floo.app.toml (3000 below).

## 2. Initialize floo config

  floo init my-rails-app

Resulting floo.app.toml:

  [app]
  name = \"my-rails-app\"

  [services.web]
  type = \"web\"
  path = \".\"
  port = 3000
  ingress = \"public\"
  dev_command = \"bin/rails server -p 3000\"
  migrate_command = \"bin/rails db:migrate\"

migrate_command runs after every deploy (against dev) and after every
promote (against prod). Rails migrations stay in sync with deploys.

## 3. Connect repo and deploy

  git add . && git commit -m \"feat: floo config\"
  git push origin main
  floo apps github connect owner/my-rails-app
  floo deploys watch --app my-rails-app

App is live at https://my-rails-app-dev.on.getfloo.com.

## 4. Local dev and one-shot commands

Two commands cover the daily Rails workflow once your first deploy is up.

Local dev server with prod-shaped env:

  floo dev --app my-rails-app --service web

Runs your dev_command locally with DATABASE_URL and other env vars
sourced from floo. Real Cloud SQL connection, no exported credentials.

Add --fixture-user to test signed-in (accounts-mode) flows locally:

  floo dev --app my-rails-app --service web --fixture-user you@example.com

The proxy injects the same X-Floo-User-* headers floo's gateway adds in
production, so the controller reading those headers works locally with
no conditional code.

One-shot commands (rake tasks, db:seed, console):

  floo run --service web -- bundle exec rake my_task
  floo run --service web -- bin/rails db:seed
  floo run --service web -- bin/rails console
  floo run --service web -- bin/rails db:migrate

floo run inherits stdin/stdout/stderr, so interactive commands like
bin/rails console work like running them locally — your shell just sees
the floo-injected env vars instead of your local .env. Migrations run
automatically on every deploy via migrate_command; use `floo run --
bin/rails db:migrate` only for ad-hoc migration work outside the deploy
path.

## 5. Add Postgres

  floo services add postgres --app my-rails-app --tier basic
  git add .floo/services.lock && git commit -m \"feat: add postgres\"
  git push origin main

Rails reads DATABASE_URL automatically. floo injects a normal PostgreSQL
URI, so ActiveRecord can parse it without custom Cloud SQL socket code.
`floo preflight` warns if a local env file still contains the old Cloud
SQL socket-style DATABASE_URL that Ruby's URI parser rejects.
Confirm config/database.yml has:

  production:
    primary:
      url: <%= ENV[\"DATABASE_URL\"] %>

## 6. Add per-user auth

floo manages user authentication. Set access_mode = \"accounts\" in floo.app.toml — that is the entire auth config:

  [app]
  name = \"my-rails-app\"
  access_mode = \"accounts\"

Push, deploy. Gateway sits in front of your app, redirects unauth'd users
to a hosted login, validates session on every request, injects
identity headers. Your Rails controllers read them:

  class ApplicationController < ActionController::Base
    before_action :load_floo_user

    private

    def load_floo_user
      @current_user_email = request.headers[\"X-Floo-User-Email\"]
      @current_user_id    = request.headers[\"X-Floo-User-Id\"]
      @current_user_name  = request.headers[\"X-Floo-User-Name\"]
    end
  end

For local development, run `floo dev --fixture-user` (section 4) — same
identity headers in front of the local server, no conditional code.

## 7. Add a custom domain

  floo domains add app.example.com --app my-rails-app

Add the CNAME shown in the output at your DNS provider.

## Common gotchas

  - /healthz is reserved by Cloud Run — use /health or /livez
  - bind: 0.0.0.0 (Rails defaults vary)
  - asset compilation runs in the Dockerfile (RAILS_SERVE_STATIC_FILES=1)
  - Rails 7+ force_ssl works correctly behind floo's edge (X-Forwarded-Proto)

Full guide with complete Ruby code: https://getfloo.com/docs/build/rails
";

const NEXTJS: &str = "\
Floo — Build a Next.js app on floo

End-to-end Next.js 14+ App Router journey: deploy, add Postgres, add
per-user auth, add a custom domain. Every step has runnable TypeScript
code in the published guide.

## 1. Dockerfile (standalone)

Set output: \"standalone\" in next.config.js, then a multi-stage Dockerfile
ending with `CMD [\"node\", \"server.js\"]`. Set HOSTNAME=0.0.0.0 (Next.js
standalone defaults to localhost — Cloud Run won't reach it).

## 2. NEXT_PUBLIC_* build-arg trap

Any NEXT_PUBLIC_* var is baked into the JS bundle at BUILD TIME. Thread
it through the Dockerfile as ARG + ENV in the build stage AND pass it
on every build:

  floo env set NEXT_PUBLIC_API_URL=https://my-app.on.getfloo.com \\
    --app my-app --build-arg

Skipping this is the most common Next.js footgun on floo.

## 3. floo init + deploy

  floo init my-nextjs-app

  [services.web]
  type = \"web\"
  path = \".\"
  port = 3000
  ingress = \"public\"
  dev_command = \"npm run dev\"
  migrate_command = \"npx prisma migrate deploy\"   # if you use Prisma

  git push origin main
  floo apps github connect owner/my-nextjs-app

## 4. Postgres

  floo services add postgres --app my-nextjs-app --tier basic
  # Prisma reads DATABASE_URL automatically

## 5. Per-user auth

  [app]
  access_mode = \"accounts\"

Then in a Server Component or Route Handler:

  import { headers } from \"next/headers\";
  const h = await headers();
  const email = h.get(\"x-floo-user-email\");

## 6. Custom domain

  floo domains add app.example.com --app my-nextjs-app

## 7. Local dev

  floo dev --app my-nextjs-app --service web

## Gotchas

  - /healthz is reserved by Cloud Run — use /health
  - HOSTNAME=0.0.0.0 (standalone defaults to localhost)
  - NEXT_PUBLIC_* must be threaded via --build-arg
  - output: \"standalone\" in next.config.js
  - Never expose tokens to client components

Full guide with complete TypeScript code: https://getfloo.com/docs/build/nextjs
";

const FASTAPI: &str = "\
Floo — Build a FastAPI app on floo

End-to-end FastAPI journey: deploy, add Postgres, add per-user auth,
add a custom domain. Every step has runnable Python code in the
published guide.

## 1. Dockerfile

  FROM python:3.12-slim
  ...
  CMD [\"uvicorn\", \"app.main:app\", \"--host\", \"0.0.0.0\", \"--port\", \"8000\"]

Bind to 0.0.0.0 — 127.0.0.1 won't accept Cloud Run traffic.

## 2. floo init + deploy

  floo init my-fastapi-app

  [services.web]
  type = \"web\"
  path = \".\"
  port = 8000
  ingress = \"public\"
  dev_command = \"uvicorn app.main:app --reload --port 8000\"
  migrate_command = \"alembic upgrade head\"   # if you use Alembic

  git push origin main
  floo apps github connect owner/my-fastapi-app

## 3. Postgres

  floo services add postgres --app my-fastapi-app --tier basic

  # Async SQLAlchemy — convert to asyncpg URL:
  DATABASE_URL = os.environ[\"DATABASE_URL\"].replace(
      \"postgresql://\", \"postgresql+asyncpg://\", 1)

## 4. Per-user auth — pick a model

  [app]
  access_mode = \"accounts\"

Then a FastAPI dependency:

  def require_user(
      email: Annotated[str | None, Header(alias=\"X-Floo-User-Email\")] = None,
      user_id: Annotated[str | None, Header(alias=\"X-Floo-User-Id\")] = None,
  ):
      if not email: raise HTTPException(401)
      return FlooUser(email=email, user_id=user_id)

  @app.get(\"/dashboard\")
  async def dashboard(user = Depends(require_user)): ...

## 5. Custom domain

  floo domains add app.example.com --app my-fastapi-app

## 6. Local dev

  floo dev --app my-fastapi-app --service web

## Gotchas

  - /healthz is reserved by Cloud Run — use /health
  - Bind to 0.0.0.0
  - Don't mix asyncpg and psycopg2 — pick one
  - X-Forwarded-Proto: build absolute URLs from forwarded scheme

Full guide with complete Python code: https://getfloo.com/docs/build/fastapi
";

const DJANGO: &str = "\
Floo — Build a Django app on floo

End-to-end Django 4+ journey: deploy, add Postgres, add per-user auth,
add a custom domain. Every step has runnable Python code in the
published guide.

## 1. Dockerfile

  FROM python:3.12-slim
  ...
  RUN python manage.py collectstatic --noinput
  CMD [\"gunicorn\", \"mysite.wsgi:application\", \"--bind\", \"0.0.0.0:8000\", \"--workers\", \"3\"]

Use whitenoise for static files, gunicorn for the WSGI server.

## 2. settings.py for production

  import dj_database_url

  SECRET_KEY = os.environ[\"DJANGO_SECRET_KEY\"]
  DEBUG = os.environ.get(\"DJANGO_DEBUG\", \"false\").lower() == \"true\"
  ALLOWED_HOSTS = [\".on.getfloo.com\", *os.environ.get(\"DJANGO_ALLOWED_HOSTS\", \"\").split(\",\")]

  SECURE_PROXY_SSL_HEADER = (\"HTTP_X_FORWARDED_PROTO\", \"https\")
  USE_X_FORWARDED_HOST = True
  SESSION_COOKIE_SECURE = True
  SESSION_COOKIE_HTTPONLY = True
  SESSION_COOKIE_SAMESITE = \"Lax\"

  DATABASES = {\"default\": dj_database_url.config(conn_max_age=600)}

## 3. floo init + deploy

  floo init my-django-app

  [services.web]
  type = \"web\"
  path = \".\"
  port = 8000
  ingress = \"public\"
  dev_command = \"python manage.py runserver 0.0.0.0:8000\"
  migrate_command = \"python manage.py migrate --noinput\"

  git push origin main
  floo apps github connect owner/my-django-app

  # Set the secret key after first deploy
  floo env set DJANGO_SECRET_KEY=\"$(python -c 'from django.core.management.utils import get_random_secret_key; print(get_random_secret_key())')\" --app my-django-app
  floo redeploy --app my-django-app

## 4. Postgres

  floo services add postgres --app my-django-app --tier basic
  # dj-database-url parses DATABASE_URL automatically

## 5. Per-user auth

  [app]
  access_mode = \"accounts\"

Add a tiny middleware that reads X-Floo-User-Email / X-Floo-User-Id /
X-Floo-User-Name from request.META (Django prefixes incoming HTTP
headers with HTTP_ and uppercases them):

  class FlooUserMiddleware:
      def __init__(self, get_response): self.get_response = get_response
      def __call__(self, request):
          email = request.META.get(\"HTTP_X_FLOO_USER_EMAIL\")
          request.floo_user = FlooUser(email=email, ...) if email else None
          return self.get_response(request)

## 6. Custom domain

  floo domains add app.example.com --app my-django-app
  floo env set DJANGO_ALLOWED_HOSTS=app.example.com --app my-django-app
  floo redeploy --app my-django-app

## 7. Local dev

  floo dev --app my-django-app --service web

## Gotchas

  - /healthz is reserved by Cloud Run — use /health
  - Bind to 0.0.0.0 in gunicorn
  - DEBUG=False in prod (the default above is False)
  - DJANGO_SECRET_KEY must be set or sessions can be forged
  - SECURE_PROXY_SSL_HEADER required for is_secure() to work behind floo

Full guide with complete Python code: https://getfloo.com/docs/build/django
";

const EXPRESS: &str = "\
Floo — Build an Express app on floo

End-to-end Express 4/5 journey: deploy, add Postgres, add per-user
auth, add a custom domain. Every step has runnable JavaScript code
in the published guide.

## 1. Dockerfile

  FROM node:20-slim AS deps
  ...
  CMD [\"node\", \"server.js\"]

## 2. Trust the proxy

  app.set(\"trust proxy\", true);
  app.listen(port, \"0.0.0.0\", ...);

Without trust proxy, req.protocol is always 'http' and secure cookies
won't get set behind floo's edge.

## 3. floo init + deploy

  floo init my-express-app

  [services.web]
  type = \"web\"
  path = \".\"
  port = 3000
  ingress = \"public\"
  dev_command = \"node --watch server.js\"

  git push origin main
  floo apps github connect owner/my-express-app

## 4. Postgres

  floo services add postgres --app my-express-app --tier basic

  import pg from \"pg\";
  export const pool = new pg.Pool({
    connectionString: process.env.DATABASE_URL, max: 10
  });

## 5. Per-user auth

  [app]
  access_mode = \"accounts\"

  app.use((req, _res, next) => {
    const email = req.get(\"x-floo-user-email\");
    req.flooUser = email ? { email, id: req.get(\"x-floo-user-id\") } : null;
    next();
  });

## 6. Custom domain

  floo domains add app.example.com --app my-express-app

## 7. Local dev

  floo dev --app my-express-app --service web

## Gotchas

  - /healthz is reserved by Cloud Run — use /health
  - app.listen(port, \"0.0.0.0\", ...) explicitly
  - app.set(\"trust proxy\", true) is required
  - SESSION_SECRET required for cookie-session
  - For server-side sessions: floo services add redis + connect-redis

Full guide with complete JavaScript code: https://getfloo.com/docs/build/express
";

const CRON: &str = "\
Floo Cron Jobs

Cron jobs are declared in floo.app.toml — never created by the CLI. Each
[cron.<name>] section becomes a managed cron job, reconciled on every
deploy (added, updated, or removed to match config).

## Declare in floo.app.toml

  [cron.daily-report]
  schedule = \"0 9 * * *\"                  # 9am UTC daily
  command  = \"python -m reports.daily\"
  service  = \"api\"                         # which service's image to run in

  [cron.cleanup]
  schedule = \"*/5 * * * *\"                 # every 5 minutes
  command  = \"node scripts/cleanup.js\"
  service  = \"worker\"
  timeout  = 600                            # max seconds (default 300, optional)

## Fields

  schedule  required  Standard cron expression in UTC.
  command   required  Shell command executed inside the target service's container.
  service   required  Name of a [services.<name>] entry; that image is reused.
  timeout   optional  Max execution time in seconds. Default 300.

## Common schedules

  * * * * *     every minute
  */5 * * * *   every 5 minutes
  0 * * * *     every hour
  0 9 * * *     daily at 9am UTC
  0 9 * * 1-5   weekdays at 9am UTC
  0 0 * * 0     weekly on Sunday at midnight UTC
  0 0 1 * *     monthly on the 1st at midnight UTC

## Deploy and verify

  Push to GitHub or run `floo redeploy`. New jobs are created, changed jobs
  updated, removed jobs deleted — all on the deploy itself.

    git push origin main && floo deploys watch --app my-app
    floo cron list --app my-app                # see jobs + last run status

## Manually trigger a job (off-schedule)

  Useful for testing or one-off catch-up runs:

    floo cron run daily-report --app my-app
    floo cron run daily-report --app my-app --dry-run   # preview, no API call

## CLI surface

  The `floo cron` CLI is read-only + manual trigger. Schedules and commands
  are config-driven; the CLI never adds, removes, or edits them.

    floo cron list --app <name>            list jobs and last run status
    floo cron show <name> --app <app>      details for one job
    floo cron run <name> --app <app>       trigger a job off-schedule

## Environment

  Jobs run inside the specified service's container image with the same
  env vars as the service — same DATABASE_URL, REDIS_URL, secrets, etc.

## Long-form guide

  https://getfloo.com/docs/guides/cron-jobs   — examples, agent workflow, troubleshooting
  https://getfloo.com/docs/reference/config-spec   — full [cron.<name>] schema reference
";

// Canonical docs topics, in the order the overview lists them. This table is
// the single source of truth for which `floo docs` topics exist: the overview
// listing, the `floo docs --help` block, and every `floo docs <topic>`
// cross-reference are all pinned back to it by tests in this module (and the
// `--help` block by a test in `cli.rs`). Add a topic here and those tests fail
// until the overview and `--help` list it too.
pub(crate) const TOPICS: &[(&str, &str)] = &[
    ("golden-path", HOWTO),
    ("quickstart", QUICKSTART),
    ("build", BUILD),
    ("nextjs", NEXTJS),
    ("rails", RAILS),
    ("fastapi", FASTAPI),
    ("django", DJANGO),
    ("express", EXPRESS),
    ("templates", TEMPLATES),
    ("services", SERVICES),
    ("edge", EDGE),
    ("previews", PREVIEWS),
    ("config", CONFIG),
    ("cron", CRON),
    ("deploy", DEPLOY),
    ("auth", AUTH),
    ("notifications", NOTIFICATIONS),
    ("feedback", FEEDBACK),
];

// Convenience aliases. An agent types the concrete noun it already has — a
// managed-service name (`storage`) or a config filename (`app-toml`) — and
// lands on the canonical topic instead of hitting "Unknown docs topic". Each
// alias resolves to a TOPICS name and is surfaced in the overview so it stays
// discoverable; both invariants are pinned by tests in this module.
pub(crate) const ALIASES: &[(&str, &str)] = &[("storage", "services"), ("app-toml", "config")];

pub fn docs(topic: Option<&str>) {
    let (topic_name, content) = match topic {
        None => ("overview", OVERVIEW),
        Some(t) => match resolve_topic(t) {
            Some(resolved) => resolved,
            None => {
                let available: Vec<&str> = TOPICS.iter().map(|(n, _)| *n).collect();
                output::error(
                    &format!("Unknown docs topic: '{t}'."),
                    &crate::errors::ErrorCode::InvalidFormat,
                    Some(&format!("Available topics: {}", available.join(", "))),
                );
                std::process::exit(1);
            }
        },
    };

    if output::is_json_mode() {
        output::success(
            &format!("docs:{topic_name}"),
            Some(serde_json::json!({
                "topic": topic_name,
                "content": content.trim(),
            })),
        );
    } else {
        eprintln!("{}", content.trim());
    }
}

/// Resolve a requested topic name to its `(canonical_name, content)`. Accepts
/// both canonical topics and convenience aliases; an alias resolves to the
/// canonical topic's name and content, so `floo docs storage` and
/// `floo docs services` are indistinguishable downstream.
fn resolve_topic(t: &str) -> Option<(&'static str, &'static str)> {
    if let Some(entry) = TOPICS.iter().find(|(name, _)| *name == t) {
        return Some(*entry);
    }
    let target = ALIASES.iter().find(|(alias, _)| *alias == t)?.1;
    TOPICS.iter().find(|(name, _)| *name == target).copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_docs_content_not_empty() {
        assert!(!OVERVIEW.is_empty());
        assert!(!QUICKSTART.is_empty());
        assert!(!SERVICES.is_empty());
        assert!(!PREVIEWS.is_empty());
        assert!(!CONFIG.is_empty());
        assert!(!CRON.is_empty());
        assert!(!DEPLOY.is_empty());
        assert!(!AUTH.is_empty());
        assert!(!TEMPLATES.is_empty());
        assert!(!BUILD.is_empty());
        assert!(!NEXTJS.is_empty());
        assert!(!RAILS.is_empty());
        assert!(!FASTAPI.is_empty());
        assert!(!DJANGO.is_empty());
        assert!(!EXPRESS.is_empty());
    }

    /// The 2026-04-30 cron-docs feedback: `floo docs app-toml` mentioned cron
    /// as a feature but never showed the [cron.<name>] schema. CONFIG must
    /// document the schema (fields + example) so agents can author cron jobs
    /// without leaving the cheatsheet, and a dedicated `cron` topic gives the
    /// short answer for `floo docs cron`.
    #[test]
    fn test_cron_schema_documented_in_config_and_cron_topics() {
        for (label, content) in [("config/app-toml", CONFIG), ("cron", CRON)] {
            assert!(
                content.contains("[cron."),
                "{label} must show the [cron.<name>] section header",
            );
            for field in ["schedule", "command", "service", "timeout"] {
                assert!(
                    content.contains(field),
                    "{label} must document the '{field}' field",
                );
            }
            assert!(
                content.contains("floo cron list"),
                "{label} must show the read-only CLI surface",
            );
        }
        // The CONFIG cheatsheet must link to the long-form guide so agents
        // can route to it from `floo docs app-toml`.
        assert!(CONFIG.contains("getfloo.com/docs/guides/cron-jobs"));
    }

    #[test]
    fn test_cron_topic_in_overview_listing() {
        // The overview cheatsheet is the entry point; missing the cron topic
        // here was part of the original discoverability gap.
        assert!(OVERVIEW.contains("floo docs cron"));
    }

    #[test]
    fn test_overview_has_key_concepts() {
        assert!(OVERVIEW.contains("Apps"));
        assert!(OVERVIEW.contains("Services"));
        assert!(OVERVIEW.contains("github connect"));
    }

    #[test]
    fn test_auth_docs_has_key_concepts() {
        // Gateway-managed accounts mode is the only documented public auth product.
        assert!(AUTH.contains("access_mode = \"accounts\""));
        assert!(AUTH.contains("X-Floo-User-Email"));
        assert!(AUTH.contains("/__floo/me"));
        assert!(AUTH.contains("/__floo/logout"));
        // OAuth-toolkit terminology must NOT appear in the public CLI docs.
        assert!(!AUTH.contains("redirect_uris"));
        assert!(!AUTH.contains("/v1/auth/apps"));
        assert!(!AUTH.contains("FLOO_APP_ID"));
        assert!(!AUTH.contains("access_token"));
    }

    #[test]
    fn test_quickstart_has_full_flow() {
        assert!(QUICKSTART.contains("auth login"));
        assert!(QUICKSTART.contains("init"));
        assert!(QUICKSTART.contains("github connect"));
        assert!(QUICKSTART.contains("apps show"));
    }

    #[test]
    fn test_services_no_coming_soon() {
        assert!(!SERVICES.contains("coming soon"));
    }

    #[test]
    fn test_deploy_mentions_github() {
        assert!(DEPLOY.contains("GitHub"));
        assert!(!DEPLOY.contains("archive"));
    }

    #[test]
    fn test_templates_has_react_fastapi() {
        assert!(TEMPLATES.contains("React + FastAPI"));
        assert!(TEMPLATES.contains("floo.app.toml"));
        assert!(TEMPLATES.contains("[services.web]"));
        assert!(TEMPLATES.contains("[services.api]"));
        assert!(TEMPLATES.contains("/api/users"));
        assert!(TEMPLATES.contains("VITE_API_URL"));
    }

    #[test]
    fn test_deploy_explains_dockerfiles() {
        assert!(DEPLOY.contains("Do I Need a Dockerfile?"));
        assert!(DEPLOY.contains("every service deploys from a Dockerfile"));
        assert!(DEPLOY.contains("floo init"));
    }

    #[test]
    fn test_services_explains_routing() {
        assert!(SERVICES.contains("gateway strips the /api"));
        assert!(SERVICES.contains("fetch(\"/api/users\")"));
    }

    #[test]
    fn test_previews_topic_documents_agent_sandbox_contract() {
        assert!(OVERVIEW.contains("floo docs previews"));
        assert!(PREVIEWS.contains("floo previews up"));
        assert!(PREVIEWS.contains("remote GitHub source only"));
        assert!(PREVIEWS.contains("dev_prod_untouched: true"));
        assert!(PREVIEWS.contains("managed_resource_branches"));
        assert!(PREVIEWS.contains("floo previews resources list"));
        assert!(PREVIEWS.contains("PREVIEW_MANAGED_SERVICE_ISOLATION_UNAVAILABLE"));
        assert!(PREVIEWS.contains("floo previews delete"));
        assert!(PREVIEWS.contains("getfloo.com/docs/cli/previews"));
    }

    #[test]
    fn test_build_topic_lists_stack_guides() {
        assert!(BUILD.contains("floo docs nextjs"));
        assert!(BUILD.contains("floo docs rails"));
        assert!(BUILD.contains("floo docs fastapi"));
        assert!(BUILD.contains("floo docs django"));
        assert!(BUILD.contains("floo docs express"));
        assert!(BUILD.contains("getfloo.com/docs/build"));
    }

    #[test]
    fn test_rails_topic_covers_full_journey() {
        // Stack-journey shape: deploy → local dev → DB → auth → domain
        assert!(RAILS.contains("floo init"));
        assert!(RAILS.contains("services add postgres"));
        assert!(RAILS.contains("access_mode = \"accounts\""));
        assert!(RAILS.contains("domains add"));
        assert!(RAILS.contains("floo dev"));
        // Rails workflow leans on rake/console/db:seed — floo run is the
        // way to do those with managed env. Mirror this surface to match
        // the published rails.mdx so agents reading via `floo docs rails`
        // see the same thing as agents reading via the docs site.
        assert!(RAILS.contains("floo run"));
        assert!(RAILS.contains("bin/rails console"));
    }

    #[test]
    fn test_overview_lists_build_journeys() {
        assert!(OVERVIEW.contains("floo docs build"));
        assert!(OVERVIEW.contains("floo docs nextjs"));
        assert!(OVERVIEW.contains("floo docs rails"));
        assert!(OVERVIEW.contains("floo docs fastapi"));
        assert!(OVERVIEW.contains("floo docs django"));
        assert!(OVERVIEW.contains("floo docs express"));
    }

    #[test]
    fn test_all_stacks_cover_full_journey() {
        for (stack, content) in [
            ("nextjs", NEXTJS),
            ("rails", RAILS),
            ("fastapi", FASTAPI),
            ("django", DJANGO),
            ("express", EXPRESS),
        ] {
            assert!(
                content.contains("floo init"),
                "{stack}: missing 'floo init'"
            );
            assert!(
                content.contains("services add postgres"),
                "{stack}: missing 'services add postgres'"
            );
            assert!(
                content.contains("access_mode = \"accounts\""),
                "{stack}: missing access_mode"
            );
            assert!(
                content.contains("domains add"),
                "{stack}: missing 'domains add'"
            );
            assert!(content.contains("floo dev"), "{stack}: missing 'floo dev'");
            assert!(
                content.contains("getfloo.com/docs/build"),
                "{stack}: missing link to full guide"
            );
        }
    }

    #[test]
    fn test_all_stacks_use_gateway_managed_auth_only() {
        // Every stack guide shows gateway-managed accounts mode and NOTHING else
        // — the OAuth toolkit is not a documented public product.
        for (stack, content) in [
            ("nextjs", NEXTJS),
            ("rails", RAILS),
            ("fastapi", FASTAPI),
            ("django", DJANGO),
            ("express", EXPRESS),
        ] {
            // Gateway-managed: app reads injected identity header.
            assert!(
                content.to_lowercase().contains("x-floo-user-email")
                    || content.contains("X-Floo-User-Email"),
                "{stack}: missing X-Floo-User-Email"
            );
            // OAuth-toolkit terminology must NOT appear.
            assert!(
                !content.contains("redirect_uris"),
                "{stack}: leaked redirect_uris"
            );
            assert!(
                !content.contains("/v1/auth/apps"),
                "{stack}: leaked OAuth endpoint"
            );
            assert!(
                !content.contains("FLOO_APP_ID"),
                "{stack}: leaked FLOO_APP_ID"
            );
            assert!(
                !content.contains("hosted app OAuth"),
                "{stack}: leaked 'hosted app OAuth'"
            );
            assert!(
                !content.contains("Hosted app OAuth"),
                "{stack}: leaked 'Hosted app OAuth'"
            );
        }
    }

    /// Every `floo docs <topic>` mention across the overview and every topic
    /// body must resolve to a real topic or alias. The dead
    /// `floo docs state-model` cross-reference (#1159) is exactly what this
    /// pins — the whole class, not just that one instance.
    #[test]
    fn test_every_floo_docs_cross_reference_resolves() {
        let valid: std::collections::HashSet<&str> = TOPICS
            .iter()
            .map(|(n, _)| *n)
            .chain(ALIASES.iter().map(|(a, _)| *a))
            .collect();
        let re = regex::Regex::new(r"floo docs ([a-z][a-z-]*)").unwrap();
        let mut bodies: Vec<&str> = TOPICS.iter().map(|(_, c)| *c).collect();
        bodies.push(OVERVIEW);
        for body in bodies {
            for cap in re.captures_iter(body) {
                let referenced = cap.get(1).unwrap().as_str();
                assert!(
                    valid.contains(referenced),
                    "cross-reference `floo docs {referenced}` has no matching topic or alias",
                );
            }
        }
    }

    /// Every canonical topic must be listed in the overview so an agent reading
    /// `floo docs` can discover all of them. `notifications` was invisible here
    /// before #1159.
    #[test]
    fn test_overview_lists_every_canonical_topic() {
        for (name, _) in TOPICS {
            assert!(
                OVERVIEW.contains(&format!("floo docs {name}")),
                "overview is missing `floo docs {name}`",
            );
        }
    }

    /// Every alias must resolve to a real canonical topic, must not shadow a
    /// canonical name, and must be surfaced in the overview so it stays
    /// discoverable rather than being a hidden duplicate (#1159).
    #[test]
    fn test_every_alias_resolves_to_canonical_topic() {
        for (alias, target) in ALIASES {
            assert!(
                TOPICS.iter().any(|(n, _)| n == target),
                "alias `{alias}` points at unknown canonical topic `{target}`",
            );
            assert!(
                !TOPICS.iter().any(|(n, _)| n == alias),
                "alias `{alias}` collides with a canonical topic name",
            );
            assert!(
                OVERVIEW.contains(alias),
                "alias `{alias}` is not surfaced in the overview",
            );
        }
    }

    /// Dispatching an alias returns the canonical topic's name and content; an
    /// unknown topic returns None so the caller shows the available-topics hint.
    #[test]
    fn test_alias_dispatch_resolves_to_canonical() {
        assert_eq!(resolve_topic("storage"), Some(("services", SERVICES)));
        assert_eq!(resolve_topic("app-toml"), Some(("config", CONFIG)));
        assert_eq!(resolve_topic("services"), Some(("services", SERVICES)));
        assert!(resolve_topic("state-model").is_none());
        assert!(resolve_topic("definitely-not-a-topic").is_none());
    }
}
