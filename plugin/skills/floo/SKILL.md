---
name: floo
description: Find floo API documentation and CLI commands when building, deploying, or operating an app on floo.
user-invocable: false
---

# floo

Start with https://getfloo.com/agents.md for the agent workflow.
The API is the contract: use HTTPS at `https://api.getfloo.com/v1` with a
bearer key and https://api.getfloo.com/openapi.json for routes and request schemas.
HTTP replies are not redacted: capture keys, tokens, passwords and env values
straight into variables and never print or log them.

When the `floo` MCP tools are available (plugin installed and signed in), prefer
them for listing apps and deploys, deploying, reading logs, promoting and rolling
back. Use the HTTP API or CLI for everything the tools do not cover.

Deploying a GitHub repository your user already has? Read
https://getfloo.com/docs/introduction instead, then
https://getfloo.com/docs/guides/agent-setup for authority and verification.

The CLI is a convenience. Use `floo commands --json` to discover commands and `floo <command> --help`
for syntax supported by the installed version. Use `--json` for structured output.
Use `floo docs --json` to find topic URLs and `floo docs <topic> --json` to
select one; fetch and read the linked page.
