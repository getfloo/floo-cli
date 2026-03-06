# Floo CLI

Floo deploys web apps from the terminal. It's CLI-first — all management happens through `floo` commands.

## Getting Started

1. `floo auth login` — authenticate (opens browser)
2. `floo init <app-name>` — scaffold a project config
3. `floo deploy` — deploy the app
4. `floo apps status <name>` — see your app's URL and status

## Self-Discovery

The CLI is fully self-documenting:

- `floo --help` — all commands
- `floo <command> --help` — command details with examples
- `floo docs` — how the platform works (services, deploys, config)
- `floo commands --json` — structured command tree for agents
- `floo <command> --dry-run` — preview what a command will do before executing

## Agent Output

Every command supports `--json`. JSON goes to stdout, human output to stderr.

```bash
floo deploy --json 2>/dev/null | jq '.data.app.url'
```

Errors return: `{"success": false, "error": {"code": "...", "message": "...", "suggestion": "..."}}`
