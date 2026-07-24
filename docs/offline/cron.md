floo Cron Jobs

Cron jobs are declared in floo.app.toml - never created by the CLI. Each
[cron.<name>] section becomes a managed cron job, reconciled on every
deploy (added, updated, or removed to match config).

## Declare in floo.app.toml

  [cron.daily-report]
  schedule = "0 9 * * *"                  # 9am UTC daily
  command  = "python -m reports.daily"
  service  = "api"                         # which service's image to run in

  [cron.cleanup]
  schedule = "*/5 * * * *"                 # every 5 minutes
  command  = "node scripts/cleanup.js"
  service  = "worker"
  timeout  = 600                            # max seconds (default 300, optional)

## Fields

  schedule  required  Standard cron expression in UTC.
  command   required  Shell command executed inside the target service's container.
  service   required  Name of a [services.<name>] entry; that image is reused.
  timeout   optional  Max execution time in seconds. Default 300.

## Common schedules

  * * * * *     every minute
  */5 * * * *   every 5 minutes
  0 * * * *     every hour
  0 9 * * *     daily at 9am UTC
  0 9 * * 1-5   weekdays at 9am UTC
  0 0 * * 0     weekly on Sunday at midnight UTC
  0 0 1 * *     monthly on the 1st at midnight UTC

## Deploy and verify

  Push to GitHub or run `floo redeploy`. New jobs are created, changed jobs
  updated, removed jobs deleted - all on the deploy itself.

    git push origin main && floo deploys watch --app my-app
    floo cron list --app my-app                # see jobs + last run status

## Manually trigger a job (off-schedule)

  Useful for testing or one-off catch-up runs:

    floo cron run daily-report --app my-app
    floo cron run daily-report --app my-app --preflight   # preview, no API call

## CLI surface

  The `floo cron` CLI is read-only + manual trigger. Schedules and commands
  are config-driven; the CLI never adds, removes, or edits them.

    floo cron list --app <name>            list jobs and last run status
    floo cron show <name> --app <app>      details for one job
    floo cron run <name> --app <app>       trigger a job off-schedule

## Environment

  Jobs run inside the specified service's container image with the same
  env vars as the service - same DATABASE_URL, REDIS_URL, secrets, etc.

## Long-form guide

  https://getfloo.com/docs/guides/cron-jobs   - examples, agent workflow, troubleshooting
  https://getfloo.com/docs/reference/config-spec   - full [cron.<name>] schema reference
