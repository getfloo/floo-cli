use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process;

use colored::Colorize;

use crate::config::{load_config, save_config};
use crate::constants::VERSION;
use crate::errors::ErrorCode;
use crate::output;

const SKILL_CONTENT: &str = include_str!("../../skills/floo/SKILL.md");

pub fn install(path: Option<PathBuf>, print: bool) {
    if print {
        if output::is_json_mode() {
            output::success(
                "Skill content",
                Some(serde_json::json!({
                    "content": SKILL_CONTENT,
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

    if output::is_json_mode() {
        output::success(
            &format!("Installed agent skill to {}", abs_path.display()),
            Some(serde_json::json!({
                "path": abs_str,
                "version": VERSION,
                "recommended_permissions": {
                    "read_only": read_only,
                    "read_write": read_write,
                },
            })),
        );
    } else {
        output::success(
            &format!("Installed agent skill to {}", abs_path.display()),
            None,
        );
        print_permission_recommendations(&read_only, &read_write);
    }
}

/// Refresh all tracked skill files. Returns the list of paths that were refreshed.
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

        match fs::write(&path, SKILL_CONTENT) {
            Ok(()) => {
                refreshed.push(path_str.clone());
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

fn recommended_permissions() -> (Vec<&'static str>, Vec<&'static str>) {
    let read_only = vec![
        "Bash(floo apps list:*)",
        "Bash(floo apps status:*)",
        "Bash(floo apps password:*)",
        "Bash(floo apps github status:*)",
        "Bash(floo deploy list:*)",
        "Bash(floo deploy logs:*)",
        "Bash(floo deploy watch:*)",
        "Bash(floo env list:*)",
        "Bash(floo env get:*)",
        "Bash(floo services list:*)",
        "Bash(floo services info:*)",
        "Bash(floo domains list:*)",
        "Bash(floo logs:*)",
        "Bash(floo analytics:*)",
        "Bash(floo releases list:*)",
        "Bash(floo releases show:*)",
        "Bash(floo check:*)",
        "Bash(floo docs:*)",
        "Bash(floo commands:*)",
        "Bash(floo version:*)",
        "Bash(floo auth whoami:*)",
        "Bash(floo orgs members list:*)",
        "Bash(floo billing contact:*)",
    ];

    let read_write = vec![
        "Bash(floo deploy:*)",
        "Bash(floo deploy rollback:*)",
        "Bash(floo init:*)",
        "Bash(floo env set:*)",
        "Bash(floo env remove:*)",
        "Bash(floo env import:*)",
        "Bash(floo services add:*)",
        "Bash(floo services rm:*)",
        "Bash(floo domains add:*)",
        "Bash(floo domains remove:*)",
        "Bash(floo apps delete:*)",
        "Bash(floo apps github connect:*)",
        "Bash(floo apps github disconnect:*)",
        "Bash(floo releases promote:*)",
        "Bash(floo billing spend-cap set:*)",
        "Bash(floo billing upgrade:*)",
        "Bash(floo orgs members set-role:*)",
        "Bash(floo update:*)",
    ];

    (read_only, read_write)
}

fn print_permission_recommendations(read_only: &[&str], read_write: &[&str]) {
    eprintln!();
    eprintln!(
        "{}",
        "  Recommended permissions for coding agents:".bold()
    );
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
        "Add these to .claude/settings.json under \"permissions.allow\"."
            .dimmed()
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_skill_content_is_embedded() {
        assert!(!SKILL_CONTENT.is_empty());
        assert!(SKILL_CONTENT.contains("# Floo"));
    }

    #[test]
    fn test_skill_content_has_key_sections() {
        assert!(SKILL_CONTENT.contains("## Getting Started"));
        assert!(SKILL_CONTENT.contains("## Self-Discovery"));
        assert!(SKILL_CONTENT.contains("floo docs"));
        assert!(SKILL_CONTENT.contains("--json"));
    }

    #[test]
    fn test_recommended_permissions_read_only() {
        let (read_only, _) = recommended_permissions();
        assert!(read_only.contains(&"Bash(floo apps list:*)"));
        assert!(read_only.contains(&"Bash(floo logs:*)"));
        assert!(read_only.contains(&"Bash(floo check:*)"));
        assert!(read_only.contains(&"Bash(floo docs:*)"));
        // Write commands should not be in read-only
        assert!(!read_only.contains(&"Bash(floo deploy:*)"));
        assert!(!read_only.contains(&"Bash(floo apps delete:*)"));
    }

    #[test]
    fn test_recommended_permissions_read_write() {
        let (_, read_write) = recommended_permissions();
        assert!(read_write.contains(&"Bash(floo deploy:*)"));
        assert!(read_write.contains(&"Bash(floo env set:*)"));
        assert!(read_write.contains(&"Bash(floo apps delete:*)"));
        // Read-only commands should not be in read-write
        assert!(!read_write.contains(&"Bash(floo logs:*)"));
        assert!(!read_write.contains(&"Bash(floo docs:*)"));
    }
}
