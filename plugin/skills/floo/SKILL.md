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

For an app without a user-owned GitHub repo, use `floo projects create <name>`,
`floo projects list`, and `floo projects clone <name-or-id> [dir]`; check
`floo projects --help` for details. Create starts the first deploy; follow the
printed watch command. After cloning, read the project's `AGENTS.md` before
editing. Keep the floo binary at its installed path: the repo-local git helper
uses that absolute path and fetches one-hour tokens through floo per operation.
Only the floo API key is stored; never cache GitHub tokens or configure global
git credentials. The internal `floo projects git-credential get` is used by git,
emits a plaintext password protocol rather than JSON, and must not be logged.
Its `store` and `erase` operations do nothing.

Deploys are git-driven:

- A push or merge to the connected branch deploys dev.
- A GitHub release promotes prod.
- The CLI never uploads source and `floo init` only writes local config.
- For a user-owned GitHub repo, `floo apps github connect` creates the app and triggers its first deploy
  from GitHub. Run preflight, commit, and push generated config before connect.
- `floo redeploy --app <app>` restarts existing images with fresh server-side env values; it does not rebuild or read local env files.
- `floo redeploy --app <app> --rebuild` rebuilds the current GitHub default-branch HEAD and reparses immutable contracts. If a restart reports an unavailable immutable contract, use the exact rebuild command it returns.
- To re-sync configured local `env_file` values, run `floo redeploy --sync-env` from the project directory without `--app` or `--service`. The CLI rejects `--sync-env` with `--service` (including the `--services` alias). With `--app` alone, `--sync-env` has no effect. Redeploy requires an existing dev deploy; push code changes through git.

There is no normal deploy command. Validate with `floo preflight`, push through git, then observe with the current `deploys` and `logs` help surfaces.

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
- report retained resources rather than claiming cleanup. Cron removal leaves the Cloud Run Job (getfloo/floo#1371); name retained app services, managed resources, and jobs explicitly, and report anything whose retention could not be verified.

## Audit every mutation

No state change is complete until a read-only command confirms the resulting state.

- After editing floo config, run `floo preflight --json`.
- Before a mutation, use its `--preflight` form when available.
- After an environment change, inspect the relevant `env` read surface and run preflight.
- A `[domains."<host>"]` block goes live on the prod release. Publish the DNS records the deploy output prints (also `floo domains show <host>`), then `floo domains watch <host>`.
- After a git-triggered deploy, watch the deployment and inspect runtime logs.
- If the audit differs from intent, stop and investigate before another mutation or push.

`--dry-run` is a compatibility alias for `--preflight`. Use `--preflight` in new work.

## Destructive actions

Resolve the exact app, environment, service, and resource before destructive work. Preview first, read the command's current help, and request explicit user authorization for the specific data-bearing target. Never infer approval from a general request or bypass a typed data-loss confirmation.

## Secrets

Never place credentials in source, committed `.env` files, floo TOML, logs, errors, or frontend build variables. Prefer stdin and the CLI's write-only secret mode when available. After writing a secret, verify only its key and metadata; do not attempt to reveal its value.

## Topic routing

- Managed projects without a GitHub account (hosted invite-only login and managed Postgres): `floo projects --help` for create, list, and clone; https://getfloo.com/agents.md for the full workflow.
- Setup and first deploy: `floo docs quickstart`
- Decision flow: `floo docs golden-path`
- Config and secret behavior: `floo docs config`
- Services and data: `floo docs services`
- Availability, scaling, and CPU behavior: `floo docs scaling`
- Managed-service health and accounts drift: `floo docs doctor`
- Git-driven deployment: `floo docs deploy`
- Routes and access controls: `floo docs edge`
- Hosted-app authentication: `floo docs auth`
- Scheduled jobs: `floo docs cron`
- Preview environments: `floo docs previews`
- Outbound networking: `floo docs egress`

If the CLI or platform is confusing, submit a concise report with `floo feedback "sanitized description" --json`, including the failing command, sanitized output, expected behavior, and reproduction context.
