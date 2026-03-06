# Floo CLI

Floo deploys web apps from the terminal. All management happens through `floo` commands.

## Getting Started

1. `floo auth login --api-key <key>` — authenticate (use `--api-key` for non-interactive/CI; omit for browser flow)
2. `floo init <app-name>` — scaffold config files in the current directory
3. `floo deploy` — detect runtime, archive source, build, deploy
4. `floo apps status <name>` — see your app's URL and status

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

Multi-service app — `floo.app.toml` in project root, plus a `floo.service.toml` in each service dir:

```toml
[app]
name = "my-app"

[services.api]
path = "./api"

[services.web]
path = "./web"
```

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
