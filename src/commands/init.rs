use std::collections::HashMap;
use std::path::PathBuf;
use std::process;

use crate::detection::{detect, DetectionResult};
use crate::dockerfile;
use crate::errors::ErrorCode;
use crate::names::generate_name;
use crate::output;
use crate::project_config::{self, AppFileAppSection, AppFileConfig, AppServiceEntry, ServiceType};

/// Header comment block written above `[app]` on `floo init`.
///
/// This is the lever that closes two recurring friction points:
/// `88e32b22` (access_mode placement is non-obvious) and `c9b70eb5`
/// (no in-CLI signal that pushing to GitHub auto-deploys to dev). Both
/// were reported on `floo-artifact` 2026-05-01 — the user only learned
/// either fact by reading the hosted docs. Putting the answer in the
/// file the user is about to edit (and in the file every coding agent
/// reads when it lands in this repo) makes the discovery cost zero.
///
/// Keep this short — a wall of comments at the top of every config
/// becomes noise. Anchor to canonical doc URLs for depth.
const APP_TOML_HEADER: &str = r#"# floo.app.toml — see https://getfloo.com/docs/reference/config-spec.md
#
# Deploys happen on `git push`. After `floo apps github connect`, every
# push to your default branch builds and deploys to the dev environment
# automatically (no `floo deploy` needed). Cutting a GitHub release
# promotes that build to production. See
# https://getfloo.com/docs/guides/golden-path.md.
#
# Common knobs to add when you need them (under [app], applies to every env):
#   access_mode = "accounts"   # require sign-in (Pro+) — public, password,
#                              #   accounts, or sso. The [app] level is the
#                              #   one that actually applies on push deploys
#                              #   today; per-env overrides via
#                              #   [environments.<name>] are documented but
#                              #   not yet applied server-side.
#
# Run `floo docs config` for the full schema."#;

pub fn init(name: Option<String>, path: PathBuf) {
    let project_path = match path.canonicalize() {
        Ok(p) => p,
        Err(_) => {
            output::error(
                &format!("Path '{}' does not exist.", path.display()),
                &ErrorCode::InvalidPath,
                Some("Provide a valid project directory."),
            );
            process::exit(1);
        }
    };

    if !project_path.is_dir() {
        output::error(
            &format!("Path '{}' is not a directory.", path.display()),
            &ErrorCode::InvalidPath,
            Some("Provide a valid project directory."),
        );
        process::exit(1);
    }

    // Error if config already exists
    if project_path.join(project_config::APP_CONFIG_FILE).exists() {
        output::error(
            &format!("{} already exists.", project_config::APP_CONFIG_FILE),
            &ErrorCode::ConfigExists,
            Some("Edit floo.app.toml directly to add services."),
        );
        process::exit(1);
    }

    let detection = detect(&project_path);

    if output::is_interactive() {
        init_interactive(&project_path, name, &detection);
    } else {
        init_non_interactive(&project_path, name, &detection);
    }
}

/// Agent-safe operating notes scaffold. Written next to floo.app.toml on
/// every `floo init` so any AI coding assistant working in the project
/// has the floo-specific gotchas at hand without needing to crawl docs.
///
/// The content mirrors the public app-auth checklist on getfloo.com so
/// the same advice is available to a coding agent reading local files
/// AND to a human reading the docs site. Closes feedback 9494ea44
/// (floo-artifact, 2026-04-30): "Floo should publish an agent-safe
/// checklist/template for apps using accounts mode... could live in
/// docs and/or be generated into AGENTS.md by floo init."
const AGENTS_MD_TEMPLATE: &str = r#"# Agent operating notes

This file is for AI coding assistants working in this floo app. It
captures the floo-specific gotchas that aren't obvious from the code.

## Working with floo

- **Run `floo preflight` before every deploy.** Catches config drift,
  missing managed-service env vars, runtime detection issues, and
  destructive plan changes — most deploy failures are preventable here.
