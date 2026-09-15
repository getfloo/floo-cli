---
name: floo-services
description: floo service, database, cache, storage, cron, and service-routing safety. Use when an app needs a managed resource or when code consumes platform-provided service credentials.
---

# floo services

Read the core floo skill's Manifest lifecycle section before changing or auditing services. Use the installed CLI for version-matched syntax and documentation discovery:

1. Run `floo docs services --json` and read the returned web page.
2. Run `floo docs scaling --json` and read the linked guide before choosing HTTP availability or worker count.
3. Use `floo docs config --json` to find the configuration documentation URL; it does not return a declaration schema.
4. Inspect exact command syntax with `floo services --help` and the selected subcommand's help.
5. Run `floo preflight --json` before pushing config or completing an operational change.

Do not copy service syntax from memory. The binary's help is version-matched; `floo docs` routes to the canonical documentation page.

## Durable invariants

- App services, routes, cron jobs, and other auditable shape belong in `floo.app.toml`.
- Read the current managed-service authoring and migration guide linked by `floo docs services`; use command help for installed syntax.
- A deploy must never silently destroy a stateful resource.
- Removing data requires an explicit, target-specific CLI action and user authorization.
- Credentials arrive through runtime environment values. Never hardcode, reconstruct, log, or commit them.
- Dev, prod, and preview resources are distinct. Resolve the environment before reading or mutating state.
- Attach managed credentials only to services that need them. Never expose backend credentials to browser code or public build variables.

## Application code

For Postgres, Redis, and Storage, consume platform-provided connection values
as-is. Read credential naming and storage contracts in the guide linked by
`floo docs services`; do not guess variable names or reconstruct credentials. Treat values such as `DATABASE_URL`
and `REDIS_URL` as secrets.

Use parameterized database queries and least-privilege application roles. Do not use platform or database administrator credentials in application code.

## Audit loop

- Config change: `floo preflight --json`, then inspect the diff before pushing.
- Operational change: use `--preflight` when supported, execute only after the preview matches intent, then use the resource's list/show command.
- Git-triggered deploy: inspect `floo deploys status --json` and follow the core floo skill's manifest lifecycle verification, including missing/extra names and partial-read failure.
- Destructive service action: inspect exact help and current resource identity, then obtain explicit authorization for that exact app, environment, and service.

For cron syntax and operations, use `floo docs cron`. For preview database isolation, use `floo docs previews`. For outbound network constraints, use `floo docs egress`.
