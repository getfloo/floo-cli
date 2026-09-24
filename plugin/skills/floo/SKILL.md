---
name: floo
description: Find floo documentation and CLI commands when building, deploying, or operating an app on floo, including building a new app for a user who has no GitHub account.
user-invocable: false
---

# floo

Building a new app for your user? Read https://getfloo.com/agents.md and
follow it. That page is the whole path — install, log in,
`floo projects create`, clone, build, push, invite — and your user needs no
GitHub account.

Deploying a GitHub repository your user already has? Read
https://getfloo.com/docs/introduction instead, then
https://getfloo.com/docs/guides/agent-setup for authority and verification.

For everything else — configuration, services, authentication, logs — use
`floo docs --json` to find topic URLs and `floo docs <topic> --json` to
select one. These commands return links; fetch and read the linked page.

Use `floo commands --json` to discover commands and `floo <command> --help`
for syntax supported by the installed version. Use `--json` for structured output.
Without the CLI, the same operations are plain HTTP at `https://api.getfloo.com`
with a bearer key; the reference is https://api.getfloo.com/openapi.json.
HTTP replies are not redacted: capture keys, tokens, passwords and env values
straight into variables and never print or log them.