- **Read the latest docs at `https://getfloo.com/docs/*.md`.** The
  Markdown URLs are agent-friendly. `floo docs <topic>` also prints
  cheatsheets locally.
- **Deploy by pushing to GitHub.** `git push` to your default branch
  triggers a dev deploy; cutting a GitHub release promotes to prod.
  Don't shell out to `gcloud run deploy` — it bypasses the floo
  pipeline.

## Agent-safe deploy debugging

- **Use `floo deploys status --json` instead of `floo deploys watch`.**
  `status` returns a compact summary (deploy id, derived phase
  booleans, gateway URL, next recommended command) without dumping
  build logs that may contain audit payloads. `watch` is fine for
  humans; for scripts and agents, prefer `status`.
- **`/health` on the direct Cloud Run URL is for infrastructure
  probes, not authenticated requests.** Cloud Run liveness/startup
  probes hit it without any session, so don't infer auth state from
  whatever can reach `/health`.
- **Test the floo gateway URL after every deploy.** A 502 on
  `*.on.getfloo.com` (or your custom domain) means the deploy didn't
  finalize even if the direct Cloud Run URL serves new code. `floo
  deploys status --json` reports `host_bound: false` in that state.

## If you set `access_mode = "accounts"` (or `"password"`)

- **Trust `X-Floo-User-Email`, `X-Floo-User-Id`, `X-Floo-User-Name`,
  and `X-Floo-User-Role` on every request your app receives.** The
  floo gateway is the only path into your container — Cloud Run
  ingress is locked to `INGRESS_TRAFFIC_INTERNAL_LOAD_BALANCER` and
  there's no `allUsers` invoker grant. The deploy pipeline raises if
  that combination would somehow ship as `INGRESS_TRAFFIC_ALL`.
- **Don't curl your `*.run.app` URL in scripts or tests.** It
  returns 403 from Cloud Run before reaching your container — by
  design.
- **Don't accept identity headers from any other path.** The trust
  boundary is the gateway, not the network in general. Authenticate
  inter-service or internal-cron calls separately.
- **Use `floo dev --fixture-user` for local dev.** It injects the
  same headers the gateway would, so your code path stays the same
  with no auth-mode toggling.

The full background — how the deploy-time invariant works, what
exactly is enforced, and how to verify the boundary — lives at
`https://getfloo.com/docs/guides/app-auth.md`.
"#;

fn write_agents_md(project_path: &std::path::Path) -> bool {
    let agents_path = project_path.join("AGENTS.md");
    if agents_path.exists() {
        // Don't clobber an existing AGENTS.md — it may have project-
        // specific notes the agent or operator wrote by hand.
        return false;
    }
    if let Err(e) = std::fs::write(&agents_path, AGENTS_MD_TEMPLATE) {
        output::error(
            &format!("Failed to write AGENTS.md: {e}"),
            &ErrorCode::FileError,
            None,
        );
        process::exit(1);
    }
    true
}

/// Attempt to generate a Dockerfile based on detection results.
/// Returns true if a Dockerfile was written.
fn generate_dockerfile_if_needed(
    project_path: &std::path::Path,
    detection: &DetectionResult,
) -> bool {
    // Skip if Dockerfile already exists (detection returns "docker" runtime)
    if detection.runtime == "docker" {
        return false;
    }

    let should_auto_generate = detection.confidence == "high" || detection.confidence == "medium";

    if !should_auto_generate {
        // Low confidence: prompt in interactive mode, skip in non-interactive
        if output::is_interactive() {
            if !output::confirm("No Dockerfile found. Generate one?") {
                return false;
            }
        } else {
            return false;
        }
    }

    let content = match dockerfile::generate_dockerfile(detection, project_path) {
        Some(c) => c,
        None => return false,
    };

    let dockerfile_path = project_path.join("Dockerfile");
    if let Err(e) = std::fs::write(&dockerfile_path, &content) {
        output::error(
            &format!("Failed to write Dockerfile: {e}"),
            &ErrorCode::FileError,
            None,
        );
        process::exit(1);
    }

    let framework_label = detection.framework.as_deref().unwrap_or(&detection.runtime);
    let version_label = detection
        .version
        .as_deref()
        .map(|v| format!(" {v}"))
        .unwrap_or_default();

    if !output::is_json_mode() {
        output::info(
            &format!(
                "Created Dockerfile for {framework_label} ({}{version_label})",
                detection.runtime
            ),
            None,
        );
    }

    true
}

