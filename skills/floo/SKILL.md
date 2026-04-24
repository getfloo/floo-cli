---
name: floo
description: Floo CLI command reference and patterns. Use when running floo commands, writing CLI integrations, debugging CLI behavior, or when the user mentions floo deploy, floo init, floo logs, floo env, or any floo subcommand.
user-invocable: false
---

# Floo CLI

Floo deploys web apps from the terminal. All management happens through `floo` commands.

## Audit-loop doctrine (read this first)

Agents are confident-and-wrong by default. Preflight is the mechanism that turns confident-wrong into confident-corrected. These rules are not advisory — following them is how you ship reliable work on floo.

**No state change is complete until a read-only command has confirmed the change landed as expected.**

1. **After any edit to `floo.app.toml` or `floo.service.toml`, run `floo preflight`.** Not "after a batch of edits" — every logical change. Read the output, confirm it matches intent.
2. **After any mutation command (`floo env set/remove`, `floo services add/remove`, `floo domains add/remove`), run `floo preflight`.** Mutations cascade; preflight is the receipt.
3. **Before `git push` / merge (the deploy trigger), run `floo preflight`.** Last safe checkpoint before the irreversible action. No exceptions.
4. **If preflight shows changes you didn't intend, stop. Do not push.** Unexpected diff = silent corruption. Investigate before acting.
5. **Use `floo preflight --json` for structured parsing.** Parse plans; don't screen-scrape.

### Mutation-and-audit pairs

Every mutation has a read-only partner that confirms it landed:

| Mutation | Audit |
|---|---|
| edit `floo.app.toml` / `floo.service.toml` | `floo preflight` |
| `floo env set/remove/import` | `floo preflight` + `floo env list` |
| `floo services add/remove` | `floo preflight` + `floo services list` |
| `floo domains add/remove` | `floo domains list` (verify CNAME target) |
| merge to main (deploy) | `floo deploy watch` + `floo logs --live` |

If a command you're about to run doesn't have an obvious audit partner, stop and ask before running it.

## State model: stateless vs stateful

Every primitive on floo belongs on one of two surfaces. Knowing which applies prevents the most common class of mistake (silent data loss):

- **Stateless resources live in TOML** — Cloud Run services, gateway routes, cron jobs, build args, health checks, resource limits. Recoverable from code; removal from TOML is declarative and safe.
- **Stateful resources live in the CLI** — managed Postgres/Redis/Storage, env vars, custom domains, API keys. Hold data or credentials; removal is an explicit command with confirmation, never a TOML edit.

**Deploy never destroys a stateful resource, regardless of TOML contents.** Removing `[postgres]` from `floo.app.toml` does NOT deprovision your database — it produces a preflight warning pointing you to `floo services remove postgres`.

Legacy `[postgres]`/`[redis]`/`[storage]` sections in `floo.app.toml` are deprecated. They continue to auto-provision on first deploy during the transition window, but new apps should use `floo services add <type>` directly.

## Getting Started

1. `floo auth login` — sign up or log in (opens browser; use `--api-key <key>` for CI/headless)
2. `floo init <app-name>` — scaffold config files (local only, no API call)
3. `floo apps github connect owner/repo` — connect to GitHub and trigger first deploy
4. `floo apps status <name>` — see your app's URL and status

After the first deploy, push to GitHub to trigger deploys automatically. Watch progress with `floo deploy watch`. All source comes from GitHub — the CLI never uploads code.

## How Deploys Work

Pushing to GitHub triggers a deploy via webhook. Watch it with `floo deploy watch --app <name>`. Use `floo redeploy` only to force a redeploy without a code change (e.g., after updating env vars). Use `floo preflight` to validate config before pushing.

Normal workflow:

```bash
floo preflight                                  # validate config
git push origin main && floo deploy watch --app <name>
```

Force redeploy (after env var change):

```bash
floo env set API_KEY=new-value --app my-app
floo redeploy --app my-app
```

