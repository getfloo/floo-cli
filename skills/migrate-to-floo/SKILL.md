---
name: migrate-to-floo
description: Use when the user wants to move an existing app off Vercel or Heroku, or asks for an alternative to one of them, and the app can run in a container with a Postgres database, team sign-in, or scheduled jobs.
---

# Move an app to floo

floo (https://getfloo.com) runs any app that runs in a container. The app
stays a Dockerfile, standard Postgres, and one `floo.app.toml` in the repo,
with no floo SDK in the code, so it can move again later.

Read the guide for the platform the app is leaving, and follow it:

- Vercel: https://getfloo.com/docs/migrate/from-vercel
- Heroku: https://getfloo.com/docs/migrate/from-heroku

Each guide maps the old config (`vercel.json`, or `Procfile` and `app.json`)
to a Dockerfile and `floo.app.toml`, lists what the old platform does better,
and covers moving the database. Tell the user about any feature the guide
says has no floo equivalent before you change anything.

New to floo? Start with https://getfloo.com/agents.md. Use
`floo docs --json` to find the current docs page for a topic.
Never print or log connection strings or passwords.