fn init_non_interactive(
    project_path: &std::path::Path,
    name: Option<String>,
    detection: &crate::detection::DetectionResult,
) {
    let app_name = match name {
        Some(n) => n,
        None => {
            output::error(
                "App name required in non-interactive mode.",
                &ErrorCode::MissingAppName,
                Some("Usage: floo init <name> [--json]"),
            );
            process::exit(1);
        }
    };

    let default_type = detection.default_service_type();
    let service_type = match default_type {
        "api" => ServiceType::Api,
        _ => ServiceType::Web,
    };

    let env_file = super::detect_env_file(project_path);
    let service_name = default_type.to_string();
    let port = detection.default_port();

    // Generate Dockerfile if none exists
    let dockerfile_generated = generate_dockerfile_if_needed(project_path, detection);

    // Write a single floo.app.toml with the service inline.
    // Agents read 'floo docs config' to understand the schema and customize it.
    let mut services = HashMap::new();
    services.insert(
        service_name.clone(),
        AppServiceEntry::scaffold(service_type.into(), ".", port, env_file),
    );

    let app_file = AppFileConfig {
        app: AppFileAppSection {
            name: app_name.clone(),
            access_mode: None,
            agent_mode: None,
        },
        auth: None,
        github: None,
        postgres: None,
        redis: None,
        storage: None,
        resources: None,
        reparo: None,
        cron: HashMap::new(),
        services,
        environments: HashMap::new(),
    };

    let mut files_written = Vec::new();

    if let Err(e) =
        project_config::write_app_config_with_header(project_path, &app_file, APP_TOML_HEADER)
    {
        output::error(&e.message, &e.code, None);
        process::exit(1);
    }
    files_written.push(project_config::APP_CONFIG_FILE);

    if dockerfile_generated {
        files_written.push("Dockerfile");
    }

    let agents_md_written = write_agents_md(project_path);
    if agents_md_written {
        files_written.push("AGENTS.md");
    }

    let mut json_data = serde_json::json!({
        "app_name": app_name,
        "files_written": files_written,
        "detection": detection.to_value(),
        "service": {
            "name": service_name,
            "type": service_type.to_string(),
            "port": port,
        },
        "dockerfile_generated": dockerfile_generated,
        "agents_md_written": agents_md_written,
        "hint": "Edit floo.app.toml to configure your services — run 'floo docs config' for the schema",
        "next_step": "Connect GitHub with 'floo apps github connect <owner/repo>'. Every git push to your default branch then deploys to dev automatically.",
    });

    // Add suggestion when Dockerfile was not generated due to low confidence
    if !dockerfile_generated && detection.runtime != "docker" {
        json_data["suggestion"] = serde_json::json!(
            "No runtime detected with sufficient confidence. Add a Dockerfile manually."
        );
    }

    if !output::is_json_mode() {
        eprintln!("  Next: 'floo apps github connect <owner/repo>' wires the repo so");
        eprintln!("        every 'git push' to your default branch deploys to dev.");
        eprintln!("  Edit floo.app.toml to configure services; run 'floo preflight' to validate.");
    }
    output::success(&format!("Initialized app '{app_name}'."), Some(json_data));
}

