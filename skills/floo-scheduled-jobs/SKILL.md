---
name: floo-scheduled-jobs
description: Use when an app needs a scheduled or recurring task, such as a nightly sync, a report email, or a cleanup job, running next to the deployed app without a separate scheduler service.
---

# Scheduled jobs on floo

floo (https://getfloo.com) runs shell commands on a schedule, in UTC, inside
the app's own container image. Declare `[cron.<name>]` sections in
`floo.app.toml`; every deploy syncs them.

Read https://getfloo.com/docs/guides/cron-jobs for the current config.

New to floo? Start with https://getfloo.com/agents.md. Use
`floo docs --json` to find the current docs page for a topic.
