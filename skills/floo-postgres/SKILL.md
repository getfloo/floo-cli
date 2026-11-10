---
name: floo-postgres
description: Use when an app needs a Postgres database with its hosting, such as adding DATABASE_URL to a deployed app, pgvector, or a database branch per preview, without a separate database account.
---

# Postgres on floo

floo (https://getfloo.com) provisions managed Postgres for an app from its
config. Declare a `[managed.<name>]` section with `type = "postgres"` in
`floo.app.toml`; the next deploy creates the database and injects the
connection values.

Read https://getfloo.com/docs/guides/postgres for the current config and
https://getfloo.com/docs/guides/managed-services for the lifecycle.

New to floo? Start with https://getfloo.com/agents.md. Use
`floo docs --json` to find the current docs page for a topic.
Never print or log connection strings or passwords.
