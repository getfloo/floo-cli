use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::process;
use std::thread;
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::api_client::FlooClient;
use crate::api_types::Deploy;
use crate::config::load_config;
use crate::deploy_status;
use crate::detection::{detect_for_services, DetectionResult};
use crate::errors::{ErrorCode, FlooApiError};
use crate::output;
use crate::project_config::{
    self, validate_service_name, AppAccessMode, AppAgentMode, AppSource, ServiceConfig,
    ServiceIngress, ServiceType,
};
use crate::resolve::resolve_app;

const POLL_INTERVAL: Duration = Duration::from_secs(2);
const POLL_TIMEOUT: Duration = Duration::from_secs(600); // 10 minutes

#[derive(Debug, Serialize)]
struct EnvInjectionPlan {
    mode: String,
    services: Vec<ServiceEnvInjectionPlan>,
    notes: Vec<String>,
}

#[derive(Debug, Serialize)]
struct ServiceEnvInjectionPlan {
    service: String,
    managed: Vec<ManagedEnvInjection>,
    required: Vec<String>,
    optional: Vec<String>,
}

#[derive(Debug, Serialize)]
struct ManagedEnvInjection {
    handle: String,
    keys: Vec<String>,
}

/// Severity of a preflight finding.
///
/// - `Error` blocks the deploy (preflight exits 1). Reserved for configs that
///   are *guaranteed* to fail to build or run from what local config alone can
///   prove (a missing build context, an invalid cron expression, a cron job
///   pointing at a service that doesn't exist in a multi-service app).
/// - `Warning` is surfaced loudly but does not block. Used where local
///   preflight can't see the full picture — server-side `floo env set` vars and
///   external databases are invisible locally, so "looks unsatisfied" is a
///   strong signal, not a certainty.
/// - `Info` is advisory context (which services are internet-facing, etc.).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
enum Severity {
    Error,
    Warning,
    Info,
}

impl Severity {
    /// Glyph used in human output: ✗ for errors, ⚠ for warnings, • for info.
    fn glyph(self) -> &'static str {
        match self {
            Severity::Error => "\u{2717}",
            Severity::Warning => "\u{26a0}",
            Severity::Info => "\u{2022}",
        }
    }
}

/// A single typed preflight finding.
///
/// Replaces the prior three uncorrelated output channels — an untyped
/// `errors` array, an untyped `warnings` array, and a `security_notes` list of
/// bare strings — with one severity-tagged shape. Agents read `severity` +
/// `code` to tell a deploy-blocking error from informational context without
/// screen-scraping prose; `data.valid` is `false` iff any finding is an error.
#[derive(Debug, Clone, Serialize)]
struct PreflightFinding {
    severity: Severity,
    code: String,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    hint: Option<String>,
}

impl PreflightFinding {
    fn new(severity: Severity, code: &str, message: String) -> Self {
        Self {
            severity,
            code: code.to_string(),
            message,
            path: None,
            hint: None,
        }
    }

    fn error(code: &str, message: String) -> Self {
        Self::new(Severity::Error, code, message)
    }

    fn warning(code: &str, message: String) -> Self {
        Self::new(Severity::Warning, code, message)
    }

    fn info(code: &str, message: String) -> Self {
        Self::new(Severity::Info, code, message)
    }

    fn with_path(mut self, path: impl Into<String>) -> Self {
        self.path = Some(path.into());
        self
    }

    fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    fn is_error(&self) -> bool {
        self.severity == Severity::Error
    }
}

/// Whether a list of findings contains at least one deploy-blocking error.
fn has_errors(findings: &[PreflightFinding]) -> bool {
    findings.iter().any(PreflightFinding::is_error)
}

/// Validate a `[cron.<name>]` schedule string the way floo's own scheduler
/// will read it: floo polls due jobs with `croniter` (see
/// `api/app/services/cron_executor.py`), so the authoritative format is
/// standard cron, not Cloud Scheduler's. This check is deliberately
/// *conservative* — it rejects only what croniter would definitely reject
/// (wrong field count, garbage tokens, plainly out-of-range numbers in the
/// canonical 5-field form) so a valid schedule never produces a false error
/// that blocks a real deploy. The point is to catch the "non-cron schedule
/// string" class (e.g. `"daily"`, `"every 9am"`, a 4-field expression), not to
/// reimplement croniter.
fn is_valid_cron_schedule(expr: &str) -> bool {
    let trimmed = expr.trim();
    if trimmed.is_empty() {
        return false;
    }

    // Named macros croniter accepts.
    if let Some(rest) = trimmed.strip_prefix('@') {
        return matches!(
            rest.to_ascii_lowercase().as_str(),
            "yearly" | "annually" | "monthly" | "weekly" | "daily" | "midnight" | "hourly"
        );
    }

    let fields: Vec<&str> = trimmed.split_whitespace().collect();
    // 5 = standard (min hour dom month dow); 6/7 = croniter's optional seconds
    // and/or year extensions. Anything else is not a cron expression.
    if !(5..=7).contains(&fields.len()) {
        return false;
    }

    for (idx, field) in fields.iter().enumerate() {
        let spec = if fields.len() == 5 {
            CronFieldSpec::five_field(idx)
        } else {
            // 6/7-field exprs get the permissive spec — their extra-field
            // meanings vary by croniter config, so we only range/name-check the
            // canonical 5-field form. Over-acceptance here is the safe direction.
            CronFieldSpec::permissive()
        };
        if !is_valid_cron_field(field, spec) {
            return false;
        }
    }
    true
}

/// Names croniter accepts in the month / day-of-week fields. Whitelisted (not
/// "any 3-letter token") so `0 0 * foo *` is rejected — croniter rejects it and
/// the cron would silently never run.
const CRON_MONTH_NAMES: [&str; 12] = [
    "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
];
const CRON_DAY_NAMES: [&str; 7] = ["sun", "mon", "tue", "wed", "thu", "fri", "sat"];

/// What a single cron field position accepts. `range` bounds plain numbers;
/// `names` is the whitelist of legal alphabetic tokens (empty = none allowed,
/// `None` = permissive: any 3-letter token, used for 6/7-field forms);
/// `allow_ext` gates the `L`/`W`/`#` day-field extensions.
#[derive(Clone, Copy)]
struct CronFieldSpec {
    range: Option<(u32, u32)>,
    names: Option<&'static [&'static str]>,
    allow_ext: bool,
}

impl CronFieldSpec {
    /// Spec for position `idx` in the canonical 5-field form
    /// (minute, hour, day-of-month, month, day-of-week). Names and the
    /// `L`/`W`/`#` extensions only appear in the day-of-month, month, and
    /// day-of-week fields — never minute or hour.
    fn five_field(idx: usize) -> Self {
        match idx {
            0 => Self {
                range: Some((0, 59)),
                names: Some(&[]),
                allow_ext: false,
            }, // minute
            1 => Self {
                range: Some((0, 23)),
                names: Some(&[]),
                allow_ext: false,
            }, // hour
            2 => Self {
                range: Some((1, 31)),
                names: Some(&[]),
                allow_ext: true,
            }, // day of month (L/W)
            3 => Self {
                range: Some((1, 12)),
                names: Some(&CRON_MONTH_NAMES),
                allow_ext: false,
            }, // month
            // day of week (0 and 7 are both Sunday); names + L/# extensions.
            _ => Self {
                range: Some((0, 7)),
                names: Some(&CRON_DAY_NAMES),
                allow_ext: true,
            },
        }
    }

    fn permissive() -> Self {
        Self {
            range: None,
            names: None,
            allow_ext: true,
        }
    }
}

/// Validate one whitespace-delimited cron field. A field is a comma-separated
/// list of items; each item is `*`/`?`, a number or named token, an `a-b`
/// range, or any of those with a `/step`. `spec` carries the numeric range,
/// the name whitelist, and whether `L`/`W`/`#` extensions are legal here.
fn is_valid_cron_field(field: &str, spec: CronFieldSpec) -> bool {
    if field.is_empty() {
        return false;
    }
    field.split(',').all(|item| is_valid_cron_item(item, spec))
}

fn is_valid_cron_item(item: &str, spec: CronFieldSpec) -> bool {
    if item.is_empty() {
        return false;
    }
    // Split an optional `/step` suffix.
    let (base, step) = match item.split_once('/') {
        Some((b, s)) => (b, Some(s)),
        None => (item, None),
    };
    if let Some(step) = step {
        // Step must be a positive integer.
        if step.is_empty() || !step.chars().all(|c| c.is_ascii_digit()) {
            return false;
        }
        if step.parse::<u32>().map(|n| n == 0).unwrap_or(true) {
            return false;
        }
    }

    // `*` is valid in every field. `?` only in the day-of-month / day-of-week
    // fields (croniter rejects it elsewhere); `allow_ext` marks exactly those
    // day fields, where the `?` and `L`/`W`/`#` day-semantics are legal.
    if base == "*" {
        return true;
    }
    if base == "?" {
        return spec.allow_ext;
    }

    // Range `a-b` or a single value.
    match base.split_once('-') {
        Some((lo, hi)) => is_valid_cron_value(lo, spec) && is_valid_cron_value(hi, spec),
        None => is_valid_cron_value(base, spec),
    }
}

fn is_valid_cron_value(value: &str, spec: CronFieldSpec) -> bool {
    if value.is_empty() {
        return false;
    }
    if value.chars().all(|c| c.is_ascii_digit()) {
        let Ok(n) = value.parse::<u32>() else {
            return false;
        };
        return match spec.range {
            Some((lo, hi)) => n >= lo && n <= hi,
            None => true,
        };
    }
    // Alphabetic names. A whitelist (`Some`) accepts only the real month/day
    // tokens for that field, so junk like `foo` in the month field is rejected
    // (it would silently never run); `None` is the permissive 6/7-field path.
    if value.len() == 3 && value.chars().all(|c| c.is_ascii_alphabetic()) {
        let lower = value.to_ascii_lowercase();
        match spec.names {
            None => return true,
            Some(set) => {
                if set.contains(&lower.as_str()) {
                    return true;
                }
            }
        }
    }
    // croniter day-field extensions: `L` (last day of month/week), `W` (nearest
    // weekday), and `name#n` / `n#n` (nth weekday of month). croniter — floo's
    // scheduler — accepts and runs these, so false-rejecting one would
    // hard-block a valid deploy, the worst outcome. We accept any token built
    // from legal cron chars that carries an extension marker rather than parse
    // the (version-dependent) L/W/# grammar exactly: over-accepting a malformed
    // extension is the safe direction (it can't block a deploy that runs today —
    // a schedule croniter ultimately can't parse simply never fires).
    spec.allow_ext
        && value.chars().all(|c| c.is_ascii_alphanumeric() || c == '#')
        && value
            .chars()
            .any(|c| matches!(c, 'L' | 'W' | 'l' | 'w' | '#'))
}

fn status_label(status: &str) -> &str {
    match status {
        "pending" => "Queued...",
        "building" => "Building...",
        "deploying" => "Deploying...",
        _ => "Deploying...",
    }
}

