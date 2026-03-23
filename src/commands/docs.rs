use crate::output;

const OVERVIEW: &str = "\
Floo — Deploy web apps from the terminal.

Floo is a deployment platform. The CLI is the primary interface for deploying,
managing, and observing your apps.

## Core Concepts

- **Apps** are the top-level unit. Each app has a unique name and URL.
- **Services** are deployable components inside an app (web servers, APIs, workers, databases).
- **Deploys** are immutable snapshots of your code, built into containers and deployed to the cloud.

## Deploy Flow

  1. `floo init <name>` — scaffold config files for your project
  2. `floo deploy --dry-run` — validate config before deploying
  3. `floo deploy` — detect runtime, trigger build via GitHub, deploy
  4. `floo apps status <name>` — see your app's URL and status

## Learn More

  floo docs services   — service types and how they work
  floo docs config     — config file formats with examples
  floo docs deploy     — detailed deploy flow and runtime detection
  floo docs auth       — add user authentication to your app
  floo --help          — all available commands
  floo <command> --help — details for a specific command
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

  postgres — managed PostgreSQL database
             Connection string injected as DATABASE_URL env var.
             Inspect with: floo services info <name> --app <app>

  redis    — managed Redis instance (coming soon)

## Commands

  floo services list --app <name>            — list all services
  floo services info <service> --app <name>  — service details (connection info for managed)
  floo services add <name> <path>            — add a service to project config
  floo services rm <name>                    — remove a service from config
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

## What Happens When You Run `floo deploy`

  1. **Detect runtime** — scans project files to determine language/framework
  2. **Create deploy** — sends deploy request to Floo API (source pulled from GitHub)
  3. **Build** — builds container image from source
  4. **Deploy** — deploys container to cloud infrastructure
  5. **URL** — returns the live URL for your app

  Your app must be connected to GitHub (`floo apps github connect <repo>`) so the
  API can pull source code directly.

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

## Setup

  1. Set access_mode in your floo.app.toml:

     [app]
     name = \"my-app\"
     access_mode = \"accounts\"

  2. Register your OAuth callback URLs:

     [auth]
     redirect_uris = [\"http://localhost:3000/callback\", \"https://my-app.com/callback\"]

  3. Deploy: `floo deploy`

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
        assert!(!SERVICES.is_empty());
        assert!(!CONFIG.is_empty());
        assert!(!DEPLOY.is_empty());
        assert!(!AUTH.is_empty());
    }

    #[test]
    fn test_overview_has_key_concepts() {
        assert!(OVERVIEW.contains("Apps"));
        assert!(OVERVIEW.contains("Services"));
        assert!(OVERVIEW.contains("Deploy Flow"));
    }

    #[test]
    fn test_auth_docs_has_key_concepts() {
        assert!(AUTH.contains("redirect_uris"));
        assert!(AUTH.contains("authorize"));
        assert!(AUTH.contains("access_token"));
        assert!(AUTH.contains("accounts"));
    }
}
