use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process;

use colored::Colorize;

use crate::config::{load_config, save_config};
use crate::constants::VERSION;
use crate::errors::ErrorCode;
use crate::output;

const SKILL_CONTENT: &str = include_str!("../../skills/floo/SKILL.md");
const SKILL_SERVICES: &str = include_str!("../../plugin/skills/floo-services/SKILL.md");
const SKILL_SECURITY: &str = include_str!("../../plugin/skills/floo-security/SKILL.md");

/// All plugin skills to install alongside the main skill.
const PLUGIN_SKILLS: &[(&str, &str)] = &[
    ("floo-services", SKILL_SERVICES),
    ("floo-security", SKILL_SECURITY),
];

pub fn install(path: Option<PathBuf>, print: bool) {
    if print {
        if output::is_json_mode() {
            let plugin_skills: Vec<serde_json::Value> = PLUGIN_SKILLS
                .iter()
                .map(|(name, content)| {
                    serde_json::json!({
                        "name": name,
                        "content": content,
                    })
                })
                .collect();
            output::success(
                "Skill content",
                Some(serde_json::json!({
                    "content": SKILL_CONTENT,
                    "plugin_skills": plugin_skills,
                    "version": VERSION,
                })),
            );
        } else {
            std::io::stdout()
                .write_all(SKILL_CONTENT.as_bytes())
                .unwrap_or_else(|e| {
                    output::error(
                        &format!("Failed to write to stdout: {e}"),
                        &ErrorCode::FileError,
                        None,
                    );
                    process::exit(1);
                });
        }
        return;
    }

    let dir = match path {
        Some(d) => d,
        None => {
            output::error(
                "No target specified.",
                &ErrorCode::MissingArgument,
                Some("Provide --path <dir> to install or --print to output to stdout."),
            );
            process::exit(1);
        }
    };

    if let Err(e) = fs::create_dir_all(&dir) {
        output::error(
            &format!("Failed to create directory '{}': {e}", dir.display()),
            &ErrorCode::FileError,
            None,
        );
        process::exit(1);
    }

    let file_path = dir.join("SKILL.md");

    let abs_path = match file_path.canonicalize().or_else(|_| {
        // Directory exists but file doesn't yet — canonicalize the parent and append filename
        dir.canonicalize().map(|d| d.join("SKILL.md"))
    }) {
        Ok(p) => p,
        Err(e) => {
            output::error(
                &format!("Failed to resolve path '{}': {e}", file_path.display()),
                &ErrorCode::FileError,
                None,
            );
            process::exit(1);
        }
    };

    if let Err(e) = fs::write(&abs_path, SKILL_CONTENT) {
        output::error(
            &format!("Failed to write '{}': {e}", abs_path.display()),
            &ErrorCode::FileError,
            None,
        );
        process::exit(1);
    }

    // Install plugin skills as sibling directories
    for (skill_name, skill_content) in PLUGIN_SKILLS {
        let skill_dir = dir.join(skill_name);
        if let Err(e) = fs::create_dir_all(&skill_dir) {
            output::error(
                &format!("Failed to create directory '{}': {e}", skill_dir.display()),
                &ErrorCode::FileError,
                None,
            );
            process::exit(1);
        }
        let skill_path = skill_dir.join("SKILL.md");
        if let Err(e) = fs::write(&skill_path, skill_content) {
            output::error(
                &format!("Failed to write '{}': {e}", skill_path.display()),
                &ErrorCode::FileError,
                None,
            );
            process::exit(1);
        }
    }

    // Track the path in config
    let abs_str = match abs_path.to_str() {
        Some(s) => s.to_string(),
        None => {
            output::error(
                &format!(
                    "Path '{}' contains invalid UTF-8 and cannot be tracked.",
                    abs_path.display()
                ),
                &ErrorCode::FileError,
                Some("Use a path containing only valid UTF-8 characters."),
            );
            process::exit(1);
        }
    };
    let mut config = load_config();
    config.add_skill_path(&abs_str);
    if let Err(e) = save_config(&config) {
        output::error(
            &format!("Skill installed but failed to save config: {e}"),
            &ErrorCode::ConfigError,
            None,
        );
        process::exit(1);
    }

    let (read_only, read_write) = recommended_permissions();

    let plugin_skill_names: Vec<&str> = PLUGIN_SKILLS.iter().map(|(name, _)| *name).collect();

    if output::is_json_mode() {
        output::success(
            &format!("Installed agent skills to {}", dir.display()),
            Some(serde_json::json!({
                "path": abs_str,
                "plugin_skills": plugin_skill_names,
                "version": VERSION,
                "recommended_permissions": {
                    "read_only": read_only,
                    "read_write": read_write,
                },
            })),
        );
    } else {
        output::success(
            &format!(
                "Installed agent skills to {} (floo, {})",
                dir.display(),
                plugin_skill_names.join(", ")
            ),
            None,
        );
        print_permission_recommendations(&read_only, &read_write);
    }
}