pub fn preflight(path: PathBuf, app: Option<String>, services_filter: Vec<String>) {
    // 1. Canonicalize path
    let project_path = match path.canonicalize() {
        Ok(p) => p,
        Err(_) => {
            output::error(
                &format!("Path '{}' is not a directory.", path.display()),
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

    // 2. Resolve app context
    let resolved = match project_config::resolve_app_context(&project_path, app.as_deref()) {
        Ok(r) => r,
        Err(e) if e.code == ErrorCode::NoConfigFound => {
            output::error(
                "No floo.app.toml or floo.service.toml found.",
                &ErrorCode::NoConfigFound,
                Some("Run 'floo init' to create config files, then 'floo apps github connect <repo>' to connect to GitHub."),
            );
            process::exit(1);
        }
        Err(e) => {
            output::error(&e.message, &e.code, e.suggestion.as_deref());
            process::exit(1);
        }
    };

    let app_name = resolved.app_name.clone();

    // 3. Discover + filter services
    let all_services = match project_config::discover_services(&resolved) {
        Ok(svcs) => svcs,
        Err(e) => {
            output::error(&e.message, &e.code, e.suggestion.as_deref());
            process::exit(1);
        }
    };
    // Full declared service set — cron `service` references validate against
    // this, not the (possibly `--services`-filtered) subset below.
    let all_service_names: Vec<String> = all_services.iter().map(|s| s.name.clone()).collect();
    let services = match project_config::filter_services(all_services, &services_filter) {
        Ok(svcs) => svcs,
        Err(e) => {
            output::error(&e.message, &e.code, e.suggestion.as_deref());
            process::exit(1);
        }
    };

    let managed_services = project_config::discover_managed_services(&resolved);

    // 4. Per-service runtime detection
    let svc_pairs: Vec<(&str, &str)> = services
        .iter()
        .map(|s| (s.name.as_str(), s.path.as_str()))
        .collect();
    let (_primary_detection, per_service_detection) =
        detect_for_services(&project_path, &svc_pairs);

    // 5. Validate (env plan first — the build/run checks read it).
    let env_injection_plan = build_env_injection_plan(&services, &managed_services, &resolved);
    let validation_findings = validate_preflight(
        &services,
        &all_service_names,
        &resolved,
        &managed_services,
        &env_injection_plan,
    );
    let security_findings = generate_security_findings(&services, &resolved, &env_injection_plan);

    let valid = !has_errors(&validation_findings);
    let contains_secrets = security_findings
        .iter()
        .any(|f| f.code == "SECRET_IN_WEB_SERVICE");

    // All findings, build/run first then security, as one typed list.
    let mut all_findings = validation_findings.clone();
    all_findings.extend(security_findings.iter().cloned());

    // 7. Remote preflight audit (declared vs deployed). Best-effort — auth/resolution
    // failures degrade to a note; local validation still ships.
    let remote_plan = fetch_remote_preflight(&app_name, &managed_services, &resolved.config_dir);

    // 8. Display
    if output::is_json_mode() {
        let svc_json: Vec<serde_json::Value> = services
            .iter()
            .zip(per_service_detection.iter())
            .map(|(svc, (_, det))| {
                serde_json::json!({
                    "name": svc.name,
                    "path": svc.path,
                    "port": svc.port,
                    "type": svc.service_type.to_string(),
                    "ingress": svc.ingress.to_string(),
                    "runtime": det.runtime,
                    "framework": det.framework,
                    "confidence": det.confidence,
                })
            })
            .collect();

        let managed_json: Vec<serde_json::Value> = managed_services
            .iter()
            .map(|ms| {
                serde_json::json!({
                    "name": ms.name,
                    "tier": ms.tier.as_deref().unwrap_or("basic"),
                })
            })
            .collect();

        let cron_json = cron_json(&resolved);

        let data = serde_json::json!({
            "app": app_name,
            "services": svc_json,
            "managed_services": managed_json,
            "env_injection_plan": env_injection_plan,
            "cron": cron_json,
            "findings": all_findings,
            "plan": remote_plan.as_ref().map(crate::output::to_value),
            "valid": valid,
        });

        // One JSON object, always. On an invalid config the payload is
        // error-shaped (`success:false` + `error`) but still carries the full
        // `data` so agents read findings either way — the prior code emitted
        // BOTH a success and an error object, breaking JSON parsing.
        let mut payload = if valid {
            serde_json::json!({ "success": true, "data": data })
        } else {
            let count = all_findings.iter().filter(|f| f.is_error()).count();
            serde_json::json!({
                "success": false,
                "error": {
                    "code": ErrorCode::ConfigInvalid.as_str(),
                    "message": format!("{count} preflight error(s) found."),
                    "suggestion": "Fix the errors above and run `floo preflight` to re-validate.",
                },
                "data": data,
            })
        };
        if contains_secrets {
            if let Some(map) = payload.as_object_mut() {
                map.insert(
                    crate::redact::CONTAINS_SECRETS_KEY.to_string(),
                    serde_json::Value::Bool(true),
                );
            }
        }
        output::print_json(&payload);

        if !valid {
            process::exit(1);
        }
        return;
    }

    // --- Human output ---
    display_preflight_human(
        &app_name,
        &resolved,
        &services,
        &per_service_detection,
        &env_injection_plan,
        &validation_findings,
    );

    if !managed_services.is_empty() {
        eprintln!("  Managed services (declared):");
        for ms in &managed_services {
            let tier_label = ms.tier.as_deref().unwrap_or("basic");
            eprintln!("    {} (tier {tier_label})", ms.name);
        }
        eprintln!();
    }

    if let Some(ref plan) = remote_plan {
        render_plan_human(plan);
    }

    render_security_findings_human(&security_findings);

    if !valid {
        for f in all_findings.iter().filter(|f| f.is_error()) {
            eprintln!("  {} {}", Severity::Error.glyph(), f.message);
        }
        let count = all_findings.iter().filter(|f| f.is_error()).count();
        output::error(
            &format!("{count} preflight error(s) found."),
            &ErrorCode::ConfigInvalid,
            Some("Fix the errors above and run `floo preflight` to re-validate."),
        );
        process::exit(1);
    }

    print_preflight_ready(&all_findings);
}

/// Serialize declared `[cron.<name>]` entries for the JSON payload. Preflight
/// previously omitted cron entirely; agents had no way to see them.
fn cron_json(resolved: &project_config::ResolvedApp) -> Vec<serde_json::Value> {
    let Some(app_cfg) = resolved.app_config.as_ref() else {
        return Vec::new();
    };
    let mut names: Vec<&String> = app_cfg.cron.keys().collect();
    names.sort();
    names
        .into_iter()
        .map(|name| {
            let cfg = &app_cfg.cron[name];
            serde_json::json!({
                "name": name,
                "schedule": cfg.schedule,
                "command": cfg.command,
                "service": cfg.service,
                "timeout": cfg.timeout.unwrap_or(300),
            })
        })
        .collect()
}

pub fn deploy(
    path: PathBuf,
    app: Option<String>,
    services_filter: Vec<String>,
    rebuild: bool,
    sync_env: bool,
    skip_migrations: bool,
) {
    // Restart path reuses the existing image and never runs migrations,
    // so `--skip-migrations` only makes sense on a rebuild path. Reject
    // the combination loudly rather than silently dropping the flag.
    if skip_migrations && app.is_some() && !rebuild {
        output::error(
            "--skip-migrations requires --rebuild.",
            &ErrorCode::ConfigInvalid,
            Some(
                "Restart paths reuse the existing image and don't run migrations,\
                 so --skip-migrations has no effect there. Add --rebuild or drop the flag.",
            ),
        );
        process::exit(1);
    }

    // --- Path 1 & 2: --app flag provided — no local directory needed ---
    if let Some(ref app_name) = app {
        // Dry-run exits early — no auth or API calls needed
        if output::is_dry_run_mode() {
            let action = if rebuild { "rebuild" } else { "restart" };
            let service_names: Vec<&str> = services_filter.iter().map(|s| s.as_str()).collect();
            let svc_clause = if service_names.is_empty() {
                String::new()
            } else {
                format!(" (services: {})", service_names.join(", "))
            };
            let mig_clause = if skip_migrations {
                " (skip migrations)"
            } else {
                ""
            };
            let preview = format!("Would {action} app '{app_name}'{svc_clause}{mig_clause}.");
            output::dry_run_preview(
                &preview,
                serde_json::json!({
                    "action": action,
                    "app": app_name,
                    "services": service_names,
                    "skip_migrations": skip_migrations,
                }),
            );
            return;
        }

        let config = load_config();
        if config.api_key.is_none() {
            output::error(
                "Not logged in.",
                &ErrorCode::NotAuthenticated,
                Some("Run 'floo auth login' to authenticate."),
            );
            process::exit(1);
        }

        let client = super::init_client(Some(config));
        let app_data = match resolve_app(&client, app_name) {
            Ok(a) => a,
            Err(e) => {
                // 404 from resolving the app == app not found; gate on status,
                // not a code string that can drift (see is_not_found).
                if e.is_not_found() {
                    output::error(
                        &format!("App '{app_name}' not found."),
                        &ErrorCode::AppNotFound,
                        Some("Check the app name or ID and try again."),
                    );
                } else {
                    output::error(&e.message, &ErrorCode::from_api(&e.code), None);
                }
                process::exit(1);
            }
        };

        if rebuild {
            deploy_rebuild(&client, &app_data, &services_filter, skip_migrations);
        } else {
            deploy_restart(&client, &app_data, &services_filter);
        }
        return;
    }

    // --- Path 3: No --app flag — full preflight from local project directory ---

    // ===== Deploy preflight (no auth required) =====

    // 1. Canonicalize path
    let project_path = match path.canonicalize() {
        Ok(p) => p,
        Err(_) => {
            output::error(
                &format!("Path '{}' is not a directory.", path.display()),
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

    // 2. Resolve app context
    let resolved = match project_config::resolve_app_context(&project_path, app.as_deref()) {
        Ok(r) => r,
        Err(e) if e.code == ErrorCode::NoConfigFound => {
            output::error(
                "No floo.app.toml or floo.service.toml found.",
                &ErrorCode::NoConfigFound,
                Some("Run 'floo init' to create config files, then 'floo apps github connect <repo>' to connect to GitHub."),
            );
            process::exit(1);
        }
        Err(e) => {
            output::error(&e.message, &e.code, e.suggestion.as_deref());
            process::exit(1);
        }
    };

    let app_name = resolved.app_name.clone();

    // 3. Discover + filter services
    let all_services = match project_config::discover_services(&resolved) {
        Ok(svcs) => svcs,
        Err(e) => {
            output::error(&e.message, &e.code, e.suggestion.as_deref());
            process::exit(1);
        }
    };
    let all_service_names: Vec<String> = all_services.iter().map(|s| s.name.clone()).collect();
    let services = match project_config::filter_services(all_services, &services_filter) {
        Ok(svcs) => svcs,
        Err(e) => {
            output::error(&e.message, &e.code, e.suggestion.as_deref());
            process::exit(1);
        }
    };

    // 3b. Discover managed service declarations (postgres, redis, etc.)
    let managed_services = project_config::discover_managed_services(&resolved);

    // 4. Per-service runtime detection
    let svc_pairs: Vec<(&str, &str)> = services
        .iter()
        .map(|s| (s.name.as_str(), s.path.as_str()))
        .collect();
    let (primary_detection, per_service_detection) = detect_for_services(&project_path, &svc_pairs);

    // 5. Validate per-service (env plan first — the build/run checks read it).
    let env_injection_plan = build_env_injection_plan(&services, &managed_services, &resolved);
    let validation_findings = validate_preflight(
        &services,
        &all_service_names,
        &resolved,
        &managed_services,
        &env_injection_plan,
    );
    let valid = !has_errors(&validation_findings);

    // 6. Display preflight info
    if !output::is_json_mode() {
        display_preflight_human(
            &app_name,
            &resolved,
            &services,
            &per_service_detection,
            &env_injection_plan,
            &validation_findings,
        );

        if !managed_services.is_empty() {
            eprintln!("  Managed services:");
            for ms in &managed_services {
                let tier_label = ms.tier.as_deref().unwrap_or("basic");
                eprintln!("    {} (tier {tier_label})", ms.name);
            }
            eprintln!();
        }
    }

    // 7. Dry-run exit — full preflight output, no auth needed
    if output::is_dry_run_mode() {
        let svc_json: Vec<serde_json::Value> = services
            .iter()
            .zip(per_service_detection.iter())
            .map(|(svc, (_, det))| {
                serde_json::json!({
                    "name": svc.name,
                    "path": svc.path,
                    "port": svc.port,
                    "type": svc.service_type.to_string(),
                    "ingress": svc.ingress.to_string(),
                    "runtime": det.runtime,
                    "framework": det.framework,
                    "confidence": det.confidence,
                })
            })
            .collect();

        let managed_json: Vec<serde_json::Value> = managed_services
            .iter()
            .map(|ms| {
                serde_json::json!({
                    "name": ms.name,
                    "tier": ms.tier.as_deref().unwrap_or("basic"),
                })
            })
            .collect();

        // Preflight already printed the human-friendly service table via
        // display_preflight_human() above (gated on !is_json_mode). Keep the
        // preview line tight so we don't duplicate it.
        let svc_count = services.len();
        let preview = format!(
            "Would deploy app '{app_name}' with {svc_count} service(s){}.",
            if valid {
                ""
            } else {
                "; preflight errors must be fixed first"
            }
        );
        output::dry_run_preview(
            &preview,
            serde_json::json!({
                "action": "deploy",
                "app": app_name,
                "services": svc_json,
                "managed_services": managed_json,
                "env_injection_plan": env_injection_plan,
                "cron": cron_json(&resolved),
                "findings": validation_findings,
                "valid": valid,
            }),
        );
        return;
    }

    // 8. Auth check — only needed for actual deploy
    let config = load_config();
    if config.api_key.is_none() {
        output::error(
            "Not logged in.",
            &ErrorCode::NotAuthenticated,
            Some("Run 'floo auth login' to authenticate."),
        );
        process::exit(1);
    }

    // 9. Fail if preflight has errors
    if !valid {
        for f in validation_findings.iter().filter(|f| f.is_error()) {
            if !output::is_json_mode() {
                eprintln!("  {} {}", Severity::Error.glyph(), f.message);
            }
        }
        let count = validation_findings.iter().filter(|f| f.is_error()).count();
        output::error(
            &format!("{count} preflight error(s) found."),
            &ErrorCode::ConfigInvalid,
            Some("Fix the errors above and run `floo preflight` to validate."),
        );
        process::exit(1);
    }

    // Use primary detection for API call metadata
    let detection = primary_detection;

    let client = super::init_client(Some(config));

    // Resolve or create app via API.
    //
    // Path 3 only runs without --app — the early Some(app) block (Path 1 & 2)
    // always returns first — so the app is resolved from local config here:
    // look it up by name and create it if the API reports it doesn't exist yet.
    let spinner = output::Spinner::new(&format!("Looking up app {}...", resolved.app_name));
    let app_data = match resolve_app(&client, &resolved.app_name) {
        Ok(app_data) => {
            spinner.finish();
            app_data
        }
        // 404 == app doesn't exist yet → create it (gate on status, not a
        // drift-prone code string; see is_not_found). resolve_app hits
        // GET /v1/apps/{id} for UUID-shaped names, so the server's own code
        // flows through verbatim and a code-string match would miss a 404
        // carrying anything other than APP_NOT_FOUND.
        Err(error) if error.is_not_found() => {
            spinner.finish();
            let spinner = output::Spinner::new(&format!("Creating app {}...", resolved.app_name));
            match client.create_app(&resolved.app_name, Some(&detection.runtime)) {
                Ok(a) => {
                    spinner.finish();
                    a
                }
                Err(e) => {
                    spinner.finish();
                    output::error(&e.message, &ErrorCode::from_api(&e.code), None);
                    process::exit(1);
                }
            }
        }
        Err(error) => {
            spinner.finish();
            output::error(&error.message, &ErrorCode::from_api(&error.code), None);
            process::exit(1);
        }
    };
    let app_id = app_data.id.clone();

    // Auto-import env vars on first deploy (or force with --sync-env)
    sync_env_vars_if_needed(&client, &app_id, &resolved, sync_env);

    // Extract access_mode: [environments.dev] override > [app] level > service_config
    let access_mode: Option<AppAccessMode> = resolved
        .app_config
        .as_ref()
        .and_then(|c| {
            c.environments
                .get("dev")
                .and_then(|env| env.access_mode)
                .or(c.app.access_mode)
        })
        .or_else(|| {
            resolved
                .service_config
                .as_ref()
                .and_then(|c| c.app.access_mode)
        });

    // Extract agent_mode from [app] section
    let agent_mode: Option<AppAgentMode> =
        resolved.app_config.as_ref().and_then(|c| c.app.agent_mode);

    // Extract auth redirect URIs from [auth] toml section
    let auth_redirect_uris: Option<Vec<String>> = resolved
        .app_config
        .as_ref()
        .and_then(|c| c.auth.as_ref())
        .and_then(|auth| auth.redirect_uris.clone());

    // Extract reparo config from [reparo] toml section
    let reparo_config = resolved.app_config.as_ref().and_then(|c| c.reparo.as_ref());

    // Extract cron job definitions from [cron] toml section
    let cron_entries: Vec<crate::project_config::CronJobEntry> = resolved
        .app_config
        .as_ref()
        .map(|c| {
            c.cron
                .iter()
                .map(|(name, cfg)| crate::project_config::CronJobEntry {
                    name: name.clone(),
                    schedule: cfg.schedule.clone(),
                    command: cfg.command.clone(),
                    service: cfg.service.clone(),
                    timeout: cfg.timeout.unwrap_or(300),
                })
                .collect()
        })
        .unwrap_or_default();
    let cron_jobs_arg = if cron_entries.is_empty() {
        None
    } else {
        Some(cron_entries.as_slice())
    };

    // Extract [github] config
    let github_config = resolved.app_config.as_ref().and_then(|c| c.github.as_ref());

    // Deploy
    let svc_slice = Some(services.as_slice());
    let spinner = output::Spinner::new("Deploying...");
    let mut deploy_data = match client.create_deploy(
        &app_id,
        &detection.runtime,
        detection.framework.as_deref(),
        svc_slice,
        access_mode.as_ref().map(|m| m.as_str()),
        agent_mode.as_ref().map(|m| m.as_str()),
        auth_redirect_uris.as_deref(),
        reparo_config,
        cron_jobs_arg,
        github_config,
        skip_migrations,
    ) {
        Ok(d) => {
            spinner.finish();
            d
        }
        Err(e) => {
            spinner.finish();
            let suggestion = match e.code.as_str() {
                "PLAN_FEATURE_PASSWORD" | "PLAN_FEATURE_ACCOUNTS" | "PLAN_FEATURE_SSO" => {
                    Some("Upgrade your plan at https://app.getfloo.com/settings/billing")
                }
                _ => None,
            };
            output::error(&e.message, &ErrorCode::from_api(&e.code), suggestion);
            process::exit(1);
        }
    };

    // Wait for deploy to complete via SSE streaming or polling
    let initial_status = deploy_data.status.as_deref().unwrap_or("");

    if deploy_status::is_terminal(initial_status) {
        // Phase 1: deploy already complete synchronously, skip streaming/polling
    } else if !output::is_json_mode() {
        // Phase 2 human mode: try SSE streaming, fall back to polling
        let deploy_id = deploy_data.id.clone();
        match stream_deploy(&client, &app_id, &deploy_id) {
            Ok(final_data) => deploy_data = final_data,
            Err(e) => {
                // SSE failed — fall back to polling
                eprintln!(
                    "Stream unavailable ({}), falling back to polling...",
                    e.code
                );
                deploy_data = poll_deploy(&client, &app_id, &deploy_data);
            }
        }
    } else {
        // Phase 2 JSON mode: stream structured NDJSON events via SSE
        let deploy_id = deploy_data.id.clone();
        match stream_deploy_json(&client, &app_id, &deploy_id) {
            Ok(final_data) => deploy_data = final_data,
            Err(_) => deploy_data = poll_deploy(&client, &app_id, &deploy_data),
        }
    }

    let final_status = deploy_data.status.as_deref().unwrap_or("");

    if deploy_status::is_failure(final_status) {
        let build_logs = deploy_data.build_logs.as_deref().unwrap_or("");
        if !output::is_json_mode() && !build_logs.is_empty() && build_logs != "[no message content]"
        {
            output::bold_line("Build Logs");
            for line in build_logs.lines() {
                output::dim_line(line);
            }
        }
        output::error_with_data(
            "Deploy failed.",
            &ErrorCode::DeployFailed,
            Some("Check build output above, or run `floo logs` for details."),
            Some(serde_json::json!({
                "app": output::to_value(&app_data),
                "deploy": output::to_value(&deploy_data),
                "build_logs": build_logs,
            })),
        );
        process::exit(1);
    }

    if final_status == "superseded" {
        output::success(
            "Deploy superseded by a newer deploy.",
            Some(serde_json::json!({
                "app": output::to_value(&app_data),
                "deploy": output::to_value(&deploy_data),
                "detection": detection.to_value(),
            })),
        );
        return;
    }

    if final_status == "cancelled" {
        output::success(
            "Deploy cancelled: its target environment was removed before it ran.",
            Some(serde_json::json!({
                "app": output::to_value(&app_data),
                "deploy": output::to_value(&deploy_data),
                "detection": detection.to_value(),
            })),
        );
        return;
    }

    let url = deploy_data.url.as_deref().unwrap_or("");

    if !output::is_json_mode() {
        if let Some(ref password) = deploy_data.generated_password {
            output::info(&format!("  Generated password: {password}"), None);
            output::info("  To retrieve later: floo apps password <name>", None);
        }
        if let Some(ref mode) = access_mode {
            output::info(&format!("  Access: {}", mode.as_str()), None);
        }
        // Closes feedback c9b70eb5 — surface the auto-deploy contract the
        // moment a manual `floo deploy` finishes, so the user knows the
        // next change ships via `git push` (no need to remember `floo deploy`).
        // The hint only renders for human terminals; JSON consumers infer
        // from the connected service in the response.
        let app_url = format!("https://app.getfloo.com/{}", app_data.name);
        output::info(
            &format!("  Next deploys: push to your default branch. Manage at {app_url}"),
            None,
        );
    }

    let service_names: Vec<&str> = services.iter().map(|s| s.name.as_str()).collect();

    let env_display = deploy_data
        .environment_name
        .as_deref()
        .map(|e| format!("{e} \u{2192} "))
        .unwrap_or_default();
    output::success(
        &format!("Deployed to {env_display}{url}"),
        Some(serde_json::json!({
            "app": output::to_value(&app_data),
            "deploy": output::to_value(&deploy_data),
            "detection": detection.to_value(),
            "services": service_names,
        })),
    );
}

/// Redeploy existing images with fresh env vars (no build). Used when `--app` is
/// provided without `--rebuild`.
fn deploy_restart(
    client: &FlooClient,
    app_data: &crate::api_types::App,
    services_filter: &[String],
) {
    let app_id = &app_data.id;

    if output::is_dry_run_mode() {
        let service_names: Vec<&str> = services_filter.iter().map(|s| s.as_str()).collect();
        let svc_clause = if service_names.is_empty() {
            String::new()
        } else {
            format!(" (services: {})", service_names.join(", "))
        };
        let preview = format!("Would restart app '{}'{svc_clause}.", app_data.name);
        output::dry_run_preview(
            &preview,
            serde_json::json!({
                "action": "restart",
                "app": app_data.name,
                "services": service_names,
            }),
        );
        return;
    }

    let svcs: Option<&[String]> = if services_filter.is_empty() {
        None
    } else {
        Some(services_filter)
    };

    let spinner = output::Spinner::new("Restarting...");
    let raw_deploy = match client.restart_app(app_id, svcs) {
        Ok(d) => {
            spinner.finish();
            d
        }
        Err(e) => {
            spinner.finish();
            output::error(&e.message, &ErrorCode::from_api(&e.code), None);
            process::exit(1);
        }
    };

    // The restart endpoint returns 202 with a DeployResponse — the pipeline
    // runs out-of-band, so the status is typically "pending" at this point.
    // Match the main deploy path and wait for a terminal status before
    // reporting back. Otherwise `floo redeploy --json` returns while the
    // deploy is still in progress and agents have to poll manually. See
    // feedback 966b2a4a.
    let mut deploy_data: Deploy = match serde_json::from_value(raw_deploy.clone()) {
        Ok(d) => d,
        Err(_) => {
            // Server response didn't match the Deploy shape — surface what we
            // got instead of silently pretending restart succeeded.
            output::error_with_data(
                "Restart returned an unexpected response shape.",
                &ErrorCode::RestartFailed,
                Some("Run `floo deploys list --app <name>` to check deploy status."),
                Some(serde_json::json!({
                    "app": output::to_value(app_data),
                    "deploy": raw_deploy,
                })),
            );
            process::exit(1);
        }
    };

    let initial_status = deploy_data.status.as_deref().unwrap_or("");
    if !deploy_status::is_terminal(initial_status) {
        let deploy_id = deploy_data.id.clone();
        deploy_data = if !output::is_json_mode() {
            match stream_deploy(client, app_id, &deploy_id) {
                Ok(d) => d,
                Err(e) => {
                    eprintln!(
                        "Stream unavailable ({}), falling back to polling...",
                        e.code
                    );
                    poll_deploy(client, app_id, &deploy_data)
                }
            }
        } else {
            match stream_deploy_json(client, app_id, &deploy_id) {
                Ok(d) => d,
                Err(_) => poll_deploy(client, app_id, &deploy_data),
            }
        };
    }

    let final_status = deploy_data.status.as_deref().unwrap_or("");
    let url = deploy_data.url.as_deref().unwrap_or("(no URL)");

    if deploy_status::is_failure(final_status) {
        output::error_with_data(
            "Restart failed.",
            &ErrorCode::RestartFailed,
            Some("Run `floo logs` for details."),
            Some(serde_json::json!({
                "app": output::to_value(app_data),
                "deploy": output::to_value(&deploy_data),
            })),
        );
        process::exit(1);
    }

    if final_status == "superseded" {
        output::success(
            "Restart superseded by a newer deploy.",
            Some(serde_json::json!({
                "app": output::to_value(app_data),
                "deploy": output::to_value(&deploy_data),
            })),
        );
        return;
    }

    if final_status == "cancelled" {
        output::success(
            "Restart cancelled: its target environment was removed before it ran.",
            Some(serde_json::json!({
                "app": output::to_value(app_data),
                "deploy": output::to_value(&deploy_data),
            })),
        );
        return;
    }

    let env_display = deploy_data
        .environment_name
        .as_deref()
        .map(|e| format!("{e} \u{2192} "))
        .unwrap_or_default();
    output::success(
        &format!("Restarted {env_display}{url}"),
        Some(serde_json::json!({
            "app": output::to_value(app_data),
            "deploy": output::to_value(&deploy_data),
        })),
    );
}

/// Force a full rebuild from the latest commit. Used when `--app --rebuild` is
/// provided — no local project directory needed.
fn deploy_rebuild(
    client: &FlooClient,
    app_data: &crate::api_types::App,
    services_filter: &[String],
    skip_migrations: bool,
) {
    let app_id = &app_data.id;
    let runtime = app_data.runtime.as_deref().unwrap_or("unknown");

    if output::is_dry_run_mode() {
        let service_names: Vec<&str> = services_filter.iter().map(|s| s.as_str()).collect();
        let svc_clause = if service_names.is_empty() {
            String::new()
        } else {
            format!(" (services: {})", service_names.join(", "))
        };
        let mig_clause = if skip_migrations {
            " (skip migrations)"
        } else {
            ""
        };
        let preview = format!(
            "Would rebuild app '{}' (runtime: {runtime}){svc_clause}{mig_clause}.",
            app_data.name
        );
        output::dry_run_preview(
            &preview,
            serde_json::json!({
                "action": "rebuild",
                "app": app_data.name,
                "runtime": runtime,
                "services": service_names,
                "skip_migrations": skip_migrations,
            }),
        );
        return;
    }

    let svcs: Option<&[String]> = if services_filter.is_empty() {
        None
    } else {
        Some(services_filter)
    };

    let spinner = output::Spinner::new("Rebuilding...");
    let mut deploy_data = match client.rebuild_app(app_id, runtime, svcs, skip_migrations) {
        Ok(d) => {
            spinner.finish();
            d
        }
        Err(e) => {
            spinner.finish();
            let suggestion = match e.code.as_str() {
                "PLAN_FEATURE_PASSWORD" | "PLAN_FEATURE_ACCOUNTS" | "PLAN_FEATURE_SSO" => {
                    Some("Upgrade your plan at https://app.getfloo.com/settings/billing")
                }
                _ => None,
            };
            output::error(&e.message, &ErrorCode::from_api(&e.code), suggestion);
            process::exit(1);
        }
    };

    // Wait for deploy to complete via SSE streaming or polling
    let initial_status = deploy_data.status.as_deref().unwrap_or("");

    if deploy_status::is_terminal(initial_status) {
        // Already complete
    } else if !output::is_json_mode() {
        let deploy_id = deploy_data.id.clone();
        match stream_deploy(client, app_id, &deploy_id) {
            Ok(final_data) => deploy_data = final_data,
            Err(e) => {
                eprintln!(
                    "Stream unavailable ({}), falling back to polling...",
                    e.code
                );
                deploy_data = poll_deploy(client, app_id, &deploy_data);
            }
        }
    } else {
        let deploy_id = deploy_data.id.clone();
        match stream_deploy_json(client, app_id, &deploy_id) {
            Ok(final_data) => deploy_data = final_data,
            Err(_) => deploy_data = poll_deploy(client, app_id, &deploy_data),
        }
    }

    let final_status = deploy_data.status.as_deref().unwrap_or("");

    if deploy_status::is_failure(final_status) {
        let build_logs = deploy_data.build_logs.as_deref().unwrap_or("");
        if !output::is_json_mode() && !build_logs.is_empty() && build_logs != "[no message content]"
        {
            output::bold_line("Build Logs");
            for line in build_logs.lines() {
                output::dim_line(line);
            }
        }
        output::error_with_data(
            "Rebuild failed.",
            &ErrorCode::DeployFailed,
            Some("Check build output above, or run `floo logs` for details."),
            Some(serde_json::json!({
                "app": output::to_value(app_data),
                "deploy": output::to_value(&deploy_data),
                "build_logs": build_logs,
            })),
        );
        process::exit(1);
    }

    if final_status == "superseded" {
        output::success(
            "Rebuild superseded by a newer deploy.",
            Some(serde_json::json!({
                "app": output::to_value(app_data),
                "deploy": output::to_value(&deploy_data),
            })),
        );
        return;
    }

    if final_status == "cancelled" {
        output::success(
            "Rebuild cancelled: its target environment was removed before it ran.",
            Some(serde_json::json!({
                "app": output::to_value(app_data),
                "deploy": output::to_value(&deploy_data),
            })),
        );
        return;
    }

    let url = deploy_data.url.as_deref().unwrap_or("");
    output::success(
        &format!("Rebuilt and deployed {url}"),
        Some(serde_json::json!({
            "app": output::to_value(app_data),
            "deploy": output::to_value(&deploy_data),
        })),
    );
}

fn service_looks_like_rails(service_dir: &Path) -> bool {
    let gemfile = service_dir.join("Gemfile");
    if let Ok(contents) = std::fs::read_to_string(&gemfile) {
        let lower = contents.to_lowercase();
        if lower.contains("gem \"rails\"") || lower.contains("gem 'rails'") {
            return true;
        }
    }

    let app_config = service_dir.join("config").join("application.rb");
    if let Ok(contents) = std::fs::read_to_string(app_config) {
        return contents.contains("Rails::Application") || contents.contains("require \"rails\"");
    }

    false
}

fn env_files_for_service(
    service_dir: &Path,
    configured_env_file: Option<&str>,
) -> Vec<(String, PathBuf)> {
    let mut files: Vec<(String, PathBuf)> = Vec::new();
    let mut labels: Vec<String> = Vec::new();
    if let Some(env_file) = configured_env_file {
        labels.push(env_file.to_string());
    }
    labels.extend(
        [".env", ".env.local", ".env.production", ".env.development"]
            .iter()
            .map(|label| label.to_string()),
    );

    for label in labels {
        if files.iter().any(|(existing, _)| existing == &label) {
            continue;
        }
        files.push((label.clone(), service_dir.join(label)));
    }
    files
}

/// Parse one env-file line into its raw `(key, value)`, or `None` for a blank
/// or comment line. THE single definition of how floo reads an env-file line:
/// both the preflight inspection paths and the deploy-sync import path
/// (`parse_env_file_soft`) call this, so they agree byte-for-byte on what a line
/// declares. Trims, skips `#` comments, strips a leading `export ` (plus any
/// extra whitespace), splits on the first `=`, and strips ONE matching pair of
/// surrounding quotes from the value. Keys are returned verbatim — callers that
/// need the deployed form uppercase them, since deploy-sync uppercases on import.
fn parse_env_line(line: &str) -> Option<(&str, &str)> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return None;
    }
    let trimmed = trimmed
        .strip_prefix("export ")
        .unwrap_or(trimmed)
        .trim_start();
    let (key, value) = trimmed.split_once('=')?;
    Some((key.trim(), strip_matching_quotes(value.trim())))
}

/// Strip ONE matching pair of surrounding single or double quotes, if present.
/// Mirrors deploy-sync's value handling; the `len() >= 2` guard also avoids the
/// `value[1..0]` panic a lone `"` would otherwise trigger.
fn strip_matching_quotes(value: &str) -> &str {
    let bytes = value.as_bytes();
    if bytes.len() >= 2 {
        let first = bytes[0];
        let last = bytes[bytes.len() - 1];
        if (first == b'"' && last == b'"') || (first == b'\'' && last == b'\'') {
            return &value[1..value.len() - 1];
        }
    }
    value
}

fn is_cloudsql_socket_database_url(value: &str) -> bool {
    let lower = value.to_lowercase();
    lower.contains("@/")
        && (lower.contains("/cloudsql/")
            || lower.contains("host=/cloudsql")
            || lower.contains("%2fcloudsql%2f")
            || lower.contains("host=%2fcloudsql"))
}

/// Validate services for common config errors, returning typed findings.
///
/// `services` is the (possibly `--services`-filtered) set being validated;
/// `all_service_names` is the full declared set, used for cron `service`
/// references so a `--services` filter doesn't make a valid cron look broken.
/// `env_plan` carries the per-service managed-injection keys + declared
/// required/optional contract, which the required-env and migrate checks read.
///
/// Absorbs the validation logic that was previously in `floo check`.
///
/// Service-local paths resolve against `resolved.config_dir` (where the config
/// lives), NOT a user-supplied path: `resolve_app_context` walks up from the
/// invocation directory, so for `floo preflight ./sub` the service `path`s are
/// relative to the config dir, which may be an ancestor. Using the invocation
/// path would false-error `SERVICE_PATH_NOT_FOUND` on a valid monorepo service.
fn validate_preflight(
    services: &[ServiceConfig],
    all_service_names: &[String],
    resolved: &project_config::ResolvedApp,
    managed_services: &[project_config::ManagedServiceDeclaration],
    env_plan: &EnvInjectionPlan,
) -> Vec<PreflightFinding> {
    let mut findings: Vec<PreflightFinding> = Vec::new();
    let mut seen_names: Vec<String> = Vec::new();
    let has_managed_postgres = managed_services.iter().any(|ms| ms.name == "postgres");

    // The single canonical set of env files the deploy imports — shared with
    // `sync_env_vars_if_needed` (an inline floo.app.toml env_file and
    // external-repo services are NOT in it, matching what deploy imports).
    let imported_env_file_list = deploy_imported_env_files(resolved);

    // Validate every imported env_file with the deploy's OWN validator over the
    // FULL (unfiltered) set: `sync_env_vars_if_needed` runs `validate_env_file_path`
    // across this exact set and `process::exit(1)`s on the first failure,
    // unconditionally and regardless of `--services`. So a single invalid
    // imported env_file is a guaranteed deploy rejection — a hard error here,
    // even when validating a `--services` subset that excludes that service.
    for (svc_name, env_file, base) in &imported_env_file_list {
        if let Err(msg) = super::env::validate_env_file_path(env_file, base) {
            findings.push(
                PreflightFinding::error(
                    "ENV_FILE_INVALID",
                    format!("Service '{svc_name}' env_file is invalid: {msg}"),
                )
                .with_hint(
                    "env_file must be a relative path inside the service that exists on disk.",
                ),
            );
        }
    }

    // Keyed by service name for the per-service required-env/migrate satisfaction
    // lookups below.
    let imported_env_files: std::collections::HashMap<String, (String, PathBuf)> =
        imported_env_file_list
            .into_iter()
            .map(|(name, env_file, base)| (name, (env_file, base)))
            .collect();

    for svc in services {
        // Validate service name
        if let Err(msg) = validate_service_name(&svc.name) {
            findings
                .push(PreflightFinding::error("INVALID_SERVICE_NAME", msg).with_path(&svc.path));
        }

        // Check for duplicate names
        if seen_names.contains(&svc.name) {
            findings.push(
                PreflightFinding::error(
                    "DUPLICATE_SERVICE_NAME",
                    format!("Duplicate service name '{}'.", svc.name),
                )
                .with_path(&svc.path),
            );
        } else {
            seen_names.push(svc.name.clone());
        }

        // Validate port
        if svc.port == 0 {
            findings.push(
                PreflightFinding::error(
                    "INVALID_PORT",
                    format!(
                        "Service '{}' has invalid port 0. Ports must be 1-65535.",
                        svc.name
                    ),
                )
                .with_path(&svc.path),
            );
        }

        // Managed-injection keys this service receives (DATABASE_URL, REDIS_URL,
        // …) — config-only, derived from the env injection plan, no disk access.
        let managed_keys = managed_keys_for_service(env_plan, &svc.name);

        // A service sourced from an external `repo` builds from THAT repo, not
        // the local checkout — its `path`/Dockerfile/env files live in the
        // referenced repo, so the local-disk checks below don't apply and the
        // local env-file-based required-env check would be a false-positive
        // flood (we can't see the external repo's env). But a `migrate_command`
        // still needs a database floo provides — a config-only check (no disk) —
        // so it runs here (with an empty local env_file set, since an
        // external-repo service imports no local env file) before we skip the
        // rest.
        if service_is_external_repo(resolved, &svc.name) {
            check_migrate_command_no_database(
                svc,
                &managed_keys,
                &std::collections::HashSet::new(),
                &mut findings,
            );
            continue;
        }

        let svc_dir = resolved.config_dir.join(&svc.path);

        // The service path is the Cloud Build context. If it doesn't exist on
        // disk the build can't even start, so this is a hard error — and there
        // is no point running the disk-dependent checks below against a
        // missing directory.
        if !svc_dir.is_dir() {
            findings.push(
                PreflightFinding::error(
                    "SERVICE_PATH_NOT_FOUND",
                    format!(
                        "Service '{}' path './{}' does not exist. The build context is missing — the deploy will fail.",
                        svc.name, svc.path
                    ),
                )
                .with_path(&svc.path)
                .with_hint("Create the directory or fix the `path` in floo.app.toml / floo.service.toml."),
            );
            continue;
        }

        // The DECLARED env_file (floo.app.toml inline OR floo.service.toml) — used
        // for the "you pointed at a missing file" check below and the Rails/secret
        // scans, which are about declared intent / local exposure.
        let configured_env_file = configured_env_file_for_service(resolved, svc);

        // Env vars that will actually be present at runtime, from local config:
        // managed-injected keys (DATABASE_URL, REDIS_URL, …) plus the keys in the
        // env_file the deploy will IMPORT. Satisfaction reads the imported set
        // (deploy_imported_env_files), NOT every declared/conventional file — an
        // inline floo.app.toml env_file is declared but never imported, so it
        // must not count. Server-side `floo env set` vars are invisible here,
        // which is why the consumers below warn rather than error.
        let imported_env_file = imported_env_files.get(&svc.name);
        let env_file_keys = match imported_env_file {
            Some((env_file, base)) => env_file_keys_for_service(base, Some(env_file.as_str())),
            None => std::collections::HashSet::new(),
        };

        // (Imported env_file path validity — ENV_FILE_INVALID — is checked once
        // over the full unfiltered import set above, matching the deploy's
        // unfiltered import; it is not repeated per filtered service here.)

        // A DECLARED-but-not-imported env_file (an inline floo.app.toml env_file,
        // which the deploy never imports) gets an advisory warning if missing —
        // not an error, since it has no effect on the deploy either way.
        if let Some(ref env_file) = configured_env_file {
            let is_imported = imported_env_file
                .map(|(imported, _)| imported == env_file)
                .unwrap_or(false);
            if !is_imported && !svc_dir.join(env_file).exists() {
                findings.push(
                    PreflightFinding::warning(
                        "ENV_FILE_NOT_FOUND",
                        format!(
                            "Service '{}' env_file '{env_file}' not found on disk.",
                            svc.name
                        ),
                    )
                    .with_path(&svc.path),
                );
            }
        }

        // A migrate_command needs a database to run against (config-only check;
        // see the helper). Local services pass the keys from their imported
        // env_file so a locally-declared DATABASE_URL satisfies it.
        check_migrate_command_no_database(svc, &managed_keys, &env_file_keys, &mut findings);

        // Required env vars the deploy can't satisfy from local config.
        let svc_plan = env_plan.services.iter().find(|p| p.service == svc.name);
        if let Some(plan) = svc_plan {
            let unsatisfied: Vec<&str> = plan
                .required
                .iter()
                .filter(|key| !managed_keys.contains(key.as_str()) && !env_file_keys.contains(*key))
                .map(String::as_str)
                .collect();
            if !unsatisfied.is_empty() {
                findings.push(
                    PreflightFinding::warning(
                        "REQUIRED_ENV_UNSATISFIED",
                        format!(
                            "Service '{}' requires env var(s) {} but they aren't injected by a managed service or present in a local env file.",
                            svc.name,
                            unsatisfied.join(", ")
                        ),
                    )
                    .with_path(&svc.path)
                    .with_hint(format!(
                        "Set them with `floo env set <KEY>=<value> --service {}` (or confirm they're already set server-side).",
                        svc.name
                    )),
                );
            }
        }

        if has_managed_postgres && service_looks_like_rails(&svc_dir) {
            for (env_label, env_path) in
                env_files_for_service(&svc_dir, configured_env_file.as_deref())
            {
                let Ok(contents) = std::fs::read_to_string(&env_path) else {
                    continue;
                };
                for line in contents.lines() {
                    let Some((key, value)) = parse_env_line(line) else {
                        continue;
                    };
                    // Case-insensitive: deploy-sync uppercases keys, so a
                    // lowercase `database_url` becomes DATABASE_URL at runtime.
                    if key.eq_ignore_ascii_case("DATABASE_URL")
                        && is_cloudsql_socket_database_url(value)
                    {
                        // The illustrative DSNs deliberately omit `user:pass@`
                        // userinfo: this string flows through print_json's
                        // redactor, and an embedded `scheme://u:p@` literal
                        // would be scrubbed to ***REDACTED***, hiding the very
                        // warning we're trying to surface. Show the host shape,
                        // not credential-shaped text.
                        findings.push(
                            PreflightFinding::warning(
                                "RAILS_DATABASE_URL_SOCKET_DSN",
                                format!(
                                    "Service '{}' looks like Rails and {env_label} contains a Cloud SQL socket-style DATABASE_URL. Rails parses DATABASE_URL with Ruby's URI parser before app code runs, so a host-less DSN like postgresql:///db?host=/cloudsql/... can fail at boot. Remove the stale local override or use floo's framework-compatible managed Postgres URL.",
                                    svc.name
                                ),
                            )
                            .with_path(&svc.path)
                            .with_hint("Managed Postgres now injects DATABASE_URL plus PGHOST/PGPORT/PGDATABASE/PGUSER/PGPASSWORD. The DATABASE_URL value should have a normal host, for example postgresql://127.0.0.1:5432/db."),
                        );
                        break;
                    }
                }
            }
        }

        // Check Dockerfile for common issues
        let dockerfile = svc_dir.join("Dockerfile");
        if dockerfile.exists() {
            match std::fs::read_to_string(&dockerfile) {
                Ok(content) => {
                    let no_lockfile = !svc_dir.join("package-lock.json").exists();
                    let mut npm_ci_flagged = false;

                    for line in content.lines() {
                        let trimmed = line.trim();

                        // Skip comments — don't match Dockerfile comment lines
                        if trimmed.starts_with('#') {
                            continue;
                        }

                        // EXPOSE mismatch
                        if let Some(expose_val) = trimmed.strip_prefix("EXPOSE ") {
                            let expose_val = expose_val.trim();
                            let port_str = expose_val.split('/').next().unwrap_or(expose_val);
                            if let Ok(exposed_port) = port_str.parse::<u16>() {
                                if exposed_port != svc.port {
                                    findings.push(
                                        PreflightFinding::warning(
                                            "EXPOSE_PORT_MISMATCH",
                                            format!(
                                                "Service '{}' Dockerfile EXPOSE {exposed_port} does not match configured port {}.",
                                                svc.name, svc.port
                                            ),
                                        )
                                        .with_path(&svc.path),
                                    );
                                }
                            }
                        }

                        // CMD exec form with $PORT or ${PORT} — variables don't expand in exec form.
                        // Emitted as a warning (not error) because heredoc content could produce
                        // false positives; the runtime failure makes it obvious quickly.
                        if trimmed.starts_with("CMD [")
                            && (trimmed.contains("$PORT") || trimmed.contains("${PORT}"))
                        {
                            findings.push(
                                PreflightFinding::warning(
                                    "CMD_EXEC_FORM_PORT",
                                    format!(
                                        "Service '{}' Dockerfile CMD uses exec form with $PORT — $PORT won't expand at runtime.",
                                        svc.name
                                    ),
                                )
                                .with_path(&svc.path)
                                .with_hint("Use shell form: CMD [\"sh\", \"-c\", \"your-command $PORT\"]"),
                            );
                        }

                        // npm ci without package-lock.json — report once per service
                        if !npm_ci_flagged && no_lockfile && trimmed.contains("npm ci") {
                            findings.push(
                                PreflightFinding::error(
                                    "NPM_CI_NO_LOCKFILE",
                                    format!(
                                        "Service '{}' Dockerfile uses 'npm ci' but package-lock.json was not found.",
                                        svc.name
                                    ),
                                )
                                .with_path(&svc.path)
                                .with_hint("Commit package-lock.json or change 'npm ci' to 'npm install' in your Dockerfile"),
                            );
                            npm_ci_flagged = true;
                        }
                    }
                }
                Err(e) => {
                    findings.push(
                        PreflightFinding::warning(
                            "DOCKERFILE_READ_ERROR",
                            format!(
                                "Service '{}' Dockerfile exists but could not be read: {e}. Checks skipped.",
                                svc.name
                            ),
                        )
                        .with_path(&svc.path),
                    );
                }
            }
        }
    }

    findings.extend(validate_cron_jobs(resolved, all_service_names));
    findings
}

/// Validate `[cron.<name>]` entries against floo's deploy contract.
///
/// Two failure modes the platform otherwise swallows silently:
/// - An invalid `schedule` deploys fine but the job never becomes due
///   (`croniter` can't parse it) — surfaced as `CRON_INVALID_SCHEDULE`.
/// - A `service` that names no declared service is silently skipped by
///   `_sync_cron_jobs` (`api/app/services/pipeline.py`) on a multi-service
///   deploy, so the job is never registered. That's a hard `CRON_SERVICE_NOT_FOUND`
///   error for multi-service apps. Single-service deploys run the job in the
///   sole image regardless of the name, so a mismatch there is only a warning.
fn validate_cron_jobs(
    resolved: &project_config::ResolvedApp,
    all_service_names: &[String],
) -> Vec<PreflightFinding> {
    let mut findings = Vec::new();
    let Some(app_cfg) = resolved.app_config.as_ref() else {
        return findings;
    };
    if app_cfg.cron.is_empty() {
        return findings;
    }

    let multi_service = all_service_names.len() > 1;
    // Stable order so JSON output and human output don't reshuffle run-to-run.
    let mut names: Vec<&String> = app_cfg.cron.keys().collect();
    names.sort();

    for name in names {
        let cfg = &app_cfg.cron[name];

        if !is_valid_cron_schedule(&cfg.schedule) {
            findings.push(
                PreflightFinding::error(
                    "CRON_INVALID_SCHEDULE",
                    format!(
                        "Cron job '{name}' has an invalid schedule '{}'. floo reads it as a standard cron expression and the job will never run.",
                        cfg.schedule
                    ),
                )
                .with_hint("Use a 5-field cron expression like \"0 9 * * *\" (every day at 09:00 UTC) or a macro like @daily."),
            );
        }

        if !all_service_names.iter().any(|s| s == &cfg.service) {
            let known = if all_service_names.is_empty() {
                "none".to_string()
            } else {
                all_service_names.join(", ")
            };
            if multi_service {
                findings.push(
                    PreflightFinding::error(
                        "CRON_SERVICE_NOT_FOUND",
                        format!(
                            "Cron job '{name}' references service '{}', which isn't a declared service ({known}). The deploy silently skips it and the job is never registered.",
                            cfg.service
                        ),
                    )
                    .with_hint("Set `service` to one of the declared services."),
                );
            } else {
                findings.push(
                    PreflightFinding::warning(
                        "CRON_SERVICE_NOT_FOUND",
                        format!(
                            "Cron job '{name}' references service '{}', which isn't the declared service ({known}). It still runs in the only service's image, but the name is misleading.",
                            cfg.service
                        ),
                    )
                    .with_hint("Set `service` to the declared service name."),
                );
            }
        }
    }

    findings
}

/// Whether a service is sourced from an external `repo` rather than the local
/// checkout. Such a service's build context (and its env files) live in the
/// referenced repo, so NONE of the local-disk checks — path existence, env_file
/// resolution, Dockerfile, the web-secret scan — apply to it. Single source of
/// that decision so every local-disk check skips external-repo services
/// consistently.
fn service_is_external_repo(resolved: &project_config::ResolvedApp, service_name: &str) -> bool {
    resolved
        .app_config
        .as_ref()
        .and_then(|c| c.services.get(service_name))
        .map(|entry| entry.repo.is_some())
        .unwrap_or(false)
}

/// The env_file a service's deploy-sync will import, from EITHER source the
/// deploy reads: the inline `floo.app.toml` service entry, or the service's own
/// `floo.service.toml` `[service] env_file`. Preflight's env-file checks must
/// see whichever is set, or they false-warn / miss secrets on service-file apps.
fn configured_env_file_for_service(
    resolved: &project_config::ResolvedApp,
    svc: &ServiceConfig,
) -> Option<String> {
    if let Some(app_cfg) = resolved.app_config.as_ref() {
        if let Some(env_file) = app_cfg
            .services
            .get(&svc.name)
            .and_then(|entry| entry.env_file.clone())
        {
            return Some(env_file);
        }
    }
    let svc_dir = resolved.config_dir.join(&svc.path);
    project_config::load_service_config(&svc_dir)
        .ok()
        .flatten()
        .and_then(|cfg| cfg.service.env_file)
}

/// Emit `MIGRATE_COMMAND_NO_DATABASE` if a service declares a `migrate_command`
/// but no database is reachable from local config. This is a CONFIG-ONLY check —
/// it reads the managed-injection keys plus whatever keys the service's imported
/// env_file declares, with no service-source disk access — so it applies to
/// external-repo services too: a migrate step with no DB fails wherever the
/// service's source lives. Callers pass an empty `env_file_keys` for
/// external-repo services (they import no local env file).
fn check_migrate_command_no_database(
    svc: &ServiceConfig,
    managed_keys: &std::collections::HashSet<String>,
    env_file_keys: &std::collections::HashSet<String>,
    findings: &mut Vec<PreflightFinding>,
) {
    if svc.migrate_command.is_some()
        && !managed_keys.contains("DATABASE_URL")
        && !env_file_keys.contains("DATABASE_URL")
    {
        findings.push(
            PreflightFinding::warning(
                "MIGRATE_COMMAND_NO_DATABASE",
                format!(
                    "Service '{}' declares migrate_command but no database is reachable from local config — no managed Postgres is attached and no DATABASE_URL is set in a local env file. The migration step will fail unless DATABASE_URL is set another way.",
                    svc.name
                ),
            )
            .with_path(&svc.path)
            .with_hint("Add `[postgres]` to floo.app.toml (or `floo services add postgres`), or set DATABASE_URL with `floo env set`."),
        );
    }
}

/// The managed-injection keys a service receives, per the env injection plan
/// (DATABASE_URL, PGHOST, REDIS_URL, …). Used to decide whether a declared
/// required var or a migrate_command's DATABASE_URL is satisfied locally.
fn managed_keys_for_service(
    env_plan: &EnvInjectionPlan,
    service_name: &str,
) -> std::collections::HashSet<String> {
    env_plan
        .services
        .iter()
        .find(|p| p.service == service_name)
        .map(|p| {
            p.managed
                .iter()
                .flat_map(|m| m.keys.iter().cloned())
                .collect()
        })
        .unwrap_or_default()
}

/// Every env var with a NON-EMPTY value the deploy will actually import for a
/// service, normalized to the UPPERCASE form deploy-sync imports them under.
///
/// Scans ONLY the service's configured `env_file` — the single source
/// `sync_env_vars_if_needed` imports. The conventional `.env*` files are
/// deliberately NOT scanned here: the deploy does not import them unless they
/// are the configured `env_file`, so counting a var found only in an
/// unconfigured `.env` would let preflight greenlight a `required` var the
/// deploy leaves unset server-side. Empty assignments (`FOO=`) are skipped for
/// the same reason (deploy treats present-but-empty as unsatisfied).
///
/// This is the satisfaction surface; the security/exposure scan in
/// `generate_security_findings` is intentionally broader (any local env file in
/// a web build context can leak, imported or not).
fn env_file_keys_for_service(
    svc_dir: &Path,
    configured_env_file: Option<&str>,
) -> std::collections::HashSet<String> {
    let mut keys = std::collections::HashSet::new();
    let Some(env_file) = configured_env_file else {
        return keys;
    };
    let Ok(contents) = std::fs::read_to_string(svc_dir.join(env_file)) else {
        return keys;
    };
    for line in contents.lines() {
        if let Some((key, value)) = parse_env_line(line) {
            if !value.is_empty() {
                // Uppercase: deploy-sync imports keys uppercased, so this is the
                // name the runtime actually sees.
                keys.insert(key.to_uppercase());
            }
        }
    }
    keys
}

/// Generate typed security findings from service config + managed services.
///
/// Severity is meaningful: `Info` is context (which services are
/// internet-facing); `Warning` is a real exposure risk (credentials reachable
/// from a browser-facing service, a secret-shaped var sitting in a web
/// service's env file). Unlike the prior implementation, none of these are
/// gated on `services.len() > 1` — a single public service that holds managed
/// credentials is exactly the case worth surfacing, and hiding it until a
/// second service appears was the bug in #1154.
///
/// `env_plan` is the already-built injection plan (single source of truth for
/// what each service receives); the caller passes it so we don't rebuild it.
fn generate_security_findings(
    services: &[ServiceConfig],
    resolved: &project_config::ResolvedApp,
    env_plan: &EnvInjectionPlan,
) -> Vec<PreflightFinding> {
    let mut findings: Vec<PreflightFinding> = Vec::new();

    // Check access_mode — note when no auth is configured.
    // Mirror what the CLI actually sends to the API: the deploy resolves
    // env-override-wins for the body.access_mode it POSTs, so the note matches
    // the value the deploy will use.
    let access_mode = resolved.app_config.as_ref().and_then(|c| {
        c.environments
            .get("dev")
            .and_then(|env| env.access_mode)
            .or(c.app.access_mode)
    });
    if matches!(access_mode, None | Some(AppAccessMode::Public)) {
        // Closes feedback 88e32b22 (floo-artifact 2026-05-01): the user had to
        // dig into the docs to discover that access_mode is a toml knob and
        // where it goes. Be specific: `[app]` is the placement applied today.
        findings.push(PreflightFinding::info(
            "ACCESS_MODE_PUBLIC",
            "Access mode is 'public' (no auth). Anyone can access your app. \
             To require auth, set `[app] access_mode = \"accounts\"` in \
             floo.app.toml — that's the placement applied on every push. \
             Per-env overrides via `[environments.<name>]` are accepted by \
             the schema but not yet applied server-side; use \
             `floo deploy --access-mode` to scope one env in the meantime."
                .to_string(),
        ));
    }

    // Note which services are internet-facing — for single-service apps too.
    let public_services: Vec<&str> = services
        .iter()
        .filter(|s| s.ingress == ServiceIngress::Public)
        .map(|s| s.name.as_str())
        .collect();
    let internal_services: Vec<&str> = services
        .iter()
        .filter(|s| s.ingress == ServiceIngress::Internal)
        .map(|s| s.name.as_str())
        .collect();

    if !public_services.is_empty() {
        findings.push(PreflightFinding::info(
            "SERVICE_INTERNET_FACING",
            format!(
                "Internet-facing: {}. Set ingress = \"internal\" in floo.app.toml to restrict.",
                public_services.join(", ")
            ),
        ));
    }
    if !internal_services.is_empty() {
        findings.push(PreflightFinding::info(
            "SERVICE_INTERNAL_ONLY",
            format!(
                "Internal only (not internet-facing): {}.",
                internal_services.join(", ")
            ),
        ));
    }

    // Managed-service credentials reaching a browser-facing (web) service — a
    // real risk whether the app has one service or ten.
    let web_service_names: Vec<&str> = services
        .iter()
        .filter(|s| s.service_type == ServiceType::Web)
        .map(|s| s.name.as_str())
        .collect();
    if env_plan.mode == "implicit_all"
        && !web_service_names.is_empty()
        && env_plan.services.iter().any(|svc| !svc.managed.is_empty())
    {
        findings.push(PreflightFinding::warning(
            "MANAGED_CREDS_TO_WEB",
            format!(
                "Managed service credentials are implicitly available to every service, including {}. Add [services.<name>.env] managed = [...] to attach them only where needed.",
                web_service_names.join(", "),
            ),
        ));
    } else {
        for svc_plan in &env_plan.services {
            let Some(svc) = services.iter().find(|s| s.name == svc_plan.service) else {
                continue;
            };
            if svc.service_type == ServiceType::Web && !svc_plan.managed.is_empty() {
                let handles: Vec<&str> =
                    svc_plan.managed.iter().map(|m| m.handle.as_str()).collect();
                findings.push(PreflightFinding::warning(
                    "MANAGED_CREDS_TO_WEB",
                    format!(
                        "Web service '{}' receives managed credentials: {}. Keep this only if browser-facing code really needs them server-side.",
                        svc.name,
                        handles.join(", "),
                    ),
                ));
            }
        }
    }

    // Secret-shaped vars sitting in a web service's env file — they may ship to
    // the browser. This is the finding that stamps the top-level
    // `contains_secrets` marker on the JSON payload.
    for svc in services {
        if svc.service_type != ServiceType::Web {
            continue;
        }
        // External-repo services build from the referenced repo; a local `.env`
        // under the config dir is not part of that service's source, so scanning
        // it would falsely emit SECRET_IN_WEB_SERVICE + contains_secrets.
        if service_is_external_repo(resolved, &svc.name) {
            continue;
        }
        // Resolve env files against the project's config dir, not the process
        // CWD — `floo preflight <path>` runs from anywhere, and the prior
        // `Path::new(&svc.path)` made this scan silently no-op off-cwd.
        let svc_dir = resolved.config_dir.join(&svc.path);
        // Scan the service's configured `env_file` (the source the deploy
        // actually imports — e.g. `.floo.env` from `floo init`, declared in
        // either floo.app.toml or floo.service.toml) in addition to the
        // conventional candidates, so a secret in a custom env file isn't missed.
        let configured_env_file = configured_env_file_for_service(resolved, svc);
        let mut env_filenames: Vec<&str> = Vec::new();
        if let Some(ef) = configured_env_file.as_deref() {
            env_filenames.push(ef);
        }
        for default in [".env", ".env.local", ".env.production"] {
            if !env_filenames.contains(&default) {
                env_filenames.push(default);
            }
        }
        for env_filename in env_filenames {
            let env_path = svc_dir.join(env_filename);
            if let Ok(contents) = std::fs::read_to_string(&env_path) {
                for line in contents.lines() {
                    let Some((key, _)) = parse_env_line(line) else {
                        continue;
                    };
                    // Check against the deployed (uppercase) form: deploy-sync
                    // uppercases keys on import, so a lowercase `stripe_secret_key`
                    // becomes the secret STRIPE_SECRET_KEY at runtime and must be
                    // flagged. The canonical redactor rule carries the PUBLIC_KEY /
                    // AWS_REGION allowlist, so preflight flags exactly what
                    // `--json` would redact — no parallel heuristic that drifts.
                    // The message keeps the var as written for recognizability.
                    let key_upper = key.to_uppercase();
                    let is_frontend_var = key_upper.starts_with("VITE_")
                        || key_upper.starts_with("NEXT_PUBLIC_")
                        || key_upper.starts_with("REACT_APP_");
                    if !is_frontend_var && crate::redact::env_var_key_is_secret(&key_upper) {
                        findings.push(
                                PreflightFinding::warning(
                                    "SECRET_IN_WEB_SERVICE",
                                    format!(
                                        "Secret-looking var '{}' in {}/{} — if this is a backend secret, remove it from the web service and set it on the api service: floo env set {}=<val> --service api",
                                        key, svc.name, env_filename, key
                                    ),
                                )
                                .with_path(&svc.path),
                            );
                    }
                }
            }
        }
    }

    findings
}

fn env_contract_for_service(
    resolved: &project_config::ResolvedApp,
    svc: &ServiceConfig,
) -> Option<project_config::ServiceEnvContract> {
    if let Some(app_cfg) = resolved.app_config.as_ref() {
        if let Some(entry) = app_cfg.services.get(&svc.name) {
            if entry.env.is_some() {
                return entry.env.clone();
            }
        }
    }

    let service_dir = if svc.path == "." {
        resolved.config_dir.clone()
    } else {
        resolved.config_dir.join(&svc.path)
    };
    project_config::load_service_env_contract(&service_dir)
        .ok()
        .flatten()
}

fn build_env_injection_plan(
    services: &[ServiceConfig],
    managed_services: &[project_config::ManagedServiceDeclaration],
    resolved: &project_config::ResolvedApp,
) -> EnvInjectionPlan {
    let contracts: Vec<Option<project_config::ServiceEnvContract>> = services
        .iter()
        .map(|svc| env_contract_for_service(resolved, svc))
        .collect();
    let explicit_managed = contracts
        .iter()
        .any(|contract| contract.as_ref().and_then(|c| c.managed.as_ref()).is_some());
    let declared_handles = managed_env_handles(resolved, managed_services);

    let mut notes = Vec::new();
    let mode = if explicit_managed {
        "explicit".to_string()
    } else if !declared_handles.is_empty() {
        notes.push(
            "No service declares env.managed, so managed service credentials use legacy implicit injection."
                .to_string(),
        );
        "implicit_all".to_string()
    } else {
        "none".to_string()
    };

    let service_plans = services
        .iter()
        .zip(contracts.iter())
        .map(|(svc, contract)| {
            let required = contract
                .as_ref()
                .map(|c| c.required.clone())
                .unwrap_or_default();
            let optional = contract
                .as_ref()
                .map(|c| c.optional.clone())
                .unwrap_or_default();
            let handles = if explicit_managed {
                contract
                    .as_ref()
                    .and_then(|c| c.normalized_managed("[env]").ok().flatten())
                    .unwrap_or_default()
            } else {
                declared_handles.clone()
            };
            let managed = handles
                .iter()
                .map(|handle| ManagedEnvInjection {
                    handle: handle.clone(),
                    keys: project_config::managed_env_attachment_keys(handle),
                })
                .collect();
            ServiceEnvInjectionPlan {
                service: svc.name.clone(),
                managed,
                required,
                optional,
            }
        })
        .collect();

    EnvInjectionPlan {
        mode,
        services: service_plans,
        notes,
    }
}

fn managed_env_handles(
    resolved: &project_config::ResolvedApp,
    managed_services: &[project_config::ManagedServiceDeclaration],
) -> Vec<String> {
    let mut handles: Vec<String> = managed_services.iter().map(|ms| ms.name.clone()).collect();
    if let Ok(lock) = crate::services_lock::read(&resolved.config_dir) {
        for managed in lock.managed_services {
            let handle = if managed.name == "default" {
                managed.service_type
            } else {
                format!("{}:{}", managed.service_type, managed.name)
            };
            handles.push(handle);
        }
    }
    handles.sort();
    handles.dedup();
    handles
}

fn display_env_injection_plan(plan: &EnvInjectionPlan) {
    eprintln!("  Env injection plan:");
    match plan.mode.as_str() {
        "explicit" => eprintln!("    mode: explicit per-service env.managed"),
        "implicit_all" => eprintln!("    mode: legacy implicit managed env on every service"),
        _ => eprintln!("    mode: no managed service credentials declared locally"),
    }
    for note in &plan.notes {
        eprintln!("    note: {note}");
    }
    for svc in &plan.services {
        eprintln!("    {}", svc.service);
        if svc.managed.is_empty() {
            eprintln!("      managed: none");
        } else {
            for managed in &svc.managed {
                eprintln!("      {} -> {}", managed.handle, managed.keys.join(", "));
            }
        }
        if !svc.required.is_empty() {
            eprintln!("      required: {}", svc.required.join(", "));
        }
        if !svc.optional.is_empty() {
            eprintln!("      optional: {}", svc.optional.join(", "));
        }
    }
    eprintln!();
}

/// Display preflight info in human-readable format. Renders the service table,
/// env-injection plan, resources, and the build/run warnings from `findings`.
/// Errors and the final ready/summary line are emitted by the caller so the
/// summary lands after all sections (security findings included).
fn display_preflight_human(
    app_name: &str,
    resolved: &project_config::ResolvedApp,
    services: &[ServiceConfig],
    per_service_detection: &[(String, DetectionResult)],
    env_injection_plan: &EnvInjectionPlan,
    findings: &[PreflightFinding],
) {
    let source_label = match resolved.source {
        AppSource::Flag => "--app flag".to_string(),
        AppSource::ServiceFile => {
            format!(
                "{} in {}",
                project_config::SERVICE_CONFIG_FILE,
                resolved.config_dir.display()
            )
        }
        AppSource::AppFile => {
            format!(
                "{} in {}",
                project_config::APP_CONFIG_FILE,
                resolved.config_dir.display()
            )
        }
    };

    eprintln!();
    eprintln!("  App '{}' (from {})", app_name, source_label);

    if services.len() > 1 {
        let names: Vec<&str> = services.iter().map(|s| s.name.as_str()).collect();
        eprintln!(
            "  Deploying {} services: {}",
            services.len(),
            names.join(", ")
        );
    }
    eprintln!();

    for (svc, (_, det)) in services.iter().zip(per_service_detection.iter()) {
        let path_label = if svc.path == "." {
            String::new()
        } else {
            format!(" (./{path})", path = svc.path)
        };
        eprintln!("  {}{path_label}", svc.name);
        eprintln!(
            "    type: {}, port: {}, ingress: {}",
            svc.service_type, svc.port, svc.ingress
        );

        let framework_label = det
            .framework
            .as_deref()
            .map(|f| format!(" ({f})"))
            .unwrap_or_default();
        eprintln!(
            "    runtime: {}{framework_label} \u{2014} {} confidence",
            det.runtime, det.confidence
        );
        eprintln!();
    }

    display_env_injection_plan(env_injection_plan);

    // Show global [resources] if present
    if let Some(ref app_cfg) = resolved.app_config {
        if let Some(ref res) = app_cfg.resources {
            let has_any = res.cpu.is_some() || res.memory.is_some() || res.max_instances.is_some();
            if has_any {
                eprintln!("  [resources] (global defaults)");
                let mut parts = Vec::new();
                if let Some(ref cpu) = res.cpu {
                    parts.push(format!("cpu: {cpu}"));
                }
                if let Some(ref memory) = res.memory {
                    parts.push(format!("memory: {memory}"));
                }
                if let Some(max) = res.max_instances {
                    parts.push(format!("max_instances: {max}"));
                }
                eprintln!("    {}", parts.join(", "));
                eprintln!();
            }
        }
    }

    for f in findings.iter().filter(|f| f.severity == Severity::Warning) {
        // The root service uses path "." — a "." : prefix there is just noise.
        match f.path.as_deref() {
            Some(path) if path != "." => {
                eprintln!("  {} {path}: {}", Severity::Warning.glyph(), f.message)
            }
            _ => eprintln!("  {} {}", Severity::Warning.glyph(), f.message),
        }
    }
}

/// Render typed security findings (info + warning) under a `Security:` header.
fn render_security_findings_human(findings: &[PreflightFinding]) {
    if findings.is_empty() {
        return;
    }
    eprintln!("  Security:");
    for f in findings {
        eprintln!("    {} {}", f.severity.glyph(), f.message);
    }
    eprintln!();
}

/// Print the auto-deploy contract and the final "ready" line. Only called when
/// the config has no errors; the warning count keeps a clean green honest.
fn print_preflight_ready(findings: &[PreflightFinding]) {
    // Closes feedback c9b70eb5: "no obvious signal anywhere in the Floo
    // workflow that dev auto-deploys on GitHub push." The CLI is the surface
    // the user is in front of right before pushing — surfacing the auto-deploy
    // contract here means they don't have to leave for the docs.
    eprintln!();
    eprintln!("  Deploys: dev auto-deploys on every `git push` to your default branch.");
    eprintln!("           Cut a GitHub release to promote the same build to production.");
    eprintln!("           See https://getfloo.com/docs/guides/golden-path.md for the full flow.");

    let warn_count = findings
        .iter()
        .filter(|f| f.severity == Severity::Warning)
        .count();
    if warn_count > 0 {
        output::success(
            &format!(
                "Config valid with {warn_count} warning(s) \u{2014} review the warnings above before deploying."
            ),
            None,
        );
    } else {
        output::success("Config valid \u{2014} ready to deploy.", None);
    }
}

/// Stream deploy logs via SSE and return the final deploy state.
pub(crate) fn stream_deploy(
    client: &FlooClient,
    app_id: &str,
    deploy_id: &str,
) -> Result<Deploy, FlooApiError> {
    let response = client.stream_deploy_logs(app_id, deploy_id)?;
    let reader = std::io::BufReader::new(response);

    let mut event_type = String::new();
    let mut data_buf = String::new();

    for line_result in reader.lines() {
        let line = match line_result {
            Ok(l) => l,
            Err(e) => {
                eprintln!("SSE connection error: {e}");
                break;
            }
        };

        if let Some(suffix) = line.strip_prefix("event: ") {
            event_type = suffix.to_string();
        } else if let Some(suffix) = line.strip_prefix("data: ") {
            data_buf = suffix.to_string();
        } else if line.starts_with(':') {
            continue; // SSE comment (heartbeat)
        } else if line.is_empty() && !event_type.is_empty() {
            // Event complete — process it
            match event_type.as_str() {
                "status" => match serde_json::from_str::<serde_json::Value>(&data_buf) {
                    Ok(parsed) => {
                        let status = parsed.get("status").and_then(|v| v.as_str()).unwrap_or("");
                        output::bold_line(status_label(status));
                    }
                    Err(e) => eprintln!("Malformed SSE status event: {e}"),
                },
                "log" => match serde_json::from_str::<serde_json::Value>(&data_buf) {
                    Ok(parsed) => {
                        if let Some(text) = parsed.get("text").and_then(|v| v.as_str()) {
                            for log_line in text.trim().lines() {
                                output::dim_line(log_line);
                            }
                        }
                    }
                    Err(e) => eprintln!("Malformed SSE log event: {e}"),
                },
                "done" => {
                    break;
                }
                "error" => match serde_json::from_str::<serde_json::Value>(&data_buf) {
                    Ok(parsed) => {
                        let msg = parsed
                            .get("message")
                            .and_then(|v| v.as_str())
                            .unwrap_or("Stream error");
                        return Err(FlooApiError::new(0, "STREAM_ERROR", msg));
                    }
                    Err(e) => {
                        eprintln!("Malformed SSE error event: {e}");
                        break;
                    }
                },
                _ => {}
            }
            event_type.clear();
            data_buf.clear();
        }
    }

    // After stream ends, fetch final deploy state for success/error output
    client.get_deploy(app_id, deploy_id)
}

/// Stream deploy events via SSE and emit NDJSON to stdout for JSON mode.
pub(crate) fn stream_deploy_json(
    client: &FlooClient,
    app_id: &str,
    deploy_id: &str,
) -> Result<Deploy, FlooApiError> {
    let response = client.stream_deploy_logs(app_id, deploy_id)?;
    let reader = std::io::BufReader::new(response);

    let mut event_type = String::new();
    let mut data_buf = String::new();

    for line_result in reader.lines() {
        let line = match line_result {
            Ok(l) => l,
            Err(_) => break,
        };

        if let Some(suffix) = line.strip_prefix("event: ") {
            event_type = suffix.to_string();
        } else if let Some(suffix) = line.strip_prefix("data: ") {
            data_buf = suffix.to_string();
        } else if line.starts_with(':') {
            continue;
        } else if line.is_empty() && !event_type.is_empty() {
            match event_type.as_str() {
                "status" => {
                    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&data_buf) {
                        let status = parsed.get("status").and_then(|v| v.as_str()).unwrap_or("");
                        output::print_json(
                            &serde_json::json!({"event": "status", "status": status}),
                        );
                    }
                }
                "log" => {
                    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&data_buf) {
                        if let Some(text) = parsed.get("text").and_then(|v| v.as_str()) {
                            output::print_json(&serde_json::json!({"event": "log", "text": text}));
                        }
                    }
                }
                "done" => {
                    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&data_buf) {
                        let status = parsed.get("status").and_then(|v| v.as_str()).unwrap_or("");
                        let url = parsed.get("url").and_then(|v| v.as_str()).unwrap_or("");
                        output::print_json(
                            &serde_json::json!({"event": "done", "status": status, "url": url}),
                        );
                    }
                    break;
                }
                "error" => {
                    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&data_buf) {
                        let msg = parsed
                            .get("message")
                            .and_then(|v| v.as_str())
                            .unwrap_or("Stream error");
                        return Err(FlooApiError::new(0, "STREAM_ERROR", msg));
                    }
                    break;
                }
                _ => {}
            }
            event_type.clear();
            data_buf.clear();
        }
    }

    client.get_deploy(app_id, deploy_id)
}

