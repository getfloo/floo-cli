floo - Golden Path

## Before You Start

  You need:
  - A project directory with source code (Dockerfile optional - floo auto-detects runtimes)
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

  The `floo auth login` command opens a browser. New users create an account automatically.
  In headless/CI environments, use: floo auth login --api-key <key>

  floo installs a GitHub webhook when you connect. After that, every
  git push triggers a build and deploy automatically.

  Check your deploy succeeded:
  floo apps show my-app

## How to Ship Changes

  git add . && git commit -m "feat: my change"
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

  Declare it in floo.app.toml:

  [managed.primary]
  type = "postgres"
  tier = "basic"

  Commit the declaration and push to GitHub:
  git add floo.app.toml && git commit -m "feat: add postgres"
  git push origin main

  The database is available on the next deploy. Credentials arrive as a
  standard DATABASE_URL plus PGHOST/PGPORT/PGDATABASE/PGUSER/PGPASSWORD.

## How to Add a Custom Domain

  1. Add the domain:

     floo domains add app.example.com --app my-app

  2. The output shows the traffic and claimant-control records to add:

     CNAME app.example.com -> my-app.on.getfloo.com
     TXT _floo-verify.app.example.com -> <claim token from the command>

  3. Add both records in your DNS provider (Cloudflare, Route 53, etc).

  4. Run `floo domains verify app.example.com --app my-app` or click
     "Verify DNS" in the dashboard. Once verified, status changes to active
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
  floo logs query --app my-app --json --tail 100 --cursor "$NEXT_CURSOR"
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
  Add a custom domain                   | floo domains add example.com --app my-app (then add the shown DNS records)
  Verify a custom domain                | floo domains verify example.com --app my-app
  View logs                             | floo logs query --app my-app
  Run locally with prod credentials     | floo dev --app my-app (requires dev_command)