/// Write embedded content only when the file is missing or differs on disk.
fn write_if_changed(path: &Path, content: &str) -> io::Result<bool> {
    if fs::read(path).is_ok_and(|existing| existing == content.as_bytes()) {
        return Ok(false);
    }

    fs::write(path, content)?;
    Ok(true)
}

/// Refresh one tracked skill bundle. Returns whether any bundled file changed.
fn refresh_skill_bundle(path: &Path) -> io::Result<bool> {
    let mut changed = write_if_changed(path, SKILL_CONTENT)?;

    if let Some(parent) = path.parent() {
        for (skill_name, skill_content) in PLUGIN_SKILLS {
            let skill_dir = parent.join(skill_name);
            let _ = fs::create_dir_all(&skill_dir);
            let skill_path = skill_dir.join("SKILL.md");
            match write_if_changed(&skill_path, skill_content) {
                Ok(plugin_changed) => changed |= plugin_changed,
                Err(e) => {
                    if !output::is_json_mode() {
                        eprintln!("  Warning: failed to refresh {skill_name} skill: {e}");
                    }
                }
            }
        }
    }

    Ok(changed)
}

/// Refresh changed tracked skill bundles. Returns the list of paths that changed.
/// Removes stale paths (directories that no longer exist) from tracking.
/// Reports errors for write failures without removing those paths.
pub fn refresh_skill_files() -> Vec<String> {
    let mut config = load_config();
    if config.skill_paths.is_empty() {
        return Vec::new();
    }

    let mut refreshed = Vec::new();
    let mut still_valid = Vec::new();

    for path_str in &config.skill_paths {
        let path = PathBuf::from(path_str);
        let parent_exists = path.parent().is_some_and(|p| p.exists());

        if !parent_exists {
            // Parent directory gone — prune from tracking
            if !output::is_json_mode() {
                eprintln!("  Removed stale skill path (directory gone): {path_str}");
            }
            continue;
        }

        match refresh_skill_bundle(&path) {
            Ok(changed) => {
                if changed {
                    refreshed.push(path_str.clone());
                }
                still_valid.push(path_str.clone());
            }
            Err(e) => {
                // Write failed but directory exists — keep tracking, report error
                still_valid.push(path_str.clone());
                if !output::is_json_mode() {
                    eprintln!("  Warning: failed to refresh skill at {path_str}: {e}");
                }
            }
        }
    }

    if still_valid.len() != config.skill_paths.len() {
        config.skill_paths = still_valid;
        if let Err(e) = save_config(&config) {
            if !output::is_json_mode() {
                eprintln!("  Warning: failed to update skill tracking in config: {e}");
            }
        }
    }

    refreshed
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PermissionClass {
    ReadOnly,
    ReadWrite,
}

#[derive(Clone, Copy, Debug)]
struct Permission {
    class: PermissionClass,
    command: &'static [&'static str],
    required_flags: &'static [&'static str],
}

impl Permission {
    fn render(self) -> String {
        let invocation = self
            .command
            .iter()
            .chain(self.required_flags)
            .copied()
            .collect::<Vec<_>>()
            .join(" ");
        format!("Bash(floo {invocation}:*)")
    }
}

const NO_FLAGS: &[&str] = &[];
const PREFLIGHT_FLAG: &[&str] = &["--preflight"];

macro_rules! permission {
    ($class:ident, [$($segment:literal),+]) => {
        Permission {
            class: PermissionClass::$class,
            command: &[$($segment),+],
            required_flags: NO_FLAGS,
        }
    };
    ($class:ident, [$($segment:literal),+], $flags:expr) => {
        Permission {
            class: PermissionClass::$class,
            command: &[$($segment),+],
            required_flags: $flags,
        }
    };
}

