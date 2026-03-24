use crate::output;

const OVERVIEW: &str = "\
Floo — Deploy web apps from the terminal.

Floo is a deployment platform. The CLI is the primary interface for deploying,
managing, and observing your apps. All source comes from GitHub — the CLI
never uploads code.

## Core Concepts

- **Apps** are the top-level unit. Each app has a unique name and URL.
- **Services** are deployable components inside an app (web servers, APIs, workers, databases).
- **Deploys** are immutable snapshots of your code, built into containers and deployed to the cloud.

## First Deploy

  1. `floo auth login` — authenticate
  2. `floo init <name>` — scaffold config files (local only)
  3. `floo apps github connect owner/repo` — connect to GitHub (triggers first deploy)
  4. `floo apps status <name>` — see your app's URL and status

  After the first deploy, use `floo deploy` for subsequent deploys.

## Learn More

  floo docs quickstart — end-to-end walkthrough
  floo docs services   — service types and managed services
  floo docs config     — config file formats with examples
  floo docs deploy     — detailed deploy flow and runtime detection
  floo docs auth       — add user authentication to your app
  floo --help          — all available commands
  floo <command> --help — details for a specific command
";

const QUICKSTART: &str = "\
Floo Quickstart — End-to-End Walkthrough

## 1. Install and Authenticate

  curl -fsSL https://getfloo.com/install.sh | bash
  floo auth login

## 2. Initialize Your Project

  cd my-project
  floo init my-app

  This creates config files (floo.service.toml or floo.app.toml) locally.
  No app is registered on the platform yet.

## 3. (Optional) Add Managed Services

  Edit floo.app.toml to add a database, cache, or storage:

  [postgres]
  tier = \"basic\"

  [redis]

  Managed services are auto-provisioned on the first deploy.
  Their credentials arrive as env vars (DATABASE_URL, REDIS_URL, STORAGE_BUCKET + STORAGE_URL).

## 4. Validate Config

  floo deploy --dry-run --json

  Checks config files, service graph, ports, and Dockerfiles locally — no
  auth or GitHub connection required. Fix any warnings before deploying.

