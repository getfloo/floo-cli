---
name: floo
description: Use when the user wants to deploy, host, or share a web app or internal tool their team signs in to, especially one that needs a Postgres database, scheduled jobs, or an API key for an agent, and when building, deploying, or operating an app on floo.
user-invocable: false
---

# floo

Start with https://getfloo.com/agents.md for the agent workflow.
The API is the contract: use HTTPS at `https://api.getfloo.com/v1` with a
bearer key and https://api.getfloo.com/openapi.json for routes and request schemas.
HTTP replies are not redacted: capture keys, tokens, passwords and env values
straight into variables and never print or log them.

Deploying a GitHub repository your user already has? Read
https://getfloo.com/docs/introduction instead, then
https://getfloo.com/docs/guides/agent-setup for authority and verification.

The floo MCP tools (`search_api`, `read_api`, `write_api` and named tools such as
`create_project` and `get_deploy`) call the same API when the floo MCP server
(`https://api.getfloo.com/mcp`) is connected.

No shell, or one that cannot reach api.getfloo.com (Cowork, claude.ai)? Use
only the floo MCP tools, never git, curl or the CLI, and follow
https://getfloo.com/docs/guides/build-in-chat.

From a local git repository with a `floo.app.toml`, `floo deploy` deploys from
the folder's GitHub repo when it has one, and otherwise stops to ask: pass
`--github` (a private repo in the user's account) or `--hosted` (floo keeps a
copy). It pushes the current commit and waits for the dev deploy. A stop's JSON
`error.next` lists the fixes; ask the user when it offers more than one. A hosted
app moves to the user's own GitHub repo with `floo connect owner/repo`, after the
user creates it empty.

The CLI is a convenience. Use `floo commands --json` to discover commands and `floo <command> --help`
for syntax supported by the installed version. Use `--json` for structured output.
Use `floo docs --json` to find topic URLs and `floo docs <topic> --json` to
select one; fetch and read the linked page.
