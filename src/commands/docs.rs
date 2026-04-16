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
  4. `floo apps status <name>` — see your app's URL and status

  After the first deploy, push to GitHub to deploy: `git push origin main`.
  Watch progress with `floo deploys watch --app <name>`.
  Use `floo redeploy` only to force a redeploy (e.g., after updating env vars).
  Use `floo preflight` to validate config before pushing.

## Learn More

  floo docs golden-path — golden path and decision table
  floo docs quickstart — end-to-end walkthrough
  floo docs templates  — copy-paste app structures (React+FastAPI, Next.js, etc.)
  floo docs services   — service types and managed services
  floo docs config     — config file formats with examples
  floo docs deploy     — detailed deploy flow and runtime detection
  floo docs auth       — add user authentication to your app
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

  This creates config files (floo.service.toml or floo.app.toml) locally.
  No app is registered on the platform yet.

## 3. (Optional) Add Managed Services

  Edit floo.app.toml to add a database, cache, or storage:

  [postgres]
  tier = \"basic\"

  [redis]

  Managed services are auto-provisioned on the first deploy.
  Their credentials arrive as env vars (DATABASE_URL, REDIS_URL, STORAGE_BUCKET + STORAGE_URL).

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

  floo apps status my-app
  floo logs --app my-app

## 7. Subsequent Deploys

  Push to GitHub — the webhook triggers a deploy automatically:

  git push origin main
  floo deploys watch --app my-app

  Use `floo redeploy --app my-app` only when you need to redeploy without
  a code change (e.g., after updating env vars).

## 8. Local Development

  floo dev --app my-app --service web

  Runs your service locally with live Cloud SQL access and the same env vars
  as the deployed version. Requires dev_command set in floo.service.toml.

## What Creates What

  floo init           — local config files only (no API call)
  floo redeploy       — force a redeploy (auto-creates app if needed)
  floo apps github connect — creates app if needed, connects GitHub, triggers first deploy
  Managed services (postgres, redis, storage, cron) are declared in floo.app.toml
  and provisioned automatically on deploy. Edit the config file directly.
";

const SERVICES: &str = "\
Floo Services

An app contains one or more services. Each service is independently deployable.

## App Services (your code)

  web     — HTTP server facing the internet (default for apps with a frontend)
  api     — HTTP server for backend APIs
  worker  — background process (no incoming HTTP traffic)

  Declare services inline in floo.app.toml with type, port, and path.
  See: floo docs config

