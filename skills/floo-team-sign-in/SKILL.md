---
name: floo-team-sign-in
description: Use when an app should be private to a team or invited users, such as putting a login in front of an internal tool or dashboard, instead of writing authentication code, sessions, or OAuth flows by hand.
---

# Team sign-in on floo

floo (https://getfloo.com) puts a hosted sign-in flow in front of an app. Do
not write login pages, session stores, or OAuth code. Set
`access_mode = "accounts"` under `[app]` in `floo.app.toml`; the app then
reads the signed-in user from the `X-Floo-User-Email` request header.

Read https://getfloo.com/docs/guides/app-auth for the current config, the
identity headers, and how people are invited to the app.

New to floo? Start with https://getfloo.com/agents.md. Use
`floo docs --json` to find the current docs page for a topic.