/// Explicit capabilities recommended to coding agents. A command omitted from
/// this typed table is intentionally not granted.
const RECOMMENDED_PERMISSIONS: &[Permission] = &[
    permission!(ReadOnly, ["apps", "list"]),
    permission!(ReadOnly, ["apps", "show"]),
    permission!(ReadOnly, ["apps", "github", "status"]),
    permission!(ReadOnly, ["deploys", "list"]),
    permission!(ReadOnly, ["deploys", "logs"]),
    permission!(ReadOnly, ["deploys", "watch"]),
    permission!(ReadOnly, ["previews", "list"]),
    permission!(ReadOnly, ["previews", "status"]),
    permission!(ReadOnly, ["previews", "logs"]),
    permission!(ReadOnly, ["previews", "resources", "list"]),
    permission!(ReadOnly, ["previews", "resources", "show"]),
    permission!(ReadOnly, ["env", "list"]),
    permission!(ReadOnly, ["services", "list"]),
    permission!(ReadOnly, ["services", "show"]),
    permission!(ReadOnly, ["domains", "list"]),
    permission!(ReadOnly, ["logs"]),
    permission!(ReadOnly, ["analytics"]),
    permission!(ReadOnly, ["releases", "list"]),
    permission!(ReadOnly, ["releases", "show"]),
    permission!(ReadOnly, ["preflight"]),
    permission!(ReadOnly, ["redeploy"], PREFLIGHT_FLAG),
    permission!(ReadOnly, ["docs"]),
    permission!(ReadOnly, ["commands"]),
    permission!(ReadOnly, ["version"]),
    permission!(ReadOnly, ["auth", "whoami"]),
    permission!(ReadOnly, ["doctor", "accounts"]),
    permission!(ReadOnly, ["doctor", "managed-services"]),
    permission!(ReadOnly, ["orgs", "members", "list"]),
    permission!(ReadOnly, ["billing", "contact"]),
    permission!(ReadWrite, ["apps", "password"]),
    permission!(ReadWrite, ["env", "get"]),
    permission!(ReadWrite, ["redeploy"]),
    permission!(ReadWrite, ["previews", "up"]),
    permission!(ReadWrite, ["previews", "delete"]),
    permission!(ReadWrite, ["previews", "resources", "reset"]),
    permission!(ReadWrite, ["deploys", "rollback"]),
    permission!(ReadWrite, ["init"]),
    permission!(ReadWrite, ["env", "set"]),
    permission!(ReadWrite, ["env", "unset"]),
    permission!(ReadWrite, ["env", "import"]),
    permission!(ReadWrite, ["domains", "add"]),
    permission!(ReadWrite, ["domains", "verify"]),
    permission!(ReadWrite, ["domains", "remove"]),
    permission!(ReadWrite, ["apps", "delete"]),
    permission!(ReadWrite, ["apps", "github", "connect"]),
    permission!(ReadWrite, ["apps", "github", "disconnect"]),
    permission!(ReadWrite, ["releases", "promote"]),
    permission!(ReadWrite, ["billing", "spend-cap", "set"]),
    permission!(ReadWrite, ["billing", "upgrade"]),
    permission!(ReadWrite, ["orgs", "members", "set-role"]),
    permission!(ReadWrite, ["update"]),
];

fn recommended_permissions() -> (Vec<String>, Vec<String>) {
    let by_class = |class| {
        RECOMMENDED_PERMISSIONS
            .iter()
            .copied()
            .filter(|permission| permission.class == class)
            .map(Permission::render)
            .collect()
    };
    (
        by_class(PermissionClass::ReadOnly),
        by_class(PermissionClass::ReadWrite),
    )
}

