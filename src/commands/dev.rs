use std::collections::{BTreeMap, HashMap};
use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::process::{self, Child};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use colored::Colorize;

use crate::config::load_config;
use crate::container::{self, BuildSpec, RunSpec, Runtime, DEFAULT_WORKDIR};
use crate::dev_proxy::{self, FixtureUser};
use crate::errors::{ErrorCode, FlooError};
use crate::output;
use crate::project_config;
use crate::project_config::AppAccessMode;

/// Color palette for service prefixes (cycles for >6 services).
const SERVICE_COLORS: &[&str] = &["blue", "green", "magenta", "cyan", "yellow", "red"];

fn color_service_prefix(prefix: &str, color_index: usize) -> String {
    let color = SERVICE_COLORS[color_index % SERVICE_COLORS.len()];
    match color {
        "blue" => prefix.blue().bold().to_string(),
        "green" => prefix.green().bold().to_string(),
        "magenta" => prefix.magenta().bold().to_string(),
        "cyan" => prefix.cyan().bold().to_string(),
        "yellow" => prefix.yellow().bold().to_string(),
        "red" => prefix.red().bold().to_string(),
        _ => prefix.bold().to_string(),
    }
}

/// Info collected from config for each service to run locally.
struct DevServiceInfo {
    name: String,
    port: u16,
    path: String,
    dev_command: String,
    migrate_command: Option<String>,
    /// Resolved absolute path to the service directory on the host.
    working_dir: PathBuf,
    /// Resolved absolute path to the service's Dockerfile.
    dockerfile: PathBuf,
    /// In-container WORKDIR parsed from the Dockerfile (or DEFAULT_WORKDIR).
    container_workdir: String,
}

/// A spawned child + the container name we used, so cleanup can `docker stop`
/// it before falling back to killing the local `docker run` process.
struct RunningService {
    name: String,
    container_name: String,
    child: Child,
}

pub struct DevArgs {
    pub app: Option<String>,
    pub fixture_user: Option<String>,
    pub fixture_id: Option<String>,
    pub fixture_name: Option<String>,
    pub fixture_role: Option<String>,
}

/// Build a FixtureUser from CLI flags, filling sensible defaults.
fn build_fixture_user(args: &DevArgs) -> Option<FixtureUser> {
    let email = args.fixture_user.as_ref()?.clone();
    let local_part = email
        .split('@')
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or("user");
    Some(FixtureUser {
        id: args
            .fixture_id
            .clone()
            .unwrap_or_else(|| format!("dev-fixture-{local_part}")),
        name: args.fixture_name.clone().unwrap_or_else(|| email.clone()),
        role: args
            .fixture_role
            .clone()
            .unwrap_or_else(|| "member".to_string()),
        email,
    })
}

