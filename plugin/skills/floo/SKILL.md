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

The CLI is a convenience. Use `floo commands --json` to discover commands and `floo <command> --help`
for syntax supported by the installed version. Use `--json` for structured output.
Use `floo docs --json` to find topic URLs and `floo docs <topic> --json` to
select one; fetch and read the linked page.
