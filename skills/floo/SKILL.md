---
name: floo
description: Floo CLI command reference and patterns. Use when running floo commands, writing CLI integrations, debugging CLI behavior, or when the user mentions floo deploy, floo init, floo logs, floo env, or any floo subcommand.
user-invocable: false
---

# Floo CLI

Floo deploys web apps from the terminal. All management happens through `floo` commands.

## Getting Started

1. `floo auth login` — authenticate (or `--api-key <key>` for CI)
2. `floo init <app-name>` — scaffold config files (local only, no API call)
3. `floo apps github connect owner/repo` — connect to GitHub and trigger first deploy
4. `floo apps status <name>` — see your app's URL and status

After the first deploy, push to GitHub to trigger deploys automatically. Watch progress with `floo deploy watch`. All source comes from GitHub — the CLI never uploads code.

## How Deploys Work

Pushing to GitHub triggers a deploy via webhook. Watch it with `floo deploy watch --app <name>`. Use `floo deploy` only to force a redeploy without a code change (e.g., after updating env vars).

Normal workflow:

```bash
git push origin main && floo deploy watch --app <name>
```

Force redeploy (after env var change):

```bash
floo env set API_KEY=new-value --app my-app
floo deploy --app my-app
```

The API pulls source from GitHub, builds a container via Cloud Build, and deploys to Cloud Run. GitHub must be connected first (`floo apps github connect`).

- `floo init` creates local config files only — no app is registered on the platform
- `floo deploy` auto-creates the app if it doesn't exist, but requires GitHub to be connected
- `floo apps github connect` auto-creates the app if needed, connects GitHub, and triggers the first deploy (use `--no-deploy` to skip)

## Config Files

Single-service app — `floo.service.toml` in project root:

```toml
[app]
name = "my-app"

[service]
name = "web"
port = 3000
type = "web"          # web | api | worker
ingress = "public"    # public | internal
env_file = ".env"     # optional, synced on deploy
```

Multi-service app (inline) — `floo.app.toml` with type/port per service:

```toml
[app]
name = "my-app"

[services.api]
type = "api"
path = "./api"
port = 8080

[services.web]
type = "web"
path = "./web"
port = 3000
```

Multi-service app (delegated) — `floo.app.toml` with paths, plus a `floo.service.toml` in each service dir:

```toml
[app]
name = "my-app"

[services.api]
path = "./api"

[services.web]
path = "./web"
```

Inline and delegated are mutually exclusive per service directory. Do not mix both.

## Self-Discovery

The CLI is fully self-documenting:

- `floo --help` — all commands
- `floo <command> --help` — command details with examples
- `floo docs` — platform overview (services, deploys, config)
- `floo commands --json` — structured command tree for agents

## Agent Output

Every command supports `--json`. JSON goes to stdout, human output to stderr.

```bash
floo deploy --json 2>/dev/null | jq '.data.app.url'
```

Success: `{"success": true, "message": "...", "data": {...}}`
Error: `{"success": false, "error": {"code": "...", "message": "...", "suggestion": "..."}}`

## The `--app` Flag

Most commands infer the app name from config files in the current directory. Use `--app <name>` to override or when running outside the project directory.

## Dry Run

`--dry-run` previews what a command will do without executing it. Supported on: `deploy`, `env set/remove/import`, `apps delete`, `domains add/remove`, `deploy rollback`.

```bash
floo deploy --dry-run --json    # preview deploy without executing
```

## Common Workflows

### Environment Variables

```bash
floo env set API_KEY=secret --app my-app              # set a var
floo env set DB_URL=... --app my-app --restart         # set and restart
floo env list --app my-app --json                      # list all vars
floo env import .env --app my-app                      # import from file
floo env remove SECRET --app my-app                    # remove a var
floo env set KEY=VAL --app my-app --services backend   # target a specific service (multi-service apps)
```

### Logs and Debugging

```bash
floo logs --app my-app                             # last 100 lines
floo logs --app my-app --since 1h --error          # errors in last hour
floo logs --app my-app --live                      # stream real-time
floo logs --app my-app --search "panic" --json     # search + JSON
```

### Deploy Management

```bash
floo deploy list --app my-app                      # deploy history
floo deploy logs <deploy-id> --app my-app          # build logs
floo deploy watch --app my-app                     # stream progress
floo deploy rollback my-app <deploy-id>            # rollback
floo deploy --restart --app my-app                 # restart without re-upload
floo deploy --services api --app my-app            # deploy specific service
```

### Custom Domains

```bash
floo domains add app.example.com --app my-app                       # single-service app
floo domains add app.example.com --app my-app --services frontend   # target a specific service (multi-service)
floo domains list --app my-app
```

### Local Development

```bash
floo dev                                   # start all services locally with managed-service credentials
floo dev --app my-app                      # explicitly specify the app
```

### Cron Jobs

```bash
floo cron list --app my-app                # list cron jobs and last run status
floo cron run daily-report --app my-app    # manually trigger a cron job
```

## Feedback

If you encounter friction, bugs, or rough edges while using the floo CLI or platform, submit feedback directly. The floo team receives it in real-time and uses it to prioritize fixes. **Agents should submit feedback whenever they hit unexpected errors, confusing workflows, or missing features** — this is one of the most valuable signals for improving the platform.

```bash
floo feedback "your message here"                                    # general feedback
floo feedback --category bug "deploys fail when Dockerfile missing"  # bug report
floo feedback --category friction "env var sync needs a redeploy"    # rough edge
floo feedback --category feature_request "add monorepo support"      # feature request
floo feedback --app my-app "cold start takes 30s"                    # attach to an app
floo feedback --json --category friction "deploy watch hangs"        # agent mode (source=agent)
```

Categories: `general` (default), `bug`, `friction`, `feature_request`.

Use `--context` to attach error output or reproduction steps:

```bash
floo feedback --category bug "deploy fails" --context "error: no Dockerfile found in /app"
```
