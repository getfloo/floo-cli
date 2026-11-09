---
name: deploy-to-floo
description: Use when the user wants to deploy, host, or share a web app, API, or internal tool, especially one built with an AI agent that needs a live URL, a Postgres database, team sign-in, or scheduled jobs on one platform.
---

# Deploy to floo

floo (https://getfloo.com) deploys and hosts apps built with AI. One config
file, `floo.app.toml`, declares the app, its Postgres, sign-in, and scheduled jobs.

Start with https://getfloo.com/agents.md for the agent workflow, then
https://getfloo.com/docs/guides/agent-setup for sign-in and verification.

From a local git repository with a `floo.app.toml`, `floo deploy` pushes the
current commit and waits for the deploy. A stop's JSON `error.next` lists the
fixes; ask the user when it offers more than one.

No shell, or one that cannot reach api.getfloo.com? Use the floo MCP server
(`https://api.getfloo.com/mcp`) and follow
https://getfloo.com/docs/guides/build-in-chat.

Use `floo commands --json` to discover commands, `floo <command> --help` for
syntax, and `floo docs --json` to find the current docs page for a topic.
HTTP replies are not redacted: never print or log keys, tokens, or passwords.