/// Poll the deploy endpoint until it reaches a terminal status.
pub(crate) fn poll_deploy(client: &FlooClient, app_id: &str, initial_data: &Deploy) -> Deploy {
    let deploy_id = initial_data.id.clone();
    let poll_start = Instant::now();
    let mut last_log_len: usize = 0;
    let mut deploy_data = initial_data.clone();

    while !deploy_status::is_terminal(deploy_data.status.as_deref().unwrap_or("")) {
        if !output::is_json_mode() {
            let build_logs = deploy_data.build_logs.as_deref().unwrap_or("");
            if build_logs.len() > last_log_len {
                let new_logs = &build_logs[last_log_len..];
                for line in new_logs.trim().lines() {
                    output::dim_line(line);
                }
                last_log_len = build_logs.len();
            }

            let status = deploy_data.status.as_deref().unwrap_or("");
            output::bold_line(status_label(status));
        }

        thread::sleep(POLL_INTERVAL);

        if poll_start.elapsed() >= POLL_TIMEOUT {
            output::error(
                "Deploy timed out after 10 minutes",
                &ErrorCode::DeployTimeout,
                Some(&format!(
                    "The deploy may still complete — check status with \
                     `floo apps status {app_id}` (deploy ID: {deploy_id})"
                )),
            );
            process::exit(1);
        }

        deploy_data = match client.get_deploy(app_id, &deploy_id) {
            Ok(d) => d,
            Err(e) => {
                output::error(&e.message, &ErrorCode::from_api(&e.code), None);
                process::exit(1);
            }
        };
    }

    // Print any remaining build logs for the final state
    if !output::is_json_mode() {
        let build_logs = deploy_data.build_logs.as_deref().unwrap_or("");
        if build_logs.len() > last_log_len {
            let new_logs = &build_logs[last_log_len..];
            for line in new_logs.trim().lines() {
                output::dim_line(line);
            }
        }
    }

    deploy_data
}