pub fn dev(args: DevArgs) {
    let DevArgs { app: app_flag, .. } = &args;
    let app_flag = app_flag.clone();

    super::require_auth();

    let config = load_config();
    let client = super::init_client(Some(config));

    // --- Resolve app from config ---
    let cwd = super::read_cwd_or_exit();

    let resolved = match project_config::resolve_app_context(&cwd, app_flag.as_deref()) {
        Ok(r) => r,
        Err(e) => {
            output::error(&e.message, &e.code, e.suggestion.as_deref());
            process::exit(1);
        }
    };

    let app = super::resolve_app_or_exit(&client, &resolved.app_name);
    let app_id = app.id.clone();
    let app_name = app.name.clone();

    // --- Read service definitions from floo.app.toml ---
    let app_config = match super::load_app_config_for_resolved_app(&resolved) {
        Ok(c) => c,
        Err(e) => {
            output::error(&e.message, &e.code, e.suggestion.as_deref());
            process::exit(1);
        }
    };

    if app_config.services.is_empty() {
        output::error(
            "No services defined in floo.app.toml.",
            &ErrorCode::NoDeployableServices,
            Some("Add [services.<name>] sections with dev_command, path, and port."),
        );
        process::exit(1);
    }

    // --- Collect and validate service info (all but Dockerfile/workdir, which
    // happens after we've enumerated services so we can report all problems
    // at once rather than failing on the first one).
    let mut partial: Vec<(String, AppServicePartial)> = Vec::new();
    let mut missing_dev_command: Vec<String> = Vec::new();
    let mut skipped_external: Vec<String> = Vec::new();

    for (name, entry) in &app_config.services {
        let dev_command = match &entry.dev_command {
            Some(cmd) => cmd.clone(),
            None => {
                if entry.repo.is_some() {
                    skipped_external.push(name.clone());
                } else {
                    missing_dev_command.push(name.clone());
                }
                continue;
            }
        };

        let port = match entry.port {
            Some(p) => p,
            None => {
                output::error(
                    &format!("Service '{name}' is missing 'port' in floo.app.toml."),
                    &ErrorCode::MissingPort,
                    Some(&format!("Add port = <number> to [services.{name}].")),
                );
                process::exit(1);
            }
        };

        let path = match &entry.path {
            Some(p) => p.clone(),
            None => {
                output::error(
                    &format!("Service '{name}' is missing 'path' in floo.app.toml."),
                    &ErrorCode::InvalidProjectConfig,
                    Some(&format!("Add path = \"./subdir\" to [services.{name}].")),
                );
                process::exit(1);
            }
        };

        partial.push((
            name.clone(),
            AppServicePartial {
                port,
                path,
                dev_command,
                migrate_command: entry.migrate_command.clone(),
            },
        ));
    }

    if !missing_dev_command.is_empty() {
        let names = missing_dev_command.join(", ");
        output::error(
            &format!("Services missing 'dev_command': {names}"),
            &ErrorCode::InvalidProjectConfig,
            Some("Add dev_command = \"<cmd>\" to each [services.<name>] in floo.app.toml."),
        );
        process::exit(1);
    }

    for name in &skipped_external {
        output::warn(&format!(
            "Skipping '{name}' (external repo, no dev_command) — \
             the service won't run locally. Hit its prod/preview URL or add a \
             dev_command if you want to run it locally."
        ));
    }

    // --- Container preflight: every runnable service needs a Dockerfile.
    // Collect all missing dockerfiles up front so the error report is
    // complete rather than one-at-a-time.
    let mut services: Vec<DevServiceInfo> = Vec::new();
    let mut missing_dockerfile: Vec<(String, PathBuf)> = Vec::new();

    for (name, p) in partial {
        let working_dir = resolved.config_dir.join(&p.path);
        if !working_dir.exists() {
            output::error(
                &format!("Service '{name}' path '{}' does not exist.", p.path),
                &ErrorCode::InvalidPath,
                Some("Check the 'path' value in floo.app.toml."),
            );
            process::exit(1);
        }
        let dockerfile = working_dir.join("Dockerfile");
        if !dockerfile.exists() {
            missing_dockerfile.push((name, dockerfile));
            continue;
        }
        let container_workdir = std::fs::read_to_string(&dockerfile)
            .ok()
            .and_then(|c| container::parse_workdir(&c))
            .unwrap_or_else(|| DEFAULT_WORKDIR.to_string());
        services.push(DevServiceInfo {
            name,
            port: p.port,
            path: p.path,
            dev_command: p.dev_command,
            migrate_command: p.migrate_command,
            working_dir,
            dockerfile,
            container_workdir,
        });
    }

    if !missing_dockerfile.is_empty() {
        let listing: String = missing_dockerfile
            .iter()
            .map(|(name, path)| format!("  - {name} ({})", path.display()))
            .collect::<Vec<_>>()
            .join("\n");
        output::error(
            &format!("floo dev needs a Dockerfile per service. Missing:\n{listing}"),
            &ErrorCode::DockerfileMissing,
            Some(
                "Run 'floo init' to scaffold a Dockerfile, or add one to each service's path. \
                 floo dev runs every dev_command inside the same image that ships to production.",
            ),
        );
        process::exit(1);
    }

    // Whether identity-header injection should run for this session.
    let is_accounts_mode = matches!(app_config.app.access_mode, Some(AppAccessMode::Accounts));
    let fixture_user = build_fixture_user(&args);
    let fixture_active = fixture_user.is_some() && is_accounts_mode;

    if fixture_user.is_some() && !is_accounts_mode {
        output::warn(
            "--fixture-user was set but access_mode is not \"accounts\" — \
             the identity-header proxy only runs for accounts-mode apps. \
             Skipping proxy setup.",
        );
    }

    services.sort_by(|a, b| a.name.cmp(&b.name));

    // --- Check for port conflicts ---
    {
        let mut seen_ports: HashMap<u16, &str> = HashMap::new();
        for svc in &services {
            if let Some(existing) = seen_ports.get(&svc.port) {
                output::error(
                    &format!(
                        "Port conflict: services '{}' and '{}' both use port {}.",
                        existing, svc.name, svc.port
                    ),
                    &ErrorCode::InvalidProjectConfig,
                    Some("Each service must use a unique port."),
                );
                process::exit(1);
            }
            seen_ports.insert(svc.port, &svc.name);
        }
    }

    // --- Dry-run short-circuit ---
    //
    // Dry-run must NOT call create_dev_session. That endpoint has two real
    // side effects: (1) it registers a dev session row on the platform, and
    // (2) it returns managed-service credentials (DATABASE_URL, REDIS_URL, …).
    if output::is_dry_run_mode() {
        let plan_services: Vec<serde_json::Value> = services
            .iter()
            .map(|svc| {
                serde_json::json!({
                    "name": svc.name,
                    "port": svc.port,
                    "url": format!("http://localhost:{}", svc.port),
                    "path": svc.path,
                    "dev_command": svc.dev_command,
                    "migrate_command": svc.migrate_command,
                    "container": {
                        "dockerfile": svc.dockerfile.display().to_string(),
                        "workdir": svc.container_workdir,
                    },
                })
            })
            .collect();
        let mut preview = format!(
            "Would start {} service(s) for '{app_name}':",
            services.len()
        );
        for svc in &services {
            preview.push_str(&format!(
                "\n    {} on http://localhost:{}",
                svc.name, svc.port
            ));
        }
        if !skipped_external.is_empty() {
            preview.push_str(&format!(
                "\nSkipping external service(s): {}",
                skipped_external.join(", ")
            ));
        }
        output::dry_run_preview(
            &preview,
            serde_json::json!({
                "action": "start_dev_session",
                "app": app_name,
                "services": plan_services,
                "skipped_external": skipped_external,
            }),
        );
        return;
    }

    // --- Container runtime preflight (after dry-run, before any side effect).
    let runtime = match container::detect_runtime() {
        Ok(r) => r,
        Err(e) => {
            output::error(&e.message, &e.code, e.suggestion.as_deref());
            process::exit(1);
        }
    };

    // --- Build images sequentially. First-run cost is unavoidable; subsequent
    // runs reuse cached images keyed by Dockerfile + lockfile content.
    let mut images: HashMap<String, String> = HashMap::new();
    for svc in &services {
        let tag = match ensure_image(
            runtime,
            &resolved.app_name,
            &svc.name,
            &svc.working_dir,
            &svc.dockerfile,
        ) {
            Ok(t) => t,
            Err(e) => {
                output::error(&e.message, &e.code, e.suggestion.as_deref());
                process::exit(1);
            }
        };
        images.insert(svc.name.clone(), tag);
    }

    // --- Create dev session via API ---
    let api_services: Vec<crate::api_types::DevSessionService> = services
        .iter()
        .map(|s| crate::api_types::DevSessionService {
            name: s.name.clone(),
            port: Some(s.port),
        })
        .collect();

    let spinner = output::Spinner::new("Creating dev session...");
    let session = match client.create_dev_session(&app_id, &api_services) {
        Ok(s) => {
            spinner.finish();
            s
        }
        Err(e) => {
            spinner.finish();
            output::error(
                &format!("Failed to create dev session: {}", e.message),
                &ErrorCode::from_api(&e.code),
                None,
            );
            process::exit(1);
        }
    };

    let session_id = session.session_id.clone();

    output::info(
        &format!("Dev session started for {} ({})", app_name, session_id),
        None,
    );

    if session.postgres_authorized {
        output::info(
            "  Postgres: your IP is authorized for direct connections",
            None,
        );
    }

    let has_redis = session
        .services
        .values()
        .any(|env_map| env_map.keys().any(|k| k.starts_with("REDIS_")));
    if has_redis {
        output::info("  Redis: connection env vars injected", None);
    }

    // --- Start fixture-user proxies (accounts-mode + --fixture-user) ---
    let mut proxy_ports: HashMap<String, u16> = HashMap::new();
    if fixture_active {
        let user = fixture_user.clone().expect("fixture_active implies Some");
        output::info(
            &format!(
                "  Identity headers: injecting X-Floo-User-* as {} on accounts-mode services",
                user.email
            ),
            None,
        );
        for svc in &services {
            match dev_proxy::start_proxy(0, svc.port, user.clone()) {
                Ok((_handle, bound_port)) => {
                    proxy_ports.insert(svc.name.clone(), bound_port);
                }
                Err(e) => {
                    output::warn(&format!(
                        "Failed to start identity-header proxy for '{}': {e}. \
                         The raw service URL still works — your app just won't \
                         see X-Floo-User-* headers.",
                        svc.name
                    ));
                }
            }
        }
    }

    // --- Print service URL table ---
    let headers: &[&str] = if proxy_ports.is_empty() {
        &["Service", "Port", "URL"]
    } else {
        &["Service", "Port", "URL", "Auth-proxied URL"]
    };
    let rows: Vec<Vec<String>> = services
        .iter()
        .map(|svc| {
            let raw_url = format!("http://localhost:{}", svc.port);
            let mut row = vec![svc.name.clone(), svc.port.to_string(), raw_url];
            if !proxy_ports.is_empty() {
                let auth_cell = match proxy_ports.get(&svc.name) {
                    Some(p) => format!("http://localhost:{p}"),
                    None => String::from("—"),
                };
                row.push(auth_cell);
            }
            row
        })
        .collect();

    if output::is_json_mode() {
        let json_services: Vec<serde_json::Value> = services
            .iter()
            .map(|svc| {
                let env_vars = session.services.get(&svc.name).cloned().unwrap_or_default();
                let auth_url = proxy_ports
                    .get(&svc.name)
                    .map(|p| format!("http://localhost:{p}"));
                serde_json::json!({
                    "name": svc.name,
                    "port": svc.port,
                    "url": format!("http://localhost:{}", svc.port),
                    "auth_proxied_url": auth_url,
                    "env_vars": env_vars,
                    "container": {
                        "image": images.get(&svc.name),
                        "workdir": svc.container_workdir,
                    },
                })
            })
            .collect();
        output::print_json(&serde_json::json!({
            "success": true,
            "data": {
                "session_id": session_id,
                "app": app_name,
                "postgres_authorized": session.postgres_authorized,
                "services": json_services,
                "container_runtime": runtime.binary(),
            }
        }));
    } else {
        output::table(headers, &rows, None);
        eprintln!();
        eprintln!(
            "  {} services run inside {}. Bind to {} so the published port is reachable.",
            "Note:".dimmed(),
            runtime,
            "0.0.0.0:$PORT".bold()
        );
        eprintln!();
    }

    // --- Set up signal handling ---
    let shutdown = Arc::new(AtomicBool::new(false));
    {
        let shutdown_flag = Arc::clone(&shutdown);
        #[cfg(unix)]
        unsafe {
            libc::signal(
                libc::SIGINT,
                signal_handler as *const () as libc::sighandler_t,
            );
            libc::signal(
                libc::SIGTERM,
                signal_handler as *const () as libc::sighandler_t,
            );
        }
        SHUTDOWN_FLAG.store(0, Ordering::SeqCst);

        #[cfg(not(unix))]
        {
            let sf = Arc::clone(&shutdown_flag);
            thread::spawn(move || loop {
                thread::sleep(Duration::from_millis(500));
                if sf.load(Ordering::Relaxed) {
                    break;
                }
            });
        }

        #[cfg(unix)]
        thread::spawn(move || loop {
            if SHUTDOWN_FLAG.load(Ordering::SeqCst) != 0 {
                shutdown_flag.store(true, Ordering::Relaxed);
                break;
            }
            thread::sleep(Duration::from_millis(100));
        });
    }

    // --- Run migrations and spawn each service inside its container.
    let mut running: Vec<RunningService> = Vec::new();

    for (idx, svc) in services.iter().enumerate() {
        let image = images
            .get(&svc.name)
            .expect("image was built for every service")
            .clone();

        let env = build_env_for_service(svc, &session);

        // Run migrations inside the container, blocking until done.
        if let Some(ref migrate_cmd) = svc.migrate_command {
            output::info(&format!("  Running migrations for {}...", svc.name), None);
            let migrate_spec = RunSpec {
                image: image.clone(),
                workdir_in_container: svc.container_workdir.clone(),
                source_mount_host: svc.working_dir.clone(),
                env: env.clone(),
                command: migrate_cmd.clone(),
                ports: BTreeMap::new(),
                preserved_paths: container::default_preserved_paths(),
                name: container::container_name(
                    &resolved.app_name,
                    &format!("{}-migrate", svc.name),
                ),
                interactive: false,
                tty: false,
                init: true,
            };
            match container::run_foreground(runtime, &migrate_spec) {
                Ok(status) if !status.success() => {
                    output::warn(&format!(
                        "Migration for '{}' exited with code {} — continuing anyway.",
                        svc.name,
                        status.code().unwrap_or(-1)
                    ));
                }
                Ok(_) => {}
                Err(e) => {
                    output::error(
                        &format!(
                            "Failed to run migrate_command for '{}': {}",
                            svc.name, e.message
                        ),
                        &e.code,
                        e.suggestion.as_deref(),
                    );
                    cleanup_running(runtime, &mut running);
                    cleanup_session(&client, &app_id, &session_id);
                    process::exit(1);
                }
            }
        }

        // Spawn the dev_command. Port published to host loopback only —
        // services must bind 0.0.0.0:$PORT inside the container.
        let mut ports = BTreeMap::new();
        ports.insert(svc.port, svc.port);
        let container_name = container::container_name(&resolved.app_name, &svc.name);
        let dev_spec = RunSpec {
            image: image.clone(),
            workdir_in_container: svc.container_workdir.clone(),
            source_mount_host: svc.working_dir.clone(),
            env,
            command: svc.dev_command.clone(),
            ports,
            preserved_paths: container::default_preserved_paths(),
            name: container_name.clone(),
            interactive: false,
            tty: false,
            init: true,
        };

        let child = match container::spawn_piped(runtime, &dev_spec) {
            Ok(c) => c,
            Err(e) => {
                output::error(
                    &format!("Failed to start service '{}': {}", svc.name, e.message),
                    &e.code,
                    Some(&format!("Command: {}", svc.dev_command)),
                );
                cleanup_running(runtime, &mut running);
                cleanup_session(&client, &app_id, &session_id);
                process::exit(1);
            }
        };

        if !output::is_json_mode() {
            let prefix = format!("[{}]", svc.name);
            let colored = color_service_prefix(&prefix, idx);
            eprintln!("{colored} started in {} (pid {})", runtime, child.id());
        }

        running.push(RunningService {
            name: svc.name.clone(),
            container_name,
            child,
        });
    }

    // --- Multiplex stdout/stderr from children ---
    let mut reader_handles: Vec<thread::JoinHandle<()>> = Vec::new();

    let mut child_entries = std::mem::take(&mut running);
    for (idx, mut entry) in child_entries.drain(..).enumerate() {
        let prefix = format!("[{}]", entry.name);

        if let Some(stdout) = entry.child.stdout.take() {
            let shutdown_ref = Arc::clone(&shutdown);
            let pfx = prefix.clone();
            let svc_name = entry.name.clone();
            reader_handles.push(thread::spawn(move || {
                let reader = std::io::BufReader::new(stdout);
                for line in reader.lines() {
                    if shutdown_ref.load(Ordering::Relaxed) {
                        break;
                    }
                    match line {
                        Ok(text) => emit_line(&svc_name, &pfx, idx, "stdout", &text),
                        Err(_) => break,
                    }
                }
            }));
        }

        if let Some(stderr) = entry.child.stderr.take() {
            let shutdown_ref = Arc::clone(&shutdown);
            let pfx = prefix.clone();
            let svc_name = entry.name.clone();
            reader_handles.push(thread::spawn(move || {
                let reader = std::io::BufReader::new(stderr);
                for line in reader.lines() {
                    if shutdown_ref.load(Ordering::Relaxed) {
                        break;
                    }
                    match line {
                        Ok(text) => emit_line(&svc_name, &pfx, idx, "stderr", &text),
                        Err(_) => break,
                    }
                }
            }));
        }

        running.push(entry);
    }

    // --- Wait for shutdown signal or all children to exit ---
    loop {
        if shutdown.load(Ordering::Relaxed) {
            if !output::is_json_mode() {
                eprintln!();
                eprintln!("{}", "Shutting down...".dimmed());
            }
            break;
        }

        let mut all_exited = true;
        for entry in running.iter_mut() {
            match entry.child.try_wait() {
                Ok(Some(status)) => {
                    if !status.success()
                        && !shutdown.load(Ordering::Relaxed)
                        && !output::is_json_mode()
                    {
                        eprintln!(
                            "{} Service '{}' exited with {}",
                            "Error:".red(),
                            entry.name,
                            status
                        );
                    }
                }
                Ok(None) => {
                    all_exited = false;
                }
                Err(_) => {}
            }
        }

        if all_exited {
            break;
        }

        thread::sleep(Duration::from_millis(200));
    }

    // --- Cleanup ---
    cleanup_running(runtime, &mut running);

    for handle in reader_handles {
        let _ = handle.join();
    }

    cleanup_session(&client, &app_id, &session_id);

    if !output::is_json_mode() {
        output::info("Dev session ended.", None);
    } else {
        output::print_json(&serde_json::json!({
            "event": "session_ended",
            "session_id": session_id,
        }));
    }
}

