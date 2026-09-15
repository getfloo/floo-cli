---
name: floo-services
description: floo service, database, cache, storage, cron, and service-routing safety. Use when an app needs a managed resource or when code consumes platform-provided service credentials.
---

# floo services

Read the core floo skill's Manifest lifecycle section before changing or auditing services. Use the installed CLI for version-matched syntax and documentation discovery:

1. Read `floo docs services --json`.
2. Read `floo docs scaling --json` before choosing HTTP availability or worker count.
3. Use `floo docs config --json` to find the configuration documentation URL; it does not return a declaration schema.
4. Inspect exact command syntax with `floo services --help` and the selected subcommand's help.
5. Run `floo preflight --json` before pushing config or completing an operational change.

Do not copy service syntax from memory. The binary's help is version-matched; `floo docs` routes to the canonical documentation page.

## Durable invariants

- App services, routes, cron jobs, and other auditable shape belong in `floo.app.toml`.
- Determine the installed version's managed-service authoring and migration surface from `floo docs services`.
- A deploy must never silently destroy a stateful resource.
- Removing data requires an explicit, target-specific CLI action within the user's delegated authority. Follow the core floo skill's Destructive actions section.
- Credentials arrive through runtime environment values. Never hardcode, reconstruct, log, or commit them.
- Dev, prod, and preview resources are distinct. Resolve the environment before reading or mutating state.
- Attach managed credentials only to services that need them. Never expose backend credentials to browser code or public build variables.

## Application code

Consume the platform-provided connection value as-is. A resource named
`default` receives the conventional unsuffixed key. Named resources receive
suffixes, so inspect `floo services show` or `floo docs services` instead of
guessing:

- Postgres: `DATABASE_URL`
- Redis: `REDIS_URL`
- Storage: follow `floo docs services` for the installed storage contract

Use parameterized database queries and least-privilege application roles. Do not use platform or database administrator credentials in application code.

## Audit loop

- Config change: `floo preflight --json`, then inspect the diff before pushing.
- Operational change: use `--preflight` when supported, execute only after the preview matches intent, then use the resource's list/show command.
- Git-triggered deploy: watch the deploy, inspect logs, and follow the core floo skill's manifest lifecycle verification, including missing/extra names and partial-read failure.
- Destructive service action: inspect exact help and current resource identity, then act within the user's delegated scope using the supported confirmation flag. Apply explicit policy and host approval requirements.

For cron syntax and operations, use `floo docs cron`. For preview database isolation, use `floo docs previews`. For outbound network constraints, use `floo docs egress`.