/// Non-fatal .env parser for the deploy path. Returns None on errors instead of exiting.
/// Separate from env.rs::parse_env_file because the deploy path is best-effort.
fn parse_env_file_soft(path: &Path) -> Option<Vec<(String, String)>> {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return None,
    };

    // Same line parser as preflight (`parse_env_line`); deploy-sync additionally
    // uppercases keys because that's the form it imports the vars under.
    let vars: Vec<(String, String)> = content
        .lines()
        .filter_map(parse_env_line)
        .map(|(key, value)| (key.to_uppercase(), value.to_string()))
        .collect();

    if vars.is_empty() {
        return None;
    }

    Some(vars)
}

/// Auto-import env vars from configured env_file on first deploy (server has 0 vars),
/// or when --sync-env is passed. Reads env_file from service configs (source of truth).
/// Every env_file the deploy will import, as `(service_name, env_file relative
/// path, base_dir)`. THE single source of "what env files get imported": the
/// root service's `floo.service.toml` `env_file`, plus each sub-service's
/// `floo.service.toml` `env_file`. Inline `floo.app.toml` `[services.x] env_file`
/// is intentionally NOT here — the deploy does not import it. Shared by
/// `sync_env_vars_if_needed` (which imports these) and the preflight
/// required-var satisfaction check (which verifies against these), so the set
/// preflight treats as imported is, by construction, exactly what deploy imports.
pub(crate) fn deploy_imported_env_files(
    resolved: &project_config::ResolvedApp,
) -> Vec<(String, String, PathBuf)> {
    let mut entries: Vec<(String, String, PathBuf)> = Vec::new();

    // Root service (floo.service.toml at the config dir).
    if let Some(ref svc_config) = resolved.service_config {
        if let Some(ref env_file) = svc_config.service.env_file {
            entries.push((
                svc_config.service.name.clone(),
                env_file.clone(),
                resolved.config_dir.clone(),
            ));
        }
    }

    // Sub-services: each app-config entry with a real subdir path, read from
    // that subdir's floo.service.toml.
    if let Some(ref app_config) = resolved.app_config {
        for entry in app_config.services.values() {
            // External-repo services build (and carry their env files) in the
            // referenced repo, not the local checkout — the deploy can't import
            // a local env_file for them, so they're not in the imported set.
            if entry.repo.is_some() {
                continue;
            }
            let Some(ref path_str) = entry.path else {
                continue;
            };
            let normalized = path_str.strip_prefix("./").unwrap_or(path_str);
            let normalized = normalized.strip_suffix('/').unwrap_or(normalized);
            if normalized.is_empty() || normalized == "." {
                continue;
            }
            let svc_dir = resolved.config_dir.join(normalized);
            if let Ok(Some(svc_config)) = project_config::load_service_config(&svc_dir) {
                if let Some(env_file) = svc_config.service.env_file {
                    entries.push((svc_config.service.name.clone(), env_file, svc_dir));
                }
            }
        }
    }

    entries
}