fn emit_line(svc: &str, prefix: &str, color_idx: usize, stream: &str, text: &str) {
    if output::is_json_mode() {
        output::print_json(&serde_json::json!({
            "event": "log",
            "service": svc,
            "stream": stream,
            "line": text,
        }));
    } else {
        let colored_pfx = color_service_prefix(prefix, color_idx);
        eprintln!("{colored_pfx} {text}");
    }
}

fn build_env_for_service(
    svc: &DevServiceInfo,
    session: &crate::api_types::DevSessionResponse,
) -> BTreeMap<String, String> {
    // Container env is intentionally minimal — only floo-managed vars plus
    // PORT. Inheriting the host environment leaks PATH/HOME/SHELL with
    // values that don't apply inside the image.
    let mut env: BTreeMap<String, String> = BTreeMap::new();
    if let Some(svc_env) = session.services.get(&svc.name) {
        for (k, v) in svc_env {
            env.insert(k.clone(), v.clone());
        }
    }
    env.insert("PORT".to_string(), svc.port.to_string());
    env
}

fn ensure_image(
    runtime: Runtime,
    app: &str,
    service: &str,
    context_dir: &Path,
    dockerfile: &Path,
) -> Result<String, FlooError> {
    let dockerfile_content = std::fs::read_to_string(dockerfile).map_err(|e| {
        FlooError::with_suggestion(
            ErrorCode::DockerfileMissing,
            format!("Failed to read {}: {e}", dockerfile.display()),
            "Check file permissions on the Dockerfile.".to_string(),
        )
    })?;
    let hash = container::compute_build_hash(&dockerfile_content, context_dir);
    let tag = container::image_tag(app, service, &hash);

    if container::image_exists(runtime, &tag) {
        return Ok(tag);
    }

    output::info(
        &format!("Building dev image for '{service}' (first run or deps changed)..."),
        None,
    );
    container::build_image(
        runtime,
        &BuildSpec {
            tag: tag.clone(),
            context_dir: context_dir.to_path_buf(),
            dockerfile: dockerfile.to_path_buf(),
        },
    )?;
    Ok(tag)
}

