use std::collections::HashMap;
use std::io::BufRead;
use std::process::{self, Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use colored::Colorize;

use crate::config::load_config;
use crate::errors::ErrorCode;
use crate::output;
use crate::project_config;

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
}

pub fn dev(app_flag: Option<String>) {
    super::require_auth();

    let config = load_config();
    let client = super::init_client(Some(config));

    // --- Resolve app from config ---
    let cwd = std::env::current_dir().unwrap_or_else(|e| {
        output::error(
            &format!("Failed to read current directory: {e}"),
            &ErrorCode::CwdError,
            Some("Ensure the current directory exists and you have read permission."),
        );
        process::exit(1);
    });

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
    let app_config = match project_config::load_app_config(&cwd) {
        Ok(Some(c)) => c,
        Ok(None) => {
            output::error(
                "No floo.app.toml found in current directory.",
                &ErrorCode::NoConfigFound,
                Some("Run 'floo init' to create a project config, or cd to your project root."),
            );
            process::exit(1);
        }
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

    // --- Collect and validate service info ---
    let mut services: Vec<DevServiceInfo> = Vec::new();
    let mut missing_dev_command: Vec<String> = Vec::new();

    for (name, entry) in &app_config.services {
        let dev_command = match &entry.dev_command {
            Some(cmd) => cmd.clone(),
            None => {
                missing_dev_command.push(name.clone());
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

        services.push(DevServiceInfo {
            name: name.clone(),
            port,
            path,
            dev_command,
            migrate_command: entry.migrate_command.clone(),
        });
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

    // Sort by name for deterministic ordering
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

    // --- Create dev session via API ---
    let api_services: Vec<crate::api_types::DevSessionService> = services
        .iter()
        .map(|s| crate::api_types::DevSessionService {
            name: s.name.clone(),
            port: s.port,
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

    // --- Print status messages ---
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

    // Check if redis env vars were provided for any service
    let has_redis = session
        .services
        .values()
        .any(|env_map| env_map.keys().any(|k| k.starts_with("REDIS_")));
    if has_redis {
        output::info("  Redis: connection env vars injected", None);
    }

    // --- Print service URL table ---
    let headers = &["Service", "Port", "URL"];
    let rows: Vec<Vec<String>> = services
        .iter()
        .map(|svc| {
            vec![
                svc.name.clone(),
                svc.port.to_string(),
                format!("http://localhost:{}", svc.port),
            ]
        })
        .collect();

    if output::is_json_mode() {
        let json_services: Vec<serde_json::Value> = services
            .iter()
            .map(|svc| {
                let env_vars = session.services.get(&svc.name).cloned().unwrap_or_default();
                serde_json::json!({
                    "name": svc.name,
                    "port": svc.port,
                    "url": format!("http://localhost:{}", svc.port),
                    "env_vars": env_vars,
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
            }
        }));
    } else {
        output::table(headers, &rows, None);
        eprintln!();
    }

    // --- Set up signal handling ---
    let shutdown = Arc::new(AtomicBool::new(false));
    {
        let shutdown_flag = Arc::clone(&shutdown);
        // Install a SIGINT (Ctrl+C) handler
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

        // On non-Unix, set the flag immediately on Ctrl+C via process exit
        // (Windows doesn't support libc signals; processes get killed directly)
        #[cfg(not(unix))]
        {
            // Just set the flag after a brief delay if the process is still alive
            let sf = Arc::clone(&shutdown_flag);
            thread::spawn(move || {
                // On Windows, rely on the process being killed externally
                loop {
                    thread::sleep(Duration::from_millis(500));
                    if sf.load(Ordering::Relaxed) {
                        break;
                    }
                }
            });
        }

        // Spawn a thread to watch for the signal and set our Arc flag
        #[cfg(unix)]
        thread::spawn(move || loop {
            if SHUTDOWN_FLAG.load(Ordering::SeqCst) != 0 {
                shutdown_flag.store(true, Ordering::Relaxed);
                break;
            }
            thread::sleep(Duration::from_millis(100));
        });
    }

    // --- Spawn child processes ---
    let mut children: Vec<(String, Child)> = Vec::new();

    for (idx, svc) in services.iter().enumerate() {
        let working_dir = cwd.join(&svc.path);

        if !working_dir.exists() {
            output::error(
                &format!("Service '{}' path '{}' does not exist.", svc.name, svc.path),
                &ErrorCode::InvalidPath,
                Some("Check the 'path' value in floo.app.toml."),
            );
            // Kill already-spawned children before exiting
            cleanup_children(&mut children);
            cleanup_session(&client, &app_id, &session_id);
            process::exit(1);
        }

        // Build environment: inherit current env + inject dev session env vars
        let mut env_vars: HashMap<String, String> = std::env::vars().collect();
        if let Some(svc_env) = session.services.get(&svc.name) {
            for (k, v) in svc_env {
                env_vars.insert(k.clone(), v.clone());
            }
        }
        // Also inject PORT so services that read it get the right value
        env_vars.insert("PORT".to_string(), svc.port.to_string());

        // Run migrate_command before starting the service, if configured
        if let Some(ref migrate_cmd) = svc.migrate_command {
            output::info(&format!("  Running migrations for {}...", svc.name), None);
            let migrate_status = match Command::new("sh")
                .arg("-c")
                .arg(migrate_cmd)
                .current_dir(&working_dir)
                .envs(&env_vars)
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit())
                .status()
            {
                Ok(s) => s,
                Err(e) => {
                    output::error(
                        &format!("Failed to run migrate_command for '{}': {e}", svc.name),
                        &ErrorCode::InternalError,
                        Some(&format!("Command: {migrate_cmd}")),
                    );
                    cleanup_children(&mut children);
                    cleanup_session(&client, &app_id, &session_id);
                    process::exit(1);
                }
            };
            if !migrate_status.success() {
                output::warn(&format!(
                    "Migration for '{}' exited with code {} — continuing anyway.",
                    svc.name,
                    migrate_status.code().unwrap_or(-1)
                ));
            }
        }

        let child = match Command::new("sh")
            .arg("-c")
            .arg(&svc.dev_command)
            .current_dir(&working_dir)
            .envs(&env_vars)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
        {
            Ok(c) => c,
            Err(e) => {
                output::error(
                    &format!("Failed to start service '{}': {e}", svc.name),
                    &ErrorCode::InternalError,
                    Some(&format!("Command: {}", svc.dev_command)),
                );
                cleanup_children(&mut children);
                cleanup_session(&client, &app_id, &session_id);
                process::exit(1);
            }
        };

        if !output::is_json_mode() {
            let prefix = format!("[{}]", svc.name);
            let colored = color_service_prefix(&prefix, idx);
            eprintln!("{colored} started (pid {})", child.id());
        }

        children.push((svc.name.clone(), child));
    }

    // --- Multiplex stdout/stderr from children ---
    let mut reader_handles: Vec<thread::JoinHandle<()>> = Vec::new();

    // We need to take ownership of the piped streams before the main loop
    // Use indices matching children vec for color assignment
    let mut child_entries: Vec<(String, Child)> = Vec::new();
    std::mem::swap(&mut children, &mut child_entries);

    for (idx, (name, mut child)) in child_entries.into_iter().enumerate() {
        let prefix = format!("[{}]", name);

        // Spawn stdout reader
        if let Some(stdout) = child.stdout.take() {
            let shutdown_ref = Arc::clone(&shutdown);
            let pfx = prefix.clone();
            let color_idx = idx;
            let json_mode = output::is_json_mode();
            let svc_name = name.clone();
            reader_handles.push(thread::spawn(move || {
                let reader = std::io::BufReader::new(stdout);
                for line in reader.lines() {
                    if shutdown_ref.load(Ordering::Relaxed) {
                        break;
                    }
                    match line {
                        Ok(text) => {
                            if json_mode {
                                output::print_json(&serde_json::json!({
                                    "event": "log",
                                    "service": svc_name,
                                    "stream": "stdout",
                                    "line": text,
                                }));
                            } else {
                                let colored_pfx = color_service_prefix(&pfx, color_idx);
                                eprintln!("{colored_pfx} {text}");
                            }
                        }
                        Err(_) => break,
                    }
                }
            }));
        }

        // Spawn stderr reader
        if let Some(stderr) = child.stderr.take() {
            let shutdown_ref = Arc::clone(&shutdown);
            let pfx = prefix.clone();
            let color_idx = idx;
            let json_mode = output::is_json_mode();
            let svc_name = name.clone();
            reader_handles.push(thread::spawn(move || {
                let reader = std::io::BufReader::new(stderr);
                for line in reader.lines() {
                    if shutdown_ref.load(Ordering::Relaxed) {
                        break;
                    }
                    match line {
                        Ok(text) => {
                            if json_mode {
                                output::print_json(&serde_json::json!({
                                    "event": "log",
                                    "service": svc_name,
                                    "stream": "stderr",
                                    "line": text,
                                }));
                            } else {
                                let colored_pfx = color_service_prefix(&pfx, color_idx);
                                eprintln!("{colored_pfx} {text}");
                            }
                        }
                        Err(_) => break,
                    }
                }
            }));
        }

        children.push((name, child));
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

        // Check if any child has exited
        let mut all_exited = true;
        for (name, child) in &mut children {
            match child.try_wait() {
                Ok(Some(status)) => {
                    if !status.success()
                        && !shutdown.load(Ordering::Relaxed)
                        && !output::is_json_mode()
                    {
                        eprintln!(
                            "{} Service '{}' exited with {}",
                            "Error:".red(),
                            name,
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
    cleanup_children(&mut children);

    // Wait for reader threads to finish
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

/// Global flag set by the signal handler (must be static for the C signal handler).
static SHUTDOWN_FLAG: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);

#[cfg(unix)]
extern "C" fn signal_handler(_sig: libc::c_int) {
    SHUTDOWN_FLAG.store(1, Ordering::SeqCst);
}

fn cleanup_children(children: &mut [(String, Child)]) {
    // Send SIGTERM (Unix) or kill (Windows) to all children
    for (name, child) in children.iter_mut() {
        #[cfg(unix)]
        unsafe {
            libc::kill(child.id() as libc::pid_t, libc::SIGTERM);
        }
        #[cfg(not(unix))]
        {
            let _ = child.kill();
        }
        if !output::is_json_mode() {
            let prefix = format!("[{}]", name);
            eprintln!("{} shutting down", prefix.dimmed());
        }
    }

    // Wait up to 5 seconds for graceful exit
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        let mut all_done = true;
        for (_, child) in children.iter_mut() {
            match child.try_wait() {
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

    // Force-kill any remaining
    for (name, child) in children.iter_mut() {
        match child.try_wait() {
            Ok(Some(_)) => {}
            _ => {
                if !output::is_json_mode() {
                    let prefix = format!("[{}]", name);
                    eprintln!("{} force killing", prefix.dimmed());
                }
                let _ = child.kill();
                let _ = child.wait();
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
