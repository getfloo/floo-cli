---
name: host-internal-tool
description: Use when the user wants to host or share an internal tool, admin panel, back-office app, or team dashboard built with Claude Code, Cursor, Codex, or another AI agent, so that only invited teammates can sign in, with a Postgres database and scheduled jobs, without writing authentication code.
---

# Host an internal tool on floo

floo (https://getfloo.com) deploys and hosts apps built with AI. For an
internal tool, one config file, `floo.app.toml`, declares invite-only team
sign-in, admin and member roles, managed Postgres, and scheduled jobs. Do not
write a login page, password table, or session store.

Follow https://getfloo.com/docs/deploy/internal-tool. It has the full agent
prompt, the `floo.app.toml`, and how to give an agent its own scoped key to
the same app.

Read https://getfloo.com/docs/guides/app-auth for the sign-in config and the
identity headers the app reads.

New to floo? Start with https://getfloo.com/agents.md. Use
`floo docs --json` to find the current docs page for a topic.