pub(crate) fn sync_env_vars_if_needed(
    client: &FlooClient,
    app_id: &str,
    resolved: &project_config::ResolvedApp,
    force_sync: bool,
) {
    // Resolve+validate each imported env_file to an absolute path (the shared
    // helper decides WHICH files are imported; this loop validates them).
    let mut env_file_entries: Vec<(String, PathBuf)> = Vec::new();
    for (svc_name, env_file, base_dir) in deploy_imported_env_files(resolved) {
        match super::env::validate_env_file_path(&env_file, &base_dir) {
            Ok(path) => env_file_entries.push((svc_name, path)),
            Err(msg) => {
                output::error(&msg, &ErrorCode::InvalidPath, None);
                process::exit(1);
            }
        }
    }

    if env_file_entries.is_empty() {
        return;
    }

    // Get server-side services — silently return on API error (services may not exist on first deploy)
    let server_services = match client.list_services(app_id, None) {
        Ok(r) => r.services,
        Err(_) => return,
    };

    for (svc_name, env_file_path) in &env_file_entries {
        let server_svc = server_services.iter().find(|s| s.name == *svc_name);

        let Some(svc) = server_svc else { continue };
        let svc_id = &svc.id;

        // Check env var count on server
        let env_count = match client.list_env_vars(app_id, Some(svc_id), "dev") {
            Ok(r) => r.env_vars.len(),
            Err(_) => continue,
        };

        // Skip if already has vars and not force-syncing
        if !force_sync && env_count > 0 {
            continue;
        }

        if !env_file_path.exists() {
            let file_name = env_file_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("env file");
            output::warn(&format!(
                "Service '{svc_name}' has env_file configured but {file_name} not found on disk."
            ));
            continue;
        }

        let vars = match parse_env_file_soft(env_file_path) {
            Some(v) => v,
            None => continue,
        };

        let count = vars.len();
        let file_name = env_file_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("env file");

        if !output::is_json_mode() {
            output::info(
                &format!(
                    "Importing {count} env var(s) for service '{svc_name}' from {file_name}..."
                ),
                None,
            );
        }

        // `false` = omit the write-only flag: env-file values come from the
        // repo (public by definition) and sticky semantics preserve any
        // existing write-only marker server-side.
        if let Err(e) = client.import_env_vars(app_id, &vars, Some(svc_id), "dev", false) {
            output::warn(&format!(
                "Failed to import env vars for service '{svc_name}': {}",
                e.message
            ));
        }
    }
}