fn print_permission_recommendations(read_only: &[String], read_write: &[String]) {
    eprintln!();
    eprintln!("{}", "  Recommended permissions for coding agents:".bold());
    eprintln!();
    eprintln!(
        "  {} {}",
        "Read-only".green().bold(),
        "(recommended to enable by default):".dimmed()
    );
    for perm in read_only {
        eprintln!("    {perm}");
    }
    eprintln!();
    eprintln!(
        "  {} {}",
        "Read-write".yellow().bold(),
        "(your choice):".dimmed()
    );
    for perm in read_write {
        eprintln!("    {perm}");
    }
    eprintln!();
    eprintln!(
        "  {}",
        "Add these to .claude/settings.json under \"permissions.allow\".".dimmed()
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_skill_content_is_embedded() {
        assert!(!SKILL_CONTENT.is_empty());
        assert!(SKILL_CONTENT.contains("# floo"));
    }

    #[test]
    fn test_skill_content_has_key_sections() {
        assert!(SKILL_CONTENT.contains("## Discover before acting"));
        assert!(SKILL_CONTENT.contains("## Deploy invariant"));
        assert!(SKILL_CONTENT.contains("floo docs"));
        assert!(SKILL_CONTENT.contains("floo commands --json"));
        assert!(SKILL_CONTENT.contains("--json"));
    }

    #[test]
    fn test_plugin_skills_are_embedded() {
        assert_eq!(PLUGIN_SKILLS.len(), 2);
        for (name, content) in PLUGIN_SKILLS {
            assert!(!name.is_empty());
            assert!(!content.is_empty());
            assert!(content.contains("---"), "{name} skill missing frontmatter");
            assert!(
                content.contains(&format!("name: {name}")),
                "{name} skill frontmatter name mismatch"
            );
        }
    }

    #[test]
    fn test_skill_bundle_refresh_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let skill_path = dir.path().join("SKILL.md");

        assert!(refresh_skill_bundle(&skill_path).unwrap());
        assert_eq!(fs::read_to_string(&skill_path).unwrap(), SKILL_CONTENT);
        for (name, content) in PLUGIN_SKILLS {
            assert_eq!(
                fs::read_to_string(dir.path().join(name).join("SKILL.md")).unwrap(),
                *content
            );
        }

        assert!(!refresh_skill_bundle(&skill_path).unwrap());
    }

    #[test]
    fn test_skill_bundle_refresh_repairs_plugin_drift_once() {
        let dir = tempfile::tempdir().unwrap();
        let skill_path = dir.path().join("SKILL.md");
        refresh_skill_bundle(&skill_path).unwrap();

        let plugin_path = dir.path().join("floo-services").join("SKILL.md");
        fs::write(&plugin_path, "stale skill").unwrap();

        assert!(refresh_skill_bundle(&skill_path).unwrap());
        assert_eq!(fs::read_to_string(plugin_path).unwrap(), SKILL_SERVICES);
        assert!(!refresh_skill_bundle(&skill_path).unwrap());
    }

    #[test]
    fn test_services_skill_covers_all_services() {
        assert!(SKILL_SERVICES.contains("floo docs services"));
        assert!(SKILL_SERVICES.contains("Postgres"));
        assert!(SKILL_SERVICES.contains("Redis"));
        assert!(SKILL_SERVICES.contains("Storage"));
        assert!(SKILL_SERVICES.contains("DATABASE_URL"));
        assert!(SKILL_SERVICES.contains("REDIS_URL"));
    }

    #[test]
    fn test_security_skill_has_anti_patterns() {
        assert!(SKILL_SECURITY.contains("## Secrets"));
        assert!(SKILL_SECURITY.contains("## Data access"));
        assert!(SKILL_SECURITY.contains("floo docs auth"));
        assert!(SKILL_SECURITY.contains("Never hardcode"));
    }

    #[test]
    fn test_recommended_permissions_read_only() {
        let (read_only, _) = recommended_permissions();
        for expected in [
            "Bash(floo apps list:*)",
            "Bash(floo logs:*)",
            "Bash(floo previews status:*)",
            "Bash(floo previews resources list:*)",
            "Bash(floo previews resources show:*)",
            "Bash(floo apps show:*)",
            "Bash(floo preflight:*)",
            "Bash(floo redeploy --preflight:*)",
            "Bash(floo docs:*)",
        ] {
            assert!(read_only.iter().any(|permission| permission == expected));
        }
        // Write commands should not be in read-only
        assert!(!read_only
            .iter()
            .any(|permission| permission == "Bash(floo deploy:*)"));
        assert!(!read_only
            .iter()
            .any(|permission| permission == "Bash(floo apps delete:*)"));
    }

    #[test]
    fn test_recommended_permissions_read_write() {
        let (_, read_write) = recommended_permissions();
        for expected in [
            "Bash(floo redeploy:*)",
            "Bash(floo previews up:*)",
            "Bash(floo previews resources reset:*)",
            "Bash(floo env set:*)",
            "Bash(floo apps delete:*)",
        ] {
            assert!(read_write.iter().any(|permission| permission == expected));
        }
        // Read-only commands should not be in read-write
        assert!(!read_write
            .iter()
            .any(|permission| permission == "Bash(floo logs:*)"));
        assert!(!read_write
            .iter()
            .any(|permission| permission == "Bash(floo docs:*)"));
    }

    #[test]
    fn test_recommended_permission_commands_exist_in_clap() {
        use clap::CommandFactory;

        let root = crate::cli::Cli::command();
        for permission in RECOMMENDED_PERMISSIONS {
            let mut command = &root;
            for segment in permission.command {
                command = command.find_subcommand(segment).unwrap_or_else(|| {
                    panic!(
                        "recommended permission references unknown command path `floo {}`",
                        permission.command.join(" ")
                    )
                });
            }
            for flag in permission.required_flags {
                let long = flag
                    .strip_prefix("--")
                    .unwrap_or_else(|| panic!("permission flag `{flag}` must use long form"));
                assert!(
                    root.get_arguments()
                        .chain(command.get_arguments())
                        .any(|argument| argument.get_long() == Some(long)),
                    "recommended permission `floo {}` uses unknown or non-canonical flag `{flag}`",
                    permission.command.join(" "),
                );
            }
        }
    }

    #[test]
    fn test_recommended_permissions_are_unique() {
        let mut rendered = std::collections::HashSet::new();
        for permission in RECOMMENDED_PERMISSIONS {
            let rule = permission.render();
            assert!(
                rendered.insert(rule.clone()),
                "duplicate permission `{rule}`"
            );
        }
    }
}