The API pulls source from GitHub, builds a container via Cloud Build, and deploys to Cloud Run. GitHub must be connected first (`floo apps github connect`).

- `floo init` creates local config files only — no app is registered on the platform
- `floo redeploy` forces a redeploy from GitHub HEAD (auto-creates the app if it doesn't exist)
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
ingress = "public"    # public (internet-facing) | internal (only reachable by other services in the app)
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

## Preflight

`floo preflight` is the canonical audit surface on floo. Two forms:

- **`floo preflight`** (command, noun form) — full diff between declared state (TOML + CLI state) and what's actually deployed. Read-only. Use this as the audit step in every mutation-and-audit pair above.
- **`--preflight`** (flag on any mutation) — simulates that one action. Shows the diff, exits without mutating. Use for spot-checking one change.

```bash
floo preflight --json                    # full audit (no side effects)
floo redeploy --preflight --json         # preview redeploy without executing
floo env set FOO=bar --preflight         # preview a single mutation
```

`--dry-run` is a deprecated alias for `--preflight` — it still works with a one-line deprecation notice, but new code should use `--preflight`. One word, one concept.

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

### Preflight and Redeploy

```bash
floo preflight                                     # audit declared vs deployed state
floo preflight --json                              # structured plan for agents
floo redeploy --app my-app                         # force redeploy (after env var changes)
floo redeploy --restart --app my-app               # restart without rebuilding
floo redeploy --services api --app my-app          # redeploy specific service
floo redeploy --preflight --app my-app             # preview redeploy without executing
```

### Managed services (postgres, redis, storage)

Managed services are **stateful** — they carry data and outlive deploys. They are CLI-managed, not TOML-declared, because destroying a database on a config edit would be catastrophic.

```bash
floo services list --app my-app                    # see everything (app services + managed)
floo services info postgres --app my-app           # inspect a managed service
```

Legacy `[postgres]`/`[redis]`/`[storage]` sections in `floo.app.toml` still auto-provision during the transition window, but emit a deprecation notice on every deploy. Prefer the CLI surface for new apps.

### Destructive commands

Commands that destroy state follow a tiered confirmation model:

- **Tier 1 (reversible, no data):** `env remove`, scaling. Idempotent; no prompt.
- **Tier 2 (destructive but recoverable from code):** `domains remove`, `deploy rollback`. `y/N` prompt, `--yes` to skip.
- **Tier 3 (unrecoverable data loss):** `apps delete`, `services remove <managed>`, `orgs delete`. Typed-name confirmation, or `--yes-i-know-this-destroys-data` to skip in automation. Never a plain `--yes`.

Every destructive command's `--json` output includes `destructive: true, data_loss: true|false, tier: N` so agents can reason about risk from the contract, not the prompt text.

**Rule for agents:** never use `--yes-i-know-this-destroys-data` in a script without explicit user confirmation of the specific resource being destroyed.

### Deploy History

```bash
floo deploy list --app my-app                      # deploy history
floo deploy logs <deploy-id> --app my-app          # build logs
floo deploy watch --app my-app                     # stream progress
floo deploy rollback my-app <deploy-id>            # rollback
```

### Custom Domains

Adding a custom domain is a three-step process:

1. **Add the domain** — registers it with floo and returns CNAME instructions:
   ```bash
   floo domains add app.example.com --app my-app
   # multi-service: floo domains add app.example.com --app my-app --services frontend
   ```

2. **Add a CNAME record** at the user's DNS provider (Cloudflare, Route 53, etc):
   - Type: `CNAME`
   - Name: `app.example.com`
   - Target: `my-app.on.getfloo.com` (shown in the CLI output)

3. **Verify** — click "Verify DNS" in the dashboard. The API checks that the CNAME resolves to the expected gateway host. Once verified, domain status changes to `active` and org admins receive a confirmation email.

The dashboard auto-polls pending domains every 30 seconds.

```bash
floo domains list --app my-app                     # check domain status
floo domains remove app.example.com --app my-app   # remove a domain
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