fn fetch_remote_preflight(
    app_name: &str,
    managed: &[project_config::ManagedServiceDeclaration],
    project_root: &Path,
) -> Option<crate::api_types::PreflightPlan> {
    use crate::api_types::DeclaredState;
    use crate::config::load_config;

    load_config().api_key.as_ref()?;

    let client = crate::api_client::FlooClient::new(None).ok()?;
    let app = crate::resolve::resolve_app(&client, app_name).ok()?;

    let declared = DeclaredState {
        managed_services: collect_declared_managed_services(managed, project_root),
    };

    client.preflight(&app.id, &declared).ok()
}

/// Build the full list of declared managed services for preflight by merging:
///
/// - Legacy top-level `[postgres]` / `[redis]` / `[storage]` sections in
///   `floo.app.toml` (passed in as `managed`).
/// - `.floo/services.lock` entries written by `floo services add`.
///
/// The lock file is the canonical record for the new explicit-attachment
/// model — services provisioned via the CLI never appear in `floo.app.toml`,
/// so leaving them out of the preflight request body would make every
/// CLI-managed service look like drift (`to_orphan`) and flip the plan
/// destructive. See feedback id `0cadb329`.
///
/// Dedup by (service_type, name): when the same `(type, name)` appears in
/// both sources, the TOML version wins because it carries an explicit `tier`.
fn collect_declared_managed_services(
    managed: &[project_config::ManagedServiceDeclaration],
    project_root: &Path,
) -> Vec<crate::api_types::DeclaredManagedService> {
    use crate::api_types::DeclaredManagedService;

    let mut declared: Vec<DeclaredManagedService> = managed
        .iter()
        .map(|ms| DeclaredManagedService {
            service_type: ms.name.clone(),
            name: "default".to_string(),
            tier: ms.tier.clone(),
        })
        .collect();

    if let Ok(lock) = crate::services_lock::read(project_root) {
        for entry in lock.managed_services {
            let already_present = declared
                .iter()
                .any(|d| d.service_type == entry.service_type && d.name == entry.name);
            if !already_present {
                declared.push(DeclaredManagedService {
                    service_type: entry.service_type,
                    name: entry.name,
                    tier: None,
                });
            }
        }
    }

    declared
}

