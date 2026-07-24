floo - Manage and observe git-driven web app deploys.

floo is a deployment platform. GitHub pushes and releases drive deploys.
The CLI handles local setup, configuration validation, management, and
observation. All source comes from GitHub - the CLI never uploads code.

## Core Concepts

- **Apps** are the top-level unit. Each app has a unique name and URL.
- **Services** are deployable components inside an app (web servers, APIs, workers, databases).
- **Deploys** are immutable snapshots of your code, built into containers and deployed to the cloud.

## First Deploy

  1. `floo auth login` - authenticate
  2. `floo init <name>` - scaffold config files (local only)
  3. `floo apps github connect owner/repo` - connect to GitHub (triggers first deploy)
  4. `floo apps show <name>` - see your app's URL and status

  After the first deploy, push to GitHub to deploy: `git push origin main`.
  Watch progress with `floo deploys watch --app <name>`.
  Use `floo redeploy` only to force a redeploy (e.g., after updating env vars).
  Use `floo preflight` to validate config before pushing.

  floo --help          - all available commands
  floo <command> --help - details for a specific command
