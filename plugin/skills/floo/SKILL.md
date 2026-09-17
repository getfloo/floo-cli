---
name: floo
description: floo CLI discovery, operating invariants, and safety rules. Use when running floo commands, writing CLI integrations, debugging CLI behavior, or working with a floo project.
user-invocable: false
---

# floo CLI

The installed `floo` binary is the version-matched source for command syntax. Check it before relying on remembered flags. Platform guidance lives at https://getfloo.com/docs; `floo docs <topic>` prints a URL, not the page content.

## Discover before acting

Use these local surfaces in order:

1. `floo commands --json` for the machine-readable command tree.
2. `floo <command> --help` for exact flags, arguments, and examples.
3. `floo docs --json` for the topic catalog and its documentation URLs.
4. `floo docs <topic> --json` for the topic's documentation URL and metadata.

The JSON docs index includes `schema_version`, `cli_version`, topic summaries, and aliases. These describe the CLI response, not a manifest schema or version-pinned documentation. `floo docs config --json` returns an unversioned URL, not a schema. The config topic has the alias `app-toml`; there is no `manifest` topic. No platform knowledge articles are bundled. Read the linked page only when website access is permitted; otherwise report that guidance is unavailable. If the installed binary lacks a needed capability, run `floo update` and check again.

For automation, pass `--json`. JSON responses go to stdout; human output goes to stderr. Parse the response envelope instead of screen-scraping prose.

## Deploy invariant

Use the topic routing below and read the linked web guide before setup,
deployment, or recovery.

Deploys are git-driven: pushes deploy dev and GitHub releases promote prod.
App configuration belongs in the repository; commit and
push generated config before connecting because the first deploy reads GitHub.
After cloning a project, read its `AGENTS.md` before editing.

Never cache GitHub tokens or configure global git credentials for managed
projects. The internal git credential helper emits a plaintext password
protocol and must not be logged.

## Source of truth

Auditable app shape and policy belong in `floo.app.toml` and move through git. This includes services, routes, access policy, cron, domain bindings, and managed-service declarations supported by the installed version. Opaque secret values stay outside git and are written through the CLI.

When older projects use a legacy authoring surface, follow the migration guidance in `floo docs config` or `floo docs services`. Do not create a second write path for the same state.

## Manifest lifecycle

- names are identities. Treat each `[services.NAME]` name as a distinct service.
- rename does not migrate. Rename is create+remove, with no automatic data migration or retirement of the old service.
- removing a block does not retire the runtime. No consumer command retires an app service; `floo services remove` targets managed resources only.
- services remove does not edit the manifest. Leaving a managed-service declaration means the next push re-provisions it. `enabled=false` is a removal proposal, not proof of cleanup or authorization to destroy data.
- always `[services.NAME.env]` in `floo.app.toml`. This also applies to single-service apps; do not use top-level `[env]` for their contracts.
- verify by comparing `services list --json` names to the manifest and treating a partial read as failure. Run `floo services list --app <app> --env <env> --json` and compare both `app_services` and `managed_services` names with their declarations, reporting missing and extra names. A `running` status can be inherited from the environment deploy and does not prove per-service health. Managed-list failure can appear as an empty list in success JSON, with its warning suppressed in JSON mode. Repeat the read without `--json` to check for a partial-view warning; if completeness is uncertain, stop and report verification failure.
- report retained resources rather than claiming cleanup. Name retained app services, managed resources, and scheduled jobs explicitly, and report anything whose retention could not be verified.

## Audit every mutation

No state change is complete until a read-only command confirms the resulting state.

- After editing floo config, run `floo preflight --json`.
- Before a mutation, use its `--preflight` form when available.
- After an environment change, inspect the relevant `env` read surface and run preflight.
- A `[domains."<host>"]` block goes live on the prod release. Publish the DNS records the deploy output prints (also `floo domains show <host>`), then `floo domains watch <host>`.
- After a git-triggered deploy, use `floo deploys status --json` to verify the result. Inspect logs only when needed and keep sensitive output out of transcripts.
- If the audit differs from intent, stop and investigate before another mutation or push.

`--dry-run` is a compatibility alias for `--preflight`. Use `--preflight` in new work.

## Destructive actions

floo connections delegate the user's existing workspace authority by default, including production and explicit destructive operations. The default is shared across accounts and plans. Existing roles, narrow credentials, and explicit org/app approval policies still apply; an API key cannot approve a policy-required human decision.

Resolve the exact app, environment, service, and resource. Preview where supported and read the command's current help. When the user's delegated task authorizes the operation, proceed with the supported non-interactive confirmation flag, including `--yes-i-know-this-destroys-data` for tier-3 commands. Do not request a second confirmation solely because floo labels the operation destructive. Ask when the operation is outside the delegated scope or the agent host requires approval. Removing a manifest declaration alone never authorizes provider-data deletion.

## Secrets

Never place credentials in source, committed `.env` files, floo TOML, logs, errors, or frontend build variables. Prefer stdin and the CLI's write-only secret mode when available. After writing a secret, verify only its key and metadata; do not attempt to reveal its value.

## Topic routing

- Managed projects without a GitHub account (hosted invite-only login and managed Postgres): `floo projects --help` for create, list, and clone; https://getfloo.com/agents.md for the full workflow.
- Setup and first deploy: `floo docs golden-path` or `floo docs quickstart`
- GitHub installation, authorization, and recovery: `floo docs github`
- Config and secret behavior: `floo docs config`
- Services and data: `floo docs services`
- Availability, scaling, and CPU behavior: `floo docs scaling`
- Managed-service health and accounts drift: `floo docs doctor`
- Git-driven deployment: `floo docs deploy`; `floo redeploy --help` for restart and rebuild syntax
- Routes and access controls: `floo docs edge`
- Hosted-app authentication: `floo docs auth`
- Scheduled jobs: `floo docs cron`
- Preview environments: `floo docs previews`
- Outbound networking: `floo docs egress`

If the CLI or platform is confusing, submit a concise report with `floo feedback "sanitized description" --json`, including the failing command, sanitized output, expected behavior, and reproduction context.