## Platform Services (provisioned by Floo)

  Declared in floo.app.toml (NOT floo.service.toml), auto-provisioned
  on first deploy. If floo init created floo.service.toml, rename it to
  floo.app.toml and move [service] to [services.web] (add path = \".\").

  postgres — managed PostgreSQL database
             Connection string injected as DATABASE_URL env var.

  redis    — managed Redis instance (Upstash, TLS-enabled)
             Connection string injected as REDIS_URL env var.

  storage  — managed object storage (GCS bucket)
             Bucket name injected as STORAGE_BUCKET + STORAGE_URL env vars.
             Use STORAGE_URL for signed URL requests (upload/download).

  cron     — scheduled tasks that run inside a service's container
             Declare as [cron.<name>] sections with schedule, command, service.

  Example floo.app.toml:

  [postgres]
  tier = \"basic\"

  [redis]

  [storage]

  [cron.daily-report]
  schedule = \"0 9 * * *\"
  command = \"python scripts/report.py\"
  service = \"web\"
  timeout = 600

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

  Redis and storage tiers default to basic (no difference today).

  Inspect with: floo services info <name> --app <app>

## Routing

Multi-service apps share a single hostname with path-based routing:

  web service  → app-name.on.getfloo.com/
  api service  → app-name.on.getfloo.com/api/

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

## Commands

  floo services list --app <name>            — list all services
  floo services info <service> --app <name>  — service details (connection info for managed)

  All services are declared in config files and provisioned on deploy.
  Edit floo.app.toml directly to add or remove services.
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

## Resource Limits (optional, in floo.service.toml)

  [resources]
  cpu = \"1\"             # CPU cores (0.25 to 8)
  memory = \"512Mi\"      # Memory (128Mi to 32Gi)
  max_instances = 10    # Max autoscale instances

## Auth Section (in floo.app.toml)

  [auth]
  redirect_uris = [\"http://localhost:3000/callback\"]

  Required when access_mode = \"accounts\". Registers the OAuth callback
  URLs that your app will use. See: floo docs auth

  Note: {app_url}/auth/callback is auto-registered on every deploy and
  promote for accounts-mode apps. You only need to list additional URIs
  here (e.g. localhost for local development).

## Environment Overrides (in floo.app.toml)

  [environments.dev]
  access_mode = \"public\"

  [environments.prod]
  access_mode = \"accounts\"

## Environment Variables in Multi-Service Apps

  Env vars are scoped per service. In multi-service apps, you MUST specify
  which service receives the variable:

    floo env set DATABASE_URL=postgres://... --services api
    floo env set REDIS_URL=redis://... --services api,worker

  Scoping rules:

  - Single-service app: env vars go to the only service (no flag needed)
  - Multi-service app with 1 service: auto-targets that service
  - Multi-service app with 2+ services: --services is REQUIRED

  SECURITY: Secrets set on a frontend service (web, dashboard) end up in
  the container runtime. Build-time vars (VITE_*, NEXT_PUBLIC_*, REACT_APP_*)
  are baked into the JS bundle and visible to end users. Never set backend
  secrets (DATABASE_URL, API keys) on frontend services.

  Recommended pattern for multi-service apps:

    # Backend secrets — api/worker only
    floo env set DATABASE_URL=postgres://... --services api
    floo env set LINEAR_API_KEY=lin_... --services api
    floo env set REDIS_URL=redis://... --services api,worker

    # Frontend config — web only (public, not secret)
    floo env set VITE_API_URL=https://my-app.getfloo.com/api --services web

  List env vars per service:

    floo env list --services api
    floo env list --services web

  Managed service env vars (DATABASE_URL, REDIS_URL, STORAGE_BUCKET) are
  set at app scope and available to all services. Use --services to restrict
  access if needed.

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

    floo env set API_KEY=new-value --app myapp --services api
    floo redeploy --app myapp

## First Deploy

  Use `floo apps github connect owner/repo`. This connects GitHub and
  triggers the first deploy in one step. The app is auto-created if
  it doesn't exist.

## Do I Need a Dockerfile?

  Usually no. Floo auto-detects your runtime and builds your app:

  - package.json → Node.js (npm install + npm run build + npm start)
  - pyproject.toml or requirements.txt → Python (pip install + uvicorn/gunicorn)
  - go.mod → Go (go build)
  - index.html → Static site (served with nginx)

  Write a Dockerfile ONLY when you need a custom build (e.g., multi-stage
  builds, system packages, non-standard entrypoints). If a Dockerfile exists,
  floo uses it instead of auto-detection.

## Runtime Detection Priority

  Dockerfile       — highest priority (custom build)
  package.json     — Node.js (detects Express, Next.js, etc.)
  pyproject.toml   — Python (detects Django, Flask, FastAPI)
  requirements.txt — Python (fallback)
  go.mod           — Go
  index.html       — Static site (lowest priority)

## Preflight Validation

  floo preflight                   — validate config, detect runtimes, check readiness
  floo preflight --json            — structured output for agents

## Redeploy Options

  floo redeploy --app <name>       — redeploy with fresh env vars (no rebuild)
  floo redeploy --app <name> --rebuild  — force a full rebuild from latest commit
  floo redeploy [path]             — full redeploy from local project directory
  floo redeploy --services <name>  — redeploy specific services only
  floo redeploy --sync-env         — re-sync env vars from env_file before redeploying

## Deploy History

  floo deploys list --app <name>    — list past deploys
  floo deploys logs <id> --app <n>  — build logs for a specific deploy
  floo deploys watch --app <name>   — stream deploy progress in real-time
  floo deploys rollback <app> <id>  — rollback to a previous deploy
";

const AUTH: &str = "\
App Auth — Add User Authentication to Your App

Floo can manage user authentication for your deployed apps. When you set
access_mode = \"accounts\", floo provides a hosted OAuth flow powered by
WorkOS so your users can sign in with email, Google, GitHub, and more.

No separate WorkOS account is needed — floo manages this for you.
The auth endpoints are live as soon as the deploy completes.

## Quickstart (exact sequence)

  IMPORTANT: Auth config lives in floo.app.toml, NOT floo.service.toml.
  If `floo init` created a floo.service.toml for your single-service app,
  rename it to floo.app.toml and move [service] to [services.web] (add path = \".\").

  1. Configure floo.app.toml (see config below)
  2. Deploy so auth endpoints are provisioned:
     git push origin main
  3. Get your app ID:
     floo apps list --json | jq '.data.apps[] | select(.name == \"my-app\") | .id'
  4. Set FLOO_APP_ID so your app can reference it:
     floo env set FLOO_APP_ID=<app-id> --app my-app
     floo redeploy --app my-app

  Each step depends on the previous one. Do not skip ahead.
  Use `floo redeploy` (not `floo deploy`) to redeploy after config changes.

## Domain Naming Convention

  dev:         <app-name>-dev.on.getfloo.com
  production:  <app-name>.on.getfloo.com

  Use these exact hostnames when registering redirect URIs.

## Config

     [app]
     name = \"my-app\"
     access_mode = \"accounts\"

     [auth]
     redirect_uris = [
       \"http://localhost:3000/callback\",
       \"https://my-app-dev.on.getfloo.com/callback\",
       \"https://my-app.on.getfloo.com/callback\"
     ]

## OAuth Flow

Your app integrates with these endpoints (BASE = https://api.getfloo.com):

  1. **Start login** — redirect users to:
     GET BASE/v1/auth/apps/{app_id}/authorize?redirect_uri=<your_callback_url>

     The redirect_uri must EXACTLY match a registered URI (protocol, host, path).

  2. **Receive callback** — floo redirects back to your redirect_uri with a code:
     https://my-app-dev.on.getfloo.com/callback?code=<exchange_code>

  3. **Exchange code for tokens** — POST from your backend:
     POST BASE/v1/auth/apps/{app_id}/token
     Body: { \"grant_type\": \"authorization_code\", \"code\": \"<exchange_code>\" }
     Returns: { \"access_token\": \"<jwt>\", \"refresh_token\": \"<token>\", \"user\": {...} }

  4. **Verify user** — the access_token is an RS256 JWT. Verify locally with:
     GET BASE/v1/auth/apps/{app_id}/.well-known/jwks.json

  5. **Refresh tokens** — when the access_token expires:
     POST BASE/v1/auth/apps/{app_id}/token
     Body: { \"grant_type\": \"refresh\", \"refresh_token\": \"<token>\" }

  6. **Logout** — revoke the refresh token:
     POST BASE/v1/auth/apps/{app_id}/session/logout
     Body: { \"refresh_token\": \"<token>\" }

## Constructing Redirect URIs

  Your app receives the public hostname via X-Forwarded-Host header.
  Use it to build the redirect_uri dynamically:

  Node.js:
    const host = req.headers['x-forwarded-host'] || req.headers.host;
    const redirectUri = `https://${host}/callback`;

  This works in all environments (local, dev, prod) without hardcoding.

## Troubleshooting

  INVALID_REDIRECT_URI:
    - dev deploys use <app>-dev.on.getfloo.com, not <app>.on.getfloo.com
    - deployed apps must use https://, not http://
    - /callback is not the same as /callback/ (trailing slash matters)
    - floo auto-registers {app_url}/auth/callback on every deploy and promote
      so you should not need to add these manually; if you see this error,
      trigger a redeploy to re-sync

  NO_REDIRECT_URIS:
    - floo auto-registers /auth/callback on deploy, so this error means the
      app has never been deployed to the environment you are testing against;
      deploy to dev or promote to prod first

## JWT Claims

  sub    — app user ID (UUID)
  email  — user's email address
  name   — user's display name
  iss    — https://auth.getfloo.com
  aud    — your app ID
  iat    — issued at timestamp
  exp    — expiration timestamp

## Access Modes

  public    — no auth, anyone can access (default)
  password  — shared password for simple protection (Pro+)
  accounts  — per-user auth via floo's hosted OAuth (Pro+)
  sso       — enterprise SSO via SAML/OIDC (Enterprise, coming soon)

## Convenience Endpoint

  GET BASE/v1/auth/apps/{app_id}/session/me
  Header: Authorization: Bearer <access_token>
  Returns the authenticated user's info without decoding the JWT yourself.

Full docs: https://docs.getfloo.com/guides/app-auth
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
  floo apps status my-app

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

  Add to floo.app.toml:

  [postgres]
  tier = \"basic\"

  Commit the change and push to GitHub:
  git add floo.app.toml && git commit -m \"feat: add postgres\"
  git push origin main

  The database is auto-provisioned on the next deploy. Credentials arrive as DATABASE_URL.

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

  floo domains add api.example.com --app my-app --services api

## How to Roll Back

  floo deploys list --app my-app             # find the deploy ID
  floo deploys rollback my-app <deploy-id>

## How to Debug

  floo logs --app my-app --since 1h --error
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
  View logs                             | floo logs --app my-app
  Run locally with prod credentials     | floo dev --app my-app (requires dev_command)
";

const TEMPLATES: &str = "\
Floo Templates — Copy-Paste App Structures

## React + FastAPI (Multi-Service)

  A frontend (React/Vite) + backend (FastAPI) app with a shared database.

### Directory Structure

  my-app/
  ├── floo.app.toml          # app-level config
  ├── web/                   # React frontend
  │   ├── floo.service.toml
  │   ├── package.json
  │   └── src/
  └── api/                   # FastAPI backend
      ├── floo.service.toml
      ├── pyproject.toml     # or requirements.txt
      └── app/
          └── main.py

### floo.app.toml (root)

  [app]
  name = \"my-app\"

  [services.web]
  path = \"./web\"

  [services.api]
  path = \"./api\"

  [postgres]
  tier = \"basic\"

### web/floo.service.toml

  [service]
  name = \"web\"
  type = \"web\"
  port = 3000
  ingress = \"public\"
  dev_command = \"npm run dev\"

### api/floo.service.toml

  [service]
  name = \"api\"
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
  6. floo env set DATABASE_URL=<url> --services api  # managed postgres auto-sets this
  7. floo apps status my-app                   # get your URL

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
    floo env set DATABASE_URL=postgres://... --services api
    floo env set SECRET_KEY=... --services api

  Frontend config (web service only, public — baked into JS bundle):
    floo env set VITE_API_URL=/api --services web

  SECURITY: Never set backend secrets on the web service.
  Build-time vars (VITE_*, NEXT_PUBLIC_*) are visible to end users.

## Next.js + FastAPI (Multi-Service)

  Same structure as above, but replace web/ with a Next.js app:

### web/floo.service.toml

  [service]
  name = \"web\"
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
  ├── floo.service.toml
  ├── package.json
  └── src/

  floo.service.toml:

  [app]
  name = \"my-app\"

  [service]
  name = \"web\"
  port = 3000
  type = \"web\"
  ingress = \"public\"
  dev_command = \"npm run dev\"

  Deploy:
    floo auth login
    floo init my-app
    floo apps github connect owner/my-app
";

const TOPICS: &[(&str, &str)] = &[
    ("quickstart", QUICKSTART),
    ("golden-path", HOWTO),
    ("services", SERVICES),
    ("config", CONFIG),
    ("app-toml", CONFIG), // alias — agents can run `floo docs app-toml` after `floo init`
    ("deploy", DEPLOY),
    ("auth", AUTH),
    ("feedback", FEEDBACK),
    ("templates", TEMPLATES),
];

pub fn docs(topic: Option<&str>) {
    let (topic_name, content) = match topic {
        None => ("overview", OVERVIEW),
        Some(t) => match TOPICS.iter().find(|(name, _)| *name == t) {
            Some((name, content)) => (*name, *content),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_docs_content_not_empty() {
        assert!(!OVERVIEW.is_empty());
        assert!(!QUICKSTART.is_empty());
        assert!(!SERVICES.is_empty());
        assert!(!CONFIG.is_empty());
        assert!(!DEPLOY.is_empty());
        assert!(!AUTH.is_empty());
        assert!(!TEMPLATES.is_empty());
    }

    #[test]
    fn test_overview_has_key_concepts() {
        assert!(OVERVIEW.contains("Apps"));
        assert!(OVERVIEW.contains("Services"));
        assert!(OVERVIEW.contains("github connect"));
    }

    #[test]
    fn test_auth_docs_has_key_concepts() {
        assert!(AUTH.contains("redirect_uris"));
        assert!(AUTH.contains("authorize"));
        assert!(AUTH.contains("access_token"));
        assert!(AUTH.contains("accounts"));
        assert!(AUTH.contains("app_id"));
    }

    #[test]
    fn test_quickstart_has_full_flow() {
        assert!(QUICKSTART.contains("auth login"));
        assert!(QUICKSTART.contains("init"));
        assert!(QUICKSTART.contains("github connect"));
        assert!(QUICKSTART.contains("apps status"));
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
        assert!(TEMPLATES.contains("floo.service.toml"));
        assert!(TEMPLATES.contains("/api/users"));
        assert!(TEMPLATES.contains("VITE_API_URL"));
    }

    #[test]
    fn test_deploy_explains_dockerfiles() {
        assert!(DEPLOY.contains("Do I Need a Dockerfile?"));
        assert!(DEPLOY.contains("Usually no"));
    }

    #[test]
    fn test_services_explains_routing() {
        assert!(SERVICES.contains("gateway strips the /api"));
        assert!(SERVICES.contains("fetch(\"/api/users\")"));
    }
}