/// Holding struct for partially-validated service config. Keeps the
/// per-service block of `dev()` flat by separating "fields read from the TOML"
/// from "fields resolved against the filesystem (working_dir, dockerfile)".
struct AppServicePartial {
    port: u16,
    path: String,
    dev_command: String,
    migrate_command: Option<String>,
}

/// Global flag set by the signal handler (must be static for the C signal handler).
static SHUTDOWN_FLAG: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);

#[cfg(unix)]
extern "C" fn signal_handler(_sig: libc::c_int) {
    SHUTDOWN_FLAG.store(1, Ordering::SeqCst);
}

fn cleanup_running(runtime: Runtime, running: &mut [RunningService]) {
    // Step 1: ask the container runtime to stop each container by name.
    // `docker stop` sends SIGTERM, waits 10s, then SIGKILL — and `--rm`
    // means the container is removed once the process exits.
    for entry in running.iter() {
        if !output::is_json_mode() {
            let prefix = format!("[{}]", entry.name);
            eprintln!("{} stopping container", prefix.dimmed());
        }
        container::stop_container(runtime, &entry.container_name);
    }

    // Step 2: wait up to 12 seconds for the local docker run process to exit
    // (a hair longer than docker stop's own 10s timeout).
    let deadline = std::time::Instant::now() + Duration::from_secs(12);
    loop {
        let mut all_done = true;
        for entry in running.iter_mut() {
            match entry.child.try_wait() {
                Ok(Some(_)) => {}
                _ => {
                    all_done = false;
                }
            }
        }
        if all_done || std::time::Instant::now() >= deadline {
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }

    // Step 3: any docker run process that hasn't exited gets SIGKILL'd
    // locally. The container is already gone (or being torn down) by now.
    for entry in running.iter_mut() {
        match entry.child.try_wait() {
            Ok(Some(_)) => {}
            _ => {
                if !output::is_json_mode() {
                    let prefix = format!("[{}]", entry.name);
                    eprintln!("{} force killing", prefix.dimmed());
                }
                let _ = entry.child.kill();
                let _ = entry.child.wait();
            }
        }
    }
}

fn cleanup_session(client: &crate::api_client::FlooClient, app_id: &str, session_id: &str) {
    if let Err(e) = client.delete_dev_session(app_id, session_id) {
        if !output::is_json_mode() {
            eprintln!(
                "{} Failed to delete dev session: {}",
                "Warning:".yellow(),
                e.message
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::project_config::{AppServiceEntry, AppServiceType};

    fn entry_with(repo: Option<&str>, dev_command: Option<&str>) -> AppServiceEntry {
        AppServiceEntry {
            service_type: AppServiceType::Web,
            path: Some(".".into()),
            repo: repo.map(String::from),
            version: None,
            plan: None,
            port: Some(3000),
            ingress: None,
            env_file: None,
            domain: None,
            cpu: None,
            memory: None,
            max_instances: None,
            min_instances: None,
            dev_command: dev_command.map(String::from),
            migrate_command: None,
            env: None,
        }
    }

    /// Mirror of the partition logic in `dev()` so we can assert the rule
    /// without spinning up subprocesses. If the dev() loop changes, update
    /// both this helper and the call sites.
    fn classify(entry: &AppServiceEntry) -> &'static str {
        match entry.dev_command {
            Some(_) => "runnable",
            None if entry.repo.is_some() => "skipped_external",
            None => "missing",
        }
    }

    #[test]
    fn external_repo_without_dev_command_is_skipped_not_an_error() {
        let entry = entry_with(Some("getfloo/floo-crm-fixture"), None);
        assert_eq!(classify(&entry), "skipped_external");
    }

    #[test]
    fn local_service_without_dev_command_is_a_hard_error() {
        let entry = entry_with(None, None);
        assert_eq!(classify(&entry), "missing");
    }

    #[test]
    fn local_service_with_dev_command_is_runnable() {
        let entry = entry_with(None, Some("npm run dev"));
        assert_eq!(classify(&entry), "runnable");
    }

    #[test]
    fn external_service_with_dev_command_still_runs() {
        let entry = entry_with(Some("getfloo/other"), Some("./run.sh"));
        assert_eq!(classify(&entry), "runnable");
    }
}
