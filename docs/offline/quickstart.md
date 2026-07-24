floo Quickstart - End-to-End Walkthrough

## Prerequisites

  - Your code must be in a **GitHub repository** (public or private).
    floo pulls source from GitHub - it does not upload local files.
  - The floo GitHub App must be installed on your GitHub org/account.
    The CLI opens GitHub to grant access during `floo apps github connect`.

## Agents & CI (headless environments)

  Agents and CI pipelines can deploy without a browser:

  1. A human installs the floo GitHub App on the org (one-time):
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

  Declare auditable intent in floo.app.toml:

  [managed.primary]
  type = "postgres"
  tier = "basic"

  [managed.cache]
  type = "redis"

  [managed.uploads]
  type = "storage"

  The first git-triggered deploy provisions missing declarations.
  Credentials arrive as runtime env vars (Postgres: DATABASE_URL + PG*,
  Redis: REDIS_URL, Storage: STORAGE_BUCKET). Removing a declaration does
  not destroy stored data. Use the explicit services lifecycle only after
  confirming the exact resource.

## 4. Validate Config

  floo preflight --json

  Checks config files, service graph, ports, and Dockerfiles locally - no
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

  Push to GitHub - the webhook triggers a deploy automatically:

  git push origin main
  floo deploys watch --app my-app

  Use `floo redeploy --app my-app` only when you need to redeploy without
  a code change (e.g., after updating env vars).

## 8. Local Development

  floo dev --app my-app

  Runs your service locally with live Cloud SQL access and the same env vars
  as the deployed version. Requires dev_command set on the service in floo.app.toml.

## What Creates What

  floo init                - local config files only (no API call)
  floo redeploy            - force a redeploy of an existing app (no code-change required)
  floo apps github connect - creates app if needed, connects GitHub, triggers first deploy
  [managed.<name>]         - declares managed-service intent in floo.app.toml
  Cron jobs are still declared in floo.app.toml ([cron.<name>]) and
  provisioned automatically on deploy - they're stateless and config-driven.
