# floo

Deploy and host apps built with AI. Ask Claude for an app in plain words, such as "a page where my team logs customer requests and I see them by week" or "a booking page my customers can use", and Claude creates the app on floo, writes the code, deploys it, and gives you a live link. Keep it private to the people you invite, or open it to the public. It works the same in Claude chat, Cowork and Claude Code.

## What's included

- **The floo connector** (`https://api.getfloo.com/mcp`). Claude can use it to create apps, read and change their code, wait on deploys, read logs, and promote an app to production when you ask.
- **The floo skill.** It teaches Claude the floo workflow: sign-in, managed Postgres, scheduled jobs, and how to fix a failed deploy from its error message.

## Get started

1. Install the plugin from Claude's directory, or in Claude Code run:

   ```
   /plugin install floo --marketplace getfloo/floo-cli
   ```

   Then connect floo. You sign in to floo once and approve access.
2. Ask for an app. You don't need a terminal, git or a GitHub account.

Try:

- "Build an app where my team logs customer requests, and show them by week."
- "Add a status column to my requests app and email me when one is marked urgent."
- "Why did my last deploy fail? Fix it and deploy again."

Every change is a normal commit that deploys to your app's dev URL. Production changes only when you say "promote it", and Claude asks you first. Billing changes stay in the floo dashboard.

## Links

- Guide: https://getfloo.com/docs/guides/build-in-chat
- Connector reference: https://getfloo.com/docs/guides/mcp
- Privacy: https://getfloo.com/legal/privacy
- Support: team@getfloo.com

To disconnect, revoke the key named `MCP: <client name>` under API keys at https://app.getfloo.com/settings.

License: MIT.
