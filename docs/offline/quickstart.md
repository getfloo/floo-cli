floo Quickstart - End-to-End Walkthrough

## Prerequisites

  - Your code must be in a **GitHub repository** (public or private).
    floo pulls source from GitHub - it does not upload local files.
  - The floo GitHub App must be installed on your GitHub org/account, and
    installed through a link floo minted. The CLI opens GitHub to grant access
    during `floo apps github connect`, and that redirect is what binds the
    installation to your floo org.

    Installing from github.com directly skips the redirect, so floo ends up
    with no binding: GitHub shows the App installed while every connect fails
    with GITHUB_INSTALLATION_NOT_AUTHORIZED. Run `floo apps github setup` to
    mint a fresh link and recover — no need to uninstall anything.

## Agents & CI (headless environments)

  Agents and CI pipelines can deploy without a browser:

  1. The agent authenticates: floo auth login --api-key <key>
  2. The agent mints an install link: floo apps github setup --no-browser
     A human opens that link once and grants access to the repo. Use this link
     rather than github.com/apps/getfloo — only the minted link carries the
     handshake that binds the installation to your floo org.
     The human must install on the account that OWNS the repository. GitHub
     scopes an installation to one account, so installing on an organization
     grants nothing for a repo owned by a personal account, and vice versa.
     On the grant page, the repository must appear under "Repository access" —
     an installation with the repo unselected reports success and still fails
     to connect.
  3. The agent runs floo init, edits floo.app.toml, and runs floo preflight
  4. The agent commits and pushes the generated config to GitHub
  5. The agent connects: floo apps github connect owner/repo --no-browser
     Instead of opening a browser, --no-browser fails with the complete grant
     instruction: the account to install on, the exact URL, the repository to
     select there, and the command to re-run. Completing it requires a human in
     a browser — an agent cannot grant GitHub access, and re-running before the
     grant lands will fail identically. Escalate the instruction to a human
     rather than retrying.
  6. Subsequent deploys: git push triggers automatic deploys via webhook

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

  [managed.default]
  type = "postgres"

  [managed.cache]
  type = "redis"

  [managed.uploads]
  type = "storage"

  The first git-triggered deploy provisions missing declarations. The optional
  tier field is retained for compatibility but does not change capacity.
  Credentials arrive as runtime env vars. The default Postgres block owns
  DATABASE_URL + PG*. Named resources own suffixed keys: REDIS_URL_CACHE,
  STORAGE_BUCKET_UPLOADS, and STORAGE_URL_UPLOADS. Removing a declaration
  does not perform terminal data deletion.

## 4. Validate Config

  floo preflight --json

  Checks config files, service graph, ports, and Dockerfiles locally - no
  auth or GitHub connection required. Fix any warnings before deploying.

## 5. Push, Connect to GitHub, and Deploy

  Commit and push the files that floo init created:

  git add floo.app.toml Dockerfile AGENTS.md
  git commit -m "chore: configure floo"
  git push origin main

  Then connect the repository:

  floo apps github connect owner/my-project

  This does three things:
  1. Creates the app on floo (if it doesn't exist)
  2. Connects your GitHub repo as the source
  3. Triggers the first deploy (source pulled from GitHub, built, deployed)

  Connect pulls source from GitHub. If the generated config is only local, the
  first deploy cannot see it. Use --no-deploy to skip the automatic deploy.

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

  Runs your service locally with live managed Postgres access and the same env vars
  as the deployed version. Requires dev_command set on the service in floo.app.toml.

## What Creates What

  floo init                - local config files only (no API call)
  floo redeploy            - force a redeploy of an existing app (no code-change required)
  floo apps github connect - creates app if needed, connects GitHub, triggers first deploy
  [managed.<name>]         - declares managed-service intent in floo.app.toml
  Cron jobs are still declared in floo.app.toml ([cron.<name>]) and
  provisioned automatically on deploy - they're stateless and config-driven.