fn render_plan_human(plan: &crate::api_types::PreflightPlan) {
    let ms = &plan.managed_services;
    if !ms.to_provision.is_empty() {
        eprintln!("  Will provision on next deploy:");
        for item in &ms.to_provision {
            let tier = item.tier.as_deref().unwrap_or("basic");
            eprintln!(
                "    + {} (tier {tier})",
                format_args!("{}/{}", item.service_type, item.name)
            );
        }
        eprintln!();
    }
    if !ms.to_orphan.is_empty() {
        eprintln!("  \u{26a0} Orphaned managed services (deploy will NOT remove these):");
        for item in &ms.to_orphan {
            let impact = item
                .data_impact
                .as_deref()
                .unwrap_or("managed service data");
            eprintln!("    - {}/{}  [{}]", item.service_type, item.name, impact);
        }
        eprintln!("    Run 'floo services remove <type> --app <name>' to deprovision explicitly.");
        eprintln!();
    }
    if !ms.in_flight_deprovisioning.is_empty() {
        eprintln!("  \u{26a0} Deprovisioning in flight:");
        for item in &ms.in_flight_deprovisioning {
            eprintln!("    … {}/{}", item.service_type, item.name);
        }
        eprintln!();
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use tempfile::TempDir;

    use super::*;

    fn write_lock(dir: &Path, body: &str) {
        let lock_dir = dir.join(".floo");
        fs::create_dir_all(&lock_dir).unwrap();
        fs::write(lock_dir.join("services.lock"), body).unwrap();
    }

    #[test]
    fn test_collect_declared_managed_services_empty_when_nothing_declared() {
        let dir = TempDir::new().unwrap();
        let result = collect_declared_managed_services(&[], dir.path());
        assert!(result.is_empty());
    }

    #[test]
    fn test_collect_declared_managed_services_pulls_lock_entries() {
        let dir = TempDir::new().unwrap();
        write_lock(
            dir.path(),
            r#"{
              "version": 1,
              "managed_services": [
                {"type": "postgres", "name": "default", "status": "ready", "created_at": null},
                {"type": "redis", "name": "default", "status": "ready", "created_at": null},
                {"type": "storage", "name": "default", "status": "ready", "created_at": null}
              ]
            }"#,
        );
        let result = collect_declared_managed_services(&[], dir.path());
        let pairs: Vec<(String, String)> = result
            .iter()
            .map(|d| (d.service_type.clone(), d.name.clone()))
            .collect();
        assert_eq!(
            pairs,
            vec![
                ("postgres".to_string(), "default".to_string()),
                ("redis".to_string(), "default".to_string()),
                ("storage".to_string(), "default".to_string()),
            ]
        );
    }

    #[test]
    fn test_collect_declared_managed_services_preserves_named_services() {
        let dir = TempDir::new().unwrap();
        write_lock(
            dir.path(),
            r#"{
              "version": 1,
              "managed_services": [
                {"type": "postgres", "name": "default", "status": "ready", "created_at": null},
                {"type": "postgres", "name": "analytics", "status": "ready", "created_at": null}
              ]
            }"#,
        );
        let result = collect_declared_managed_services(&[], dir.path());
        let names: Vec<String> = result.iter().map(|d| d.name.clone()).collect();
        assert!(names.contains(&"default".to_string()));
        assert!(names.contains(&"analytics".to_string()));
    }

    #[test]
    fn test_collect_declared_managed_services_dedups_against_toml() {
        let dir = TempDir::new().unwrap();
        write_lock(
            dir.path(),
            r#"{
              "version": 1,
              "managed_services": [
                {"type": "postgres", "name": "default", "status": "ready", "created_at": null}
              ]
            }"#,
        );
        let toml_decl = vec![project_config::ManagedServiceDeclaration {
            name: "postgres".to_string(),
            tier: Some("basic".to_string()),
        }];
        let result = collect_declared_managed_services(&toml_decl, dir.path());
        assert_eq!(result.len(), 1);
        // TOML wins because it carries an explicit tier.
        assert_eq!(result[0].tier.as_deref(), Some("basic"));
    }

    #[test]
    fn test_collect_declared_managed_services_no_lock_file() {
        let dir = TempDir::new().unwrap();
        // No .floo/services.lock — only TOML declarations come through.
        let toml_decl = vec![project_config::ManagedServiceDeclaration {
            name: "postgres".to_string(),
            tier: None,
        }];
        let result = collect_declared_managed_services(&toml_decl, dir.path());
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].service_type, "postgres");
        assert_eq!(result[0].name, "default");
    }

    #[test]
    fn test_parse_env_file_soft_basic() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join(".floo.env");
        fs::write(&path, "KEY=value\nOTHER=123\n").unwrap();
        let vars = parse_env_file_soft(&path).unwrap();
        assert_eq!(
            vars,
            vec![
                ("KEY".to_string(), "value".to_string()),
                ("OTHER".to_string(), "123".to_string()),
            ]
        );
    }

    #[test]
    fn test_parse_env_file_soft_missing_file() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("nonexistent.env");
        assert!(parse_env_file_soft(&path).is_none());
    }

    #[test]
    fn test_parse_env_file_soft_empty_file() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join(".env");
        fs::write(&path, "").unwrap();
        assert!(parse_env_file_soft(&path).is_none());
    }

    #[test]
    fn test_parse_env_file_soft_skips_malformed() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join(".env");
        fs::write(&path, "GOOD=value\nBADLINE\n# comment\nALSO_GOOD=123\n").unwrap();
        let vars = parse_env_file_soft(&path).unwrap();
        assert_eq!(
            vars,
            vec![
                ("GOOD".to_string(), "value".to_string()),
                ("ALSO_GOOD".to_string(), "123".to_string()),
            ]
        );
    }

    // --- Cron schedule validation ---

    #[test]
    fn test_cron_schedule_accepts_standard_five_field() {
        for expr in [
            "0 9 * * *",       // every day 09:00
            "*/5 * * * *",     // every 5 minutes
            "0 0,12 * * *",    // midnight and noon
            "0 9 * * 1-5",     // weekdays
            "15 14 1 * *",     // 14:15 on the 1st
            "0 22 * * 1-5",    // 22:00 weekdays
            "23 0-20/2 * * *", // step over a range
            "5 4 * * sun",     // named day
            "0 0 1 jan *",     // named month
            "  0 9 * * *  ",   // surrounding whitespace
            "0 0 L * *",       // croniter: last day of month
            "0 9 * * SUN#2",   // croniter: 2nd Sunday of the month
            "0 9 * * fri#3",   // croniter: 3rd Friday of the month
            "0 0 1,L * *",     // list mixing a number and an extension
            "0 0 ? * MON",     // `?` in day-of-month (croniter day-field token)
            "0 0 1 * ?",       // `?` in day-of-week
        ] {
            assert!(is_valid_cron_schedule(expr), "should accept '{expr}'");
        }
    }

    #[test]
    fn test_cron_schedule_accepts_macros() {
        for expr in [
            "@daily",
            "@hourly",
            "@weekly",
            "@monthly",
            "@yearly",
            "@MIDNIGHT",
        ] {
            assert!(is_valid_cron_schedule(expr), "should accept '{expr}'");
        }
    }

    #[test]
    fn test_cron_schedule_rejects_non_cron_strings() {
        for expr in [
            "",                // empty
            "daily",           // bare word
            "every day",       // prose
            "9am",             // prose
            "0 9 * *",         // 4 fields
            "0 9 * * * * * *", // 8 fields
            "60 9 * * *",      // minute out of range
            "0 24 * * *",      // hour out of range
            "0 9 32 * *",      // day-of-month out of range
            "0 9 * 13 *",      // month out of range
            "0 9 * * 8 ",      // hmm dow 8 — see note below
            "@reboot",         // not a floo-supported macro
            "*/0 * * * *",     // zero step
            "foo 9 * * *",     // alpha in the minute field
            "0 jan * * *",     // alpha in the hour field
            "L 9 * * *",       // extension token in the minute field
            "0 0 * foo *",     // bogus month name (not JAN..DEC)
            "0 0 * * xyz",     // bogus day name (not SUN..SAT)
            "0 0 * mon *",     // day name in the month field
            "0 0 * * jan",     // month name in the day-of-week field
            "? 0 * * *",       // `?` in the minute field (croniter day-field only)
            "0 ? * * *",       // `?` in the hour field
        ] {
            // dow allows 0-7 (7 == Sunday); 8 is out of range.
            assert!(!is_valid_cron_schedule(expr), "should reject '{expr}'");
        }
    }

    #[test]
    fn test_cron_schedule_dow_seven_is_sunday() {
        assert!(is_valid_cron_schedule("0 9 * * 7"));
    }

    // --- Finding helpers ---

    #[test]
    fn test_has_errors() {
        assert!(!has_errors(&[]));
        assert!(!has_errors(&[PreflightFinding::warning("W", "w".into())]));
        assert!(!has_errors(&[PreflightFinding::info("I", "i".into())]));
        assert!(has_errors(&[
            PreflightFinding::warning("W", "w".into()),
            PreflightFinding::error("E", "e".into()),
        ]));
    }

    #[test]
    fn test_finding_serializes_with_severity_and_omits_empty_optionals() {
        let f = PreflightFinding::error("SERVICE_PATH_NOT_FOUND", "missing".into())
            .with_path("api")
            .with_hint("create it");
        let v = serde_json::to_value(&f).unwrap();
        assert_eq!(v["severity"], "error");
        assert_eq!(v["code"], "SERVICE_PATH_NOT_FOUND");
        assert_eq!(v["path"], "api");
        assert_eq!(v["hint"], "create it");

        // No path/hint -> keys omitted entirely.
        let bare = PreflightFinding::info("X", "x".into());
        let bv = serde_json::to_value(&bare).unwrap();
        assert!(bv.get("path").is_none());
        assert!(bv.get("hint").is_none());
    }

    #[test]
    fn test_parse_env_line_strips_export_and_quotes() {
        // `export FOO=bar` records key `FOO`, matching the deploy-sync parser.
        assert_eq!(parse_env_line("export FOO=bar"), Some(("FOO", "bar")));
        assert_eq!(
            parse_env_line("  export  DATABASE_URL=\"x\"  "),
            Some(("DATABASE_URL", "x"))
        );
        assert_eq!(parse_env_line("FOO=bar"), Some(("FOO", "bar")));
        // `export` only stripped as a prefix word, not inside a key.
        assert_eq!(parse_env_line("exported=1"), Some(("exported", "1")));
        assert_eq!(parse_env_line("# comment"), None);
        assert_eq!(parse_env_line(""), None);
        // One matching quote pair only; a lone quote doesn't panic.
        assert_eq!(parse_env_line("FOO='x'"), Some(("FOO", "x")));
        assert_eq!(parse_env_line("FOO=\""), Some(("FOO", "\"")));
    }

    #[test]
    fn test_parse_env_line_and_soft_agree_on_export_spacing() {
        // The inspection parser and the deploy-sync import parser must read the
        // SAME key from `export  FOO=bar` (multiple spaces). parse_env_file_soft
        // additionally uppercases the key (its import normalization).
        assert_eq!(parse_env_line("export  FOO=bar"), Some(("FOO", "bar")));
        let dir = TempDir::new().unwrap();
        let path = dir.path().join(".env");
        fs::write(&path, "export  FOO=bar\n").unwrap();
        assert_eq!(
            parse_env_file_soft(&path),
            Some(vec![("FOO".to_string(), "bar".to_string())])
        );
    }
}