## 5. Connect to GitHub and Deploy

  floo apps github connect owner/my-project

  This does three things:
  1. Creates the app on floo (if it doesn't exist)
  2. Connects your GitHub repo as the source
  3. Triggers the first deploy (source pulled from GitHub, built, deployed)

  Use --no-deploy to skip the automatic deploy.

## 6. Check Status

  floo apps status my-app
  floo logs --app my-app

## 7. Subsequent Deploys

  Push code to GitHub, then:

  floo deploy --app my-app

  Or set up git-push-deploy: floo apps github connect will auto-deploy
  on every push to your default branch.

## What Creates What

  floo init           — local config files only (no API call)
  floo deploy         — auto-creates the app if needed, then deploys
  floo apps github connect — creates app if needed, connects GitHub, triggers first deploy
  floo services add   — adds a user-managed service to config (NOT managed databases)

  Managed services (postgres, redis, storage) are declared in floo.app.toml
  and provisioned automatically on deploy.
";

const SERVICES: &str = "\
Floo Services

An app contains one or more services. Each service is independently deployable.

## User-Managed Services (your code)

  web     — HTTP server facing the internet (default for apps with a frontend)
  api     — HTTP server for backend APIs
  worker  — background process (no incoming HTTP traffic)

  These are deployed from source via `floo deploy`. Each has its own
  `floo.service.toml` with port, runtime, and ingress settings.

## Managed Services (provisioned by Floo)

  Declared in floo.app.toml, auto-provisioned on first deploy:

  postgres — managed PostgreSQL database
             Connection string injected as DATABASE_URL env var.

  redis    — managed Redis instance (Upstash, TLS-enabled)
             Connection string injected as REDIS_URL env var.

  storage  — managed object storage (GCS bucket)
             Bucket name injected as STORAGE_BUCKET + STORAGE_URL env vars.
             Use STORAGE_URL for signed URL requests (upload/download).

  Example floo.app.toml:

  [postgres]
  tier = \"basic\"

  [redis]

  [storage]

  Inspect with: floo services info <name> --app <app>

## Commands

  floo services list --app <name>            — list all services
  floo services info <service> --app <name>  — service details (connection info for managed)
  floo services add <name> <path>            — add a user-managed service to project config
  floo services rm <name>                    — remove a service from config

  Note: `floo services add` adds user-managed services (web/api/worker) to
  config. Managed databases are declared in floo.app.toml and provisioned
  automatically on deploy.
";

const CONFIG: &str = "\
Floo Config Files

## floo.service.toml — Single-Service Apps

  [app]
  name = \"my-app\"

  [service]
  name = \"web\"
  port = 3000
  type = \"web\"
  ingress = \"public\"
  env_file = \".env\"

## floo.app.toml — Multi-Service Apps

  [app]
  name = \"my-app\"

  [services.api]
  path = \"./api\"

  [services.web]
  path = \"./web\"

  Each service directory has its own floo.service.toml.

## Inline Multi-Service App (in floo.app.toml)

  [app]
  name = \"my-app\"

  [services.api]
  type = \"api\"
  path = \"./api\"
  port = 8080
  ingress = \"public\"

  [services.web]
  type = \"web\"
  path = \"./web\"
  port = 3000
  ingress = \"public\"

  When type and port are set inline, no per-service floo.service.toml is needed.

## Inline vs Delegated

  These modes are mutually exclusive per service. If a service has type and
  port inline in floo.app.toml, there must not be a floo.service.toml in
  that service's subdirectory. The CLI fails preflight if both are present.

## Managed Services (in floo.app.toml)

  [postgres]
  tier = \"basic\"

  [redis]

  [storage]

  Auto-provisioned on first deploy. Credentials injected as env vars.

## Resource Limits (optional, in floo.service.toml)

  [resources]
  cpu = \"1\"             # CPU cores (0.25 to 8)
  memory = \"512Mi\"      # Memory (128Mi to 32Gi)
  max_instances = 10    # Max autoscale instances

## Auth Section (in floo.app.toml)

  [auth]
  redirect_uris = [\"http://localhost:3000/callback\"]

  Required when access_mode = \"accounts\". Registers the OAuth callback
  URLs that your app will use. See: floo docs auth

## Environment Overrides (in floo.app.toml)

  [environments.dev]
  access_mode = \"public\"

  [environments.prod]
  access_mode = \"accounts\"

## Commands

  floo init <name>   — generate config files interactively
  floo deploy --dry-run  — validate config before deploying
";

const DEPLOY: &str = "\
Floo Deploy Flow

## How Deploys Work

  All source comes from GitHub. The CLI never uploads code.

  1. **Detect runtime** — CLI scans project files to determine language/framework
  2. **Create deploy** — CLI sends metadata to the API
  3. **Pull source** — API downloads source from your connected GitHub repo
  4. **Build** — builds container image via Cloud Build
  5. **Deploy** — deploys container to Cloud Run
  6. **URL** — returns the live URL for your app

## First Deploy vs Subsequent Deploys

  **First deploy:** Use `floo apps github connect owner/repo`. This connects
  GitHub and triggers the first deploy in one step. The app is auto-created
  if it doesn't exist.

  **Subsequent deploys:** Use `floo deploy`. GitHub must already be connected.

  **App creation:** `floo init` creates local config files only. The app is
  registered on the platform when you first deploy (either via `floo deploy`
  or `floo apps github connect`).

## Runtime Detection Priority

  Dockerfile       — highest priority (custom build)
  package.json     — Node.js (detects Express, Next.js, etc.)
  pyproject.toml   — Python (detects Django, Flask, FastAPI)
  requirements.txt — Python (fallback)
  go.mod           — Go
  index.html       — Static site (lowest priority)

## Deploy Options

  floo deploy [path]                — deploy from directory (default: current)
  floo deploy --app <name>         — deploy to existing app
  floo deploy --services <name>    — deploy specific services only
  floo deploy --restart            — restart without rebuilding
  floo deploy --sync-env           — re-sync env vars from env_file before deploy
  floo deploy --dry-run            — preview what would be deployed without deploying

## Deploy History

  floo deploy list --app <name>    — list past deploys
  floo deploy logs <id> --app <n>  — build logs for a specific deploy
  floo deploy watch --app <name>   — stream deploy progress in real-time
  floo deploy rollback <app> <id>  — rollback to a previous deploy
";

const AUTH: &str = "\
App Auth — Add User Authentication to Your App

Floo can manage user authentication for your deployed apps. When you set
access_mode = \"accounts\", floo provides a hosted OAuth flow powered by
WorkOS so your users can sign in with email, Google, GitHub, and more.

## What Happens When You Enable It

  1. Set access_mode and redirect URIs in floo.app.toml
  2. Deploy with `floo deploy`
  3. Floo automatically provisions the auth endpoints for your app

  No separate WorkOS account is needed — floo manages this for you.
  The auth endpoints are live as soon as the deploy completes.

## Setup

  1. Set access_mode in your floo.app.toml:

     [app]
     name = \"my-app\"
     access_mode = \"accounts\"

  2. Register your OAuth callback URLs:

     [auth]
     redirect_uris = [\"http://localhost:3000/callback\", \"https://my-app.com/callback\"]

  3. Deploy (first deploy: `floo apps github connect`, subsequent: `floo deploy`)

  4. Get your app ID (needed for the OAuth URLs below):

     floo apps list --json | jq '.data.apps[] | select(.name == \"my-app\") | .id'

## OAuth Flow

Your app integrates with these endpoints (BASE = https://api.getfloo.com):

  1. **Start login** — redirect users to:
     GET BASE/v1/auth/apps/{app_id}/authorize?redirect_uri=<your_callback_url>

  2. **Receive callback** — floo redirects back to your redirect_uri with a code:
     https://your-app.com/callback?code=<exchange_code>

  3. **Exchange code for tokens** — POST from your backend:
     POST BASE/v1/auth/apps/{app_id}/token
     Body: { \"grant_type\": \"authorization_code\", \"code\": \"<exchange_code>\" }
     Returns: { \"access_token\": \"<jwt>\", \"refresh_token\": \"<token>\", \"user\": {...} }

  4. **Verify user** — the access_token is an RS256 JWT. Verify locally with:
     GET BASE/v1/auth/apps/{app_id}/.well-known/jwks.json

  5. **Refresh tokens** — when the access_token expires:
     POST BASE/v1/auth/apps/{app_id}/token
     Body: { \"grant_type\": \"refresh\", \"refresh_token\": \"<token>\" }

  6. **Logout** — revoke the refresh token:
     POST BASE/v1/auth/apps/{app_id}/session/logout
     Body: { \"refresh_token\": \"<token>\" }

## Access Modes

  public    — no auth, anyone can access (default)
  password  — shared password for simple protection (Pro+)
  accounts  — per-user auth via floo's hosted OAuth (Pro+)
  sso       — enterprise SSO via SAML/OIDC (Enterprise)

## JWT Claims

  sub    — app user ID (UUID)
  email  — user's email address
  name   — user's display name
  iss    — https://auth.getfloo.com
  aud    — your app ID
  iat    — issued at timestamp
  exp    — expiration timestamp

## Convenience Endpoint

  GET BASE/v1/auth/apps/{app_id}/session/me
  Header: Authorization: Bearer <access_token>
  Returns the authenticated user's info without decoding the JWT yourself.
";

const TOPICS: &[(&str, &str)] = &[
    ("quickstart", QUICKSTART),
    ("services", SERVICES),
    ("config", CONFIG),
    ("deploy", DEPLOY),
    ("auth", AUTH),
];

pub fn docs(topic: Option<&str>) {
    let (topic_name, content) = match topic {
        None => ("overview", OVERVIEW),
        Some(t) => match TOPICS.iter().find(|(name, _)| *name == t) {
            Some((name, content)) => (*name, *content),
            None => {
                let available: Vec<&str> = TOPICS.iter().map(|(n, _)| *n).collect();
                output::error(
                    &format!("Unknown docs topic: '{t}'."),
                    &crate::errors::ErrorCode::InvalidFormat,
                    Some(&format!("Available topics: {}", available.join(", "))),
                );
                std::process::exit(1);
            }
        },
    };

    if output::is_json_mode() {
        output::success(
            &format!("docs:{topic_name}"),
            Some(serde_json::json!({
                "topic": topic_name,
                "content": content.trim(),
            })),
        );
    } else {
        eprintln!("{}", content.trim());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_docs_content_not_empty() {
        assert!(!OVERVIEW.is_empty());
        assert!(!QUICKSTART.is_empty());
        assert!(!SERVICES.is_empty());
        assert!(!CONFIG.is_empty());
        assert!(!DEPLOY.is_empty());
        assert!(!AUTH.is_empty());
    }

    #[test]
    fn test_overview_has_key_concepts() {
        assert!(OVERVIEW.contains("Apps"));
        assert!(OVERVIEW.contains("Services"));
        assert!(OVERVIEW.contains("github connect"));
    }

    #[test]
    fn test_auth_docs_has_key_concepts() {
        assert!(AUTH.contains("redirect_uris"));
        assert!(AUTH.contains("authorize"));
        assert!(AUTH.contains("access_token"));
        assert!(AUTH.contains("accounts"));
        assert!(AUTH.contains("app_id"));
    }

    #[test]
    fn test_quickstart_has_full_flow() {
        assert!(QUICKSTART.contains("auth login"));
        assert!(QUICKSTART.contains("init"));
        assert!(QUICKSTART.contains("github connect"));
        assert!(QUICKSTART.contains("apps status"));
    }

    #[test]
    fn test_services_no_coming_soon() {
        assert!(!SERVICES.contains("coming soon"));
    }

    #[test]
    fn test_deploy_mentions_github() {
        assert!(DEPLOY.contains("GitHub"));
        assert!(!DEPLOY.contains("archive"));
    }
}
