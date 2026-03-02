# Floo — Agent Skill

Floo is a deployment platform. CLI-first — the CLI is the primary interface. Deploy apps with `floo deploy`.

Every command supports `--json` for structured output: JSON goes to stdout, human-readable output goes to stderr. This means `floo deploy --json 2>/dev/null | jq` works for parsing.

## Authentication

```bash
floo auth login        # opens browser, handles login + account creation (device code flow)
floo auth logout       # clear stored credentials
floo auth whoami       # show current user
```

No auth required for `floo skills install` or `floo --help`.

Credentials are stored at `~/.floo/config.json`.

If signups are closed, you'll get a `SIGNUP_DISABLED` error — join the waitlist at https://getfloo.com.

## Platform Architecture

**Apps** contain **services**. An app is the top-level unit; services are deployable components.

### Service Types

- **User-managed** — your code (web servers, APIs, workers). Deployed from source via `floo deploy`.
- **Database** — managed Postgres, auto-provisioned. Connection string injected as `DATABASE_URL` env var. Inspect with `floo services info <name>`.
- **Gateway** — reverse proxy for custom domains and path-prefix routing. Optional, opt-in per app.

### Deploy Flow

1. Detect runtime (Dockerfile > package.json > pyproject.toml > go.mod > index.html)
2. Archive source (respects `.flooignore`)
3. Upload to Floo API
4. Build container image
5. Deploy to Cloud Run
6. Return live URL

## Config Files

### `floo.service.toml` — Single-Service Apps

```toml
[app]
name = "my-app"

[service]
port = 8080
runtime = "python"
build_command = "pip install -r requirements.txt"
```

### `floo.app.toml` — Multi-Service Apps

```toml
[app]
name = "my-app"

[[services]]
name = "api"
path = "./api"

[[services]]
name = "web"
path = "./web"
```

Each service directory has its own `floo.service.toml`.

## Command Reference

### Deploy

```bash
floo deploy [path] --json
# --app <name>        deploy to existing app
# --services <name>   deploy specific services (repeatable)
# --restart           restart without re-uploading source
```

JSON output: `data.app`, `data.deploy`, `data.detection`

`apps status` human output shows `Org:` line with the org slug. `apps list` includes an `Org` column. JSON output for both commands includes `org_id` per app.

### Apps

```bash
floo apps list --json                    # list all apps (includes Org column)
floo apps status <name> --json           # app details + org + services + deploy status
floo apps delete <name> --json           # delete an app (--force to skip prompt)
floo apps connect --repo owner/repo --installation-id <id> --app <name>  # GitHub auto-deploy
floo apps disconnect --app <name>        # remove GitHub connection
```

### Environment Variables

```bash
floo env set KEY=value --app <name> --json          # set env var
floo env set KEY=value --app <name> --restart --json # set + restart
floo env list --app <name> --json                    # list env vars
floo env get KEY --app <name> --json                 # get plaintext value
floo env remove KEY --app <name> --json              # remove env var
floo env import .env --app <name> --json             # import from .env file
# --services <name>   target specific services (repeatable)
```

### Custom Domains

```bash
floo domains add <hostname> --app <name> --json      # add domain
floo domains list --app <name> --json                 # list domains
floo domains remove <hostname> --app <name> --json    # remove domain
# --services <name>   target specific service
```

### Logs

```bash
floo logs --app <name> --json
# --tail <n>          number of lines (default: 100)
# --since <duration>  e.g. 1h, 30m, 2d, or ISO timestamp
# --severity <level>  DEBUG, INFO, WARNING, ERROR, CRITICAL
# --error             shorthand for --severity ERROR
# --services <name>   filter by service (repeatable)
# --search <text>     filter by text (case-insensitive)
# --live              stream logs in real-time (poll every 2s)
# --output <file>     write logs to file
```

### Services

```bash
floo services list --app <name> --json               # list all services
floo services info <service-name> --app <name> --json # service details
```

### Releases & Rollbacks

```bash
floo releases list --app <name> --json               # list releases
floo releases show <release-id> --app <name> --json  # release details
floo promote --app <name> --json                     # promote to prod (GitHub release)
floo rollbacks list --app <name> --json              # list deploys for rollback
floo rollback <app> <deploy-id> --json               # rollback to a previous deploy
```

### CLI Management

```bash
floo version --json                      # print installed version
floo update --json                       # update CLI binary
floo update --version v0.2.0 --json      # install specific version
floo skills install --path <dir>         # install agent skill to directory
floo skills install --print              # print skill content to stdout
```

### Auto-Update

The CLI checks for updates once every 24 hours in the background. Updates are downloaded silently and applied automatically on the next launch. This never blocks command execution.

- Check + download happen in a background thread during normal commands
- On the next run, the staged binary is swapped in before the command runs
- Nothing happens in `--json` mode (no noise for agents)
- Set `FLOO_NO_UPDATE_CHECK=1` to disable auto-update entirely
- Cache file: `~/.floo/version-check.json`
- Staged binary: `~/.floo/staged-update/`

## Error Codes

| Code | Meaning | Recovery |
|------|---------|----------|
| `NOT_AUTHENTICATED` | No API key found | Run `floo auth login` |
| `SIGNUP_DISABLED` | Account creation is closed | Join waitlist at https://getfloo.com |
| `DEPLOY_FAILED` | Build or deploy failed | Check build logs: `floo deploy --json \| jq '.data.deploy.build_logs'` |
| `DEPLOY_TIMEOUT` | Deploy didn't complete in time | Check status: `floo apps status <name> --json` |
| `APP_NOT_FOUND` | App name/ID doesn't exist | Verify name: `floo apps list --json` |
| `ENV_VAR_NOT_FOUND` | Env var key doesn't exist | List vars: `floo env list --app <name> --json` |
| `DOMAIN_ALREADY_EXISTS` | Domain already assigned | Check domains: `floo domains list --app <name> --json` |
| `PARSE_ERROR` | Unexpected API response | This is a bug — report it |

All errors in `--json` mode return: `{"success": false, "error": {"code": "...", "message": "...", "suggestion": "..."}}`

## Common Workflows

### Deploy and verify

```bash
floo deploy . --json
floo apps status <app-name> --json
floo logs --app <app-name> --json --since 1m --severity ERROR
```

### Set env var and restart

```bash
floo env set DATABASE_URL=postgres://... --app <name> --restart --json
```

### Check deploy status after failure

```bash
floo apps status <app-name> --json | jq '.data.services'
floo logs --app <app-name> --json --since 5m --error
```

### Rollback a bad deploy

```bash
floo rollbacks list --app <name> --json          # find the deploy ID
floo rollback <app-name> <deploy-id> --force --json
```

### Add a custom domain

```bash
floo domains add app.example.com --app <name> --json
# Then configure DNS: CNAME to the URL from floo apps status
```
