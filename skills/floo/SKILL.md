---
name: floo
description: floo CLI discovery, operating invariants, and safety rules. Use when running floo commands, writing CLI integrations, debugging CLI behavior, or working with a floo project.
user-invocable: false
---

# floo CLI

The installed `floo` binary is the version-matched source for command syntax and offline platform guidance. Do not rely on remembered flags or make a web request before checking it.

## Discover before acting

Use these local surfaces in order:

1. `floo commands --json` for the machine-readable command tree.
2. `floo <command> --help` for exact flags, arguments, and examples.
3. `floo docs --json` for the offline topic catalog.
4. `floo docs <topic> --json` for version-matched platform guidance.

The JSON docs index includes `schema_version`, `cli_version`, topic summaries, and aliases. If the installed binary lacks a needed capability, run `floo update` and check again. Use getfloo.com only when the bundled knowledge is insufficient and website access is permitted.

For automation, pass `--json`. JSON responses go to stdout; human output goes to stderr. Parse the response envelope instead of screen-scraping prose.

## Deploy invariant

Deploys are git-driven:

- A push or merge to the connected branch deploys dev.
- A GitHub release promotes prod.
- The CLI never uploads source and `floo init` only writes local config.
- `floo redeploy` is for a no-code rebuild from connected GitHub source, such as applying changed environment values.

There is no normal deploy command. Validate with `floo preflight`, push through git, then observe with the current `deploys` and `logs` help surfaces.

## Source of truth

Auditable app shape and policy belong in `floo.app.toml` and move through git. This includes services, routes, access policy, cron, domain bindings, and managed-service declarations supported by the installed version. Opaque secret values stay outside git and are written through the CLI. Stateful resources and external bindings have explicit CLI lifecycle actions; omitting a declaration never grants permission to destroy stored data.

When older projects use a legacy authoring surface, follow the migration guidance in `floo docs config` or `floo docs services`. Do not create a second write path for the same state.

## Audit every mutation

No state change is complete until a read-only command confirms the resulting state.

- After editing floo config, run `floo preflight --json`.
- Before a mutation, use its `--preflight` form when available.
- After an environment change, inspect the relevant `env` read surface and run preflight.
- After a service or domain change, inspect its list/show surface and run preflight.
- After a git-triggered deploy, watch the deployment and inspect runtime logs.
- If the audit differs from intent, stop and investigate before another mutation or push.

`--dry-run` is a compatibility alias for `--preflight`. Use `--preflight` in new work.

## Destructive actions

Resolve the exact app, environment, service, and resource before destructive work. Preview first, read the command's current help, and request explicit user authorization for the specific data-bearing target. Never infer approval from a general request or bypass a typed data-loss confirmation.

## Secrets

Never place credentials in source, committed `.env` files, floo TOML, logs, errors, or frontend build variables. Prefer stdin and the CLI's write-only secret mode when available. After writing a secret, verify only its key and metadata; do not attempt to reveal its value.

## Topic routing

- Setup and first deploy: `floo docs quickstart`
- Decision flow: `floo docs golden-path`
- Config and secret behavior: `floo docs config`
- Services and data: `floo docs services`
- Managed-service health and accounts drift: `floo docs doctor`
- Git-driven deployment: `floo docs deploy`
- Routes and access controls: `floo docs edge`
- Hosted-app authentication: `floo docs auth`
- Scheduled jobs: `floo docs cron`
- Preview environments: `floo docs previews`
- Outbound networking: `floo docs egress`

If the CLI or platform is confusing, submit a concise report with `floo feedback "sanitized description" --json`, including the failing command, sanitized output, expected behavior, and reproduction context.
