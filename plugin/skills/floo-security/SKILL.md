---
name: floo-security
description: Security invariants for applications deployed on floo. Use for authentication, secrets, database connections, cookies, CORS, access policy, environment values, and production readiness.
---

# floo application security

Start with the installed, offline guidance:

- `floo docs auth --json` for hosted-app identity.
- `floo docs edge --json` for network admission and access policy.
- `floo docs config --json` for environment and write-only secret behavior.
- `floo <command> --help` for exact current syntax.

Use the website only if the binary's version-matched guidance is insufficient and website access is permitted.

## Secrets

- Never hardcode or commit credentials, tokens, connection strings, or populated `.env` files.
- Never put secret values in floo TOML, frontend build variables, logs, errors, traces, or API responses.
- Prefer stdin and the CLI's write-only secret mode.
- Read secrets through the runtime environment and fail closed when a required value is missing.
- Verify secret writes by key and metadata only. Never try to read a write-only value back.

## Data access

- Use floo-provided connection values without reconstructing them.
- Use parameterized queries and least-privilege roles.
- Keep dev, prod, and preview credentials separate.
- Return generic client errors while recording sanitized diagnostic context internally.

## Identity and browser boundaries

- With gateway-managed accounts, trust only the identity contract documented by `floo docs auth`.
- Do not build a second password or session system when managed accounts mode owns authentication.
- If application-owned sessions are required, use secure, HTTP-only cookies with bounded lifetime and server-side validation.
- Never store bearer tokens in browser local storage.
- Restrict CORS to the intended origins and never disable TLS verification.

## Platform policy

Auditable access and edge policy belongs in `floo.app.toml` and moves through git. Inspect it with read-only CLI surfaces, run `floo preflight --json`, and review the diff before pushing. Do not create an imperative second write path for the same policy.

Resolve the exact app and environment before any security mutation. Preview with `--preflight` where supported, audit the resulting state, and require explicit user authorization for destructive or data-bearing targets.
