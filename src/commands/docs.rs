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
  2. `floo check` — validate config before deploying
  3. `floo deploy` — detect runtime, archive source, upload, build, deploy
  4. `floo apps status <name>` — see your app's URL and status

## Learn More

  floo docs services   — service types and how they work
  floo docs config     — config file formats with examples
  floo docs deploy     — detailed deploy flow and runtime detection
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

## Environment Overrides (in floo.app.toml)

  [environments.dev]
  access_mode = \"public\"

  [environments.prod]
  access_mode = \"password\"

## Commands

  floo init <name>   — generate config files interactively
  floo check         — validate config before deploying
";

const DEPLOY: &str = "\
Floo Deploy Flow

## What Happens When You Run `floo deploy`

  1. **Detect runtime** — scans project files to determine language/framework
  2. **Archive source** — creates .tar.gz of project (respects .flooignore)
  3. **Upload** — sends archive to Floo API
  4. **Build** — builds container image from source
  5. **Deploy** — deploys container to cloud infrastructure
  6. **URL** — returns the live URL for your app

## Runtime Detection Priority

  Dockerfile       — highest priority (custom build)
  package.json     — Node.js (detects Express, Next.js, etc.)
  pyproject.toml   — Python (detects Django, Flask, FastAPI)
  requirements.txt — Python (fallback)
  go.mod           — Go
  index.html       — Static site (lowest priority)

## .flooignore

  Works like .gitignore. Add patterns to exclude files from the archive:

    node_modules/
    .git/
    *.log
    .env

  Max archive size: 500MB.

## Deploy Options

  floo deploy [path]                — deploy from directory (default: current)
  floo deploy --app <name>         — deploy to existing app
  floo deploy --services <name>    — deploy specific services only
  floo deploy --restart            — restart without re-uploading source
  floo deploy --sync-env           — re-sync env vars from env_file before deploy
  floo deploy --dry-run            — preview what would be deployed without deploying

## Deploy History

  floo deploy list --app <name>    — list past deploys
  floo deploy logs <id> --app <n>  — build logs for a specific deploy
  floo deploy watch --app <name>   — stream deploy progress in real-time
  floo deploy rollback <app> <id>  — rollback to a previous deploy
";

pub fn docs(topic: Option<&str>) {
    let (topic_name, content) = match topic {
        None => ("overview", OVERVIEW),
        Some("services") => ("services", SERVICES),
        Some("config") => ("config", CONFIG),
        Some("deploy") => ("deploy", DEPLOY),
        Some(other) => {
            output::error(
                &format!("Unknown docs topic: '{other}'."),
                &crate::errors::ErrorCode::InvalidFormat,
                Some("Available topics: services, config, deploy"),
            );
            std::process::exit(1);
        }
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
    }

    #[test]
    fn test_overview_has_key_concepts() {
        assert!(OVERVIEW.contains("Apps"));
        assert!(OVERVIEW.contains("Services"));
        assert!(OVERVIEW.contains("Deploy Flow"));
    }
}
