floo - Build a Rails app on floo

End-to-end Rails journey: deploy, add Postgres, add per-user auth, add a
custom domain. Every step has runnable Ruby code.

## 1. Add a Dockerfile

Rails 7.1+ ships a production Dockerfile from `rails new`. Otherwise:

  bin/rails generate dockerfile

Bind to 0.0.0.0 (not localhost). Cloud Run only routes traffic to
processes bound to all interfaces. Expose the same port you set in
floo.app.toml (3000 below).

## 2. Initialize floo config

  floo init my-rails-app

Resulting floo.app.toml:

  [app]
  name = "my-rails-app"

  [services.web]
  type = "web"
  path = "."
  port = 3000
  ingress = "public"
  dev_command = "bin/rails server -p 3000"
  migrate_command = "bin/rails db:migrate"

migrate_command runs after every deploy (against dev) and after every
promote (against prod). Rails migrations stay in sync with deploys.

## 3. Connect repo and deploy

  git add . && git commit -m "feat: floo config"
  git push origin main
  floo apps github connect owner/my-rails-app
  floo deploys watch --app my-rails-app

App is live at https://my-rails-app-dev.on.getfloo.com.

## 4. Local dev and one-shot commands

Two commands cover the daily Rails workflow once your first deploy is up.

Local dev server with prod-shaped env:

  floo dev --app my-rails-app

Runs your dev_command locally with DATABASE_URL and other env vars
sourced from floo. Real Cloud SQL connection, no exported credentials.

Add --fixture-user to test signed-in (accounts-mode) flows locally:

  floo dev --app my-rails-app --fixture-user you@example.com

The proxy injects the same X-Floo-User-* headers floo's gateway adds in
production, so the controller reading those headers works locally with
no conditional code.

One-shot commands (rake tasks, db:seed, console):

  floo run --service web -- bundle exec rake my_task
  floo run --service web -- bin/rails db:seed
  floo run --service web -- bin/rails console
  floo run --service web -- bin/rails db:migrate

floo run inherits stdin/stdout/stderr, so interactive commands like
bin/rails console work like running them locally - your shell just sees
the floo-injected env vars instead of your local .env. Migrations run
automatically on every deploy via migrate_command; use `floo run --
bin/rails db:migrate` only for ad-hoc migration work outside the deploy
path.

## 5. Add Postgres

  [managed.primary]
  type = "postgres"
  tier = "basic"

  git add floo.app.toml && git commit -m "feat: add postgres"
  git push origin main

Rails reads DATABASE_URL automatically. floo injects a normal PostgreSQL
URI, so ActiveRecord can parse it without custom Cloud SQL socket code.
`floo preflight` warns if a local env file still contains the old Cloud
SQL socket-style DATABASE_URL that Ruby's URI parser rejects.
Confirm config/database.yml has:

  production:
    primary:
      url: <%= ENV["DATABASE_URL"] %>

## 6. Add per-user auth

floo manages user authentication. Set access_mode = "accounts" in floo.app.toml - that is the entire auth config:

  [app]
  name = "my-rails-app"
  access_mode = "accounts"

Push, deploy. Gateway sits in front of your app, redirects unauth'd users
to a hosted login, validates session on every request, injects
identity headers. Your Rails controllers read them:

  class ApplicationController < ActionController::Base
    before_action :load_floo_user

    private

    def load_floo_user
      @current_user_email = request.headers["X-Floo-User-Email"]
      @current_user_id    = request.headers["X-Floo-User-Id"]
      @current_user_name  = request.headers["X-Floo-User-Name"]
    end
  end

For local development, run `floo dev --fixture-user` (section 4) - same
identity headers in front of the local server, no conditional code.

## 7. Add a custom domain

  floo domains add app.example.com --app my-rails-app

Add the traffic and `_floo-verify` TXT records shown in the output, then run
`floo domains verify app.example.com --app my-rails-app`.

## Common gotchas

  - /healthz is reserved by Cloud Run - use /health or /livez
  - bind: 0.0.0.0 (Rails defaults vary)
  - asset compilation runs in the Dockerfile (RAILS_SERVE_STATIC_FILES=1)
  - Rails 7+ force_ssl works correctly behind floo's edge (X-Forwarded-Proto)

Full guide with complete Ruby code: https://getfloo.com/docs/build/rails