fn init_interactive(
    project_path: &std::path::Path,
    name: Option<String>,
    detection: &crate::detection::DetectionResult,
) {
    // Show detection info
    if detection.runtime != "unknown" {
        let framework_label = detection
            .framework
            .as_deref()
            .map(|f| format!(" ({f})"))
            .unwrap_or_default();
        output::info(
            &format!(
                "Detected {}{framework_label} \u{2014} {} confidence",
                detection.runtime, detection.confidence
            ),
            None,
        );
    }

    // Generate Dockerfile if none exists
    generate_dockerfile_if_needed(project_path, detection);

    let default_name = name.unwrap_or_else(generate_name);
    let app_name = output::prompt_with_default("App name", &default_name);

    let mut services_map: HashMap<String, AppServiceEntry> = HashMap::new();

    // First service prompt
    if output::confirm("Add a service?") {
        loop {
            let default_svc_name = detection.default_service_type().to_string();
            let svc_name = output::prompt_with_default("Service name", &default_svc_name);

            let default_path = ".".to_string();
            let svc_path = output::prompt_with_default("Service path", &default_path);

            let default_port = detection.default_port().to_string();
            let port_str = output::prompt_with_default("Port", &default_port);
            let port: u16 = port_str.parse().unwrap_or_else(|_| {
                output::error(
                    &format!("Invalid port number: '{port_str}'."),
                    &ErrorCode::InvalidFormat,
                    Some("Port must be a number between 1 and 65535."),
                );
                process::exit(1);
            });

            let default_type = detection.default_service_type();
            let type_str = output::prompt_with_default("Type (web/api/worker)", default_type);
            let service_type = match type_str.as_str() {
                "web" => ServiceType::Web,
                "api" => ServiceType::Api,
                "worker" => ServiceType::Worker,
                other => {
                    output::error(
                        &format!("Unknown service type '{other}'."),
                        &ErrorCode::InvalidType,
                        Some("Valid types: web, api, worker."),
                    );
                    process::exit(1);
                }
            };

            let svc_dir = project_path.join(&svc_path);
            let env_file = match super::detect_env_file(&svc_dir) {
                Some(name) => {
                    let use_it =
                        output::confirm(&format!("  {name} detected. Use it for cloud deploy?"));
                    if use_it {
                        Some(name)
                    } else {
                        None
                    }
                }
                None => None,
            };

            let path_str = if svc_path == "." {
                ".".to_string()
            } else {
                format!("./{svc_path}")
            };
            services_map.insert(
                svc_name.clone(),
                AppServiceEntry::scaffold(service_type.into(), path_str, port, env_file),
            );

            if !output::confirm("Add another service?") {
                break;
            }
        }
    } else {
        // No explicit service — add a default inline entry so floo.app.toml is valid.
        // The agent edits it after reading 'floo docs config'.
        let default_type = detection.default_service_type();
        let service_type = match default_type {
            "api" => ServiceType::Api,
            _ => ServiceType::Web,
        };
        let env_file = super::detect_env_file(project_path);
        services_map.insert(
            default_type.to_string(),
            AppServiceEntry::scaffold(service_type.into(), ".", detection.default_port(), env_file),
        );
    }

    // Write app config
    let app_file = AppFileConfig {
        app: AppFileAppSection {
            name: app_name.clone(),
            access_mode: None,
            agent_mode: None,
        },
        auth: None,
        github: None,
        postgres: None,
        redis: None,
        storage: None,
        resources: None,
        reparo: None,
        cron: HashMap::new(),
        services: services_map,
        environments: HashMap::new(),
    };

    if let Err(e) =
        project_config::write_app_config_with_header(project_path, &app_file, APP_TOML_HEADER)
    {
        output::error(&e.message, &e.code, None);
        process::exit(1);
    }
    output::info(&format!("Wrote {}", project_config::APP_CONFIG_FILE), None);

    if write_agents_md(project_path) {
        output::info("Wrote AGENTS.md (agent operating notes)", None);
    }

    eprintln!("  Next: 'floo apps github connect <owner/repo>' wires the repo so");
    eprintln!("        every 'git push' to your default branch deploys to dev.");
    eprintln!("  Edit floo.app.toml to configure services; run 'floo preflight' to validate.");
    output::success(&format!("Initialized app '{app_name}'."), None);
}
