use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process;

use crate::config::{load_config, save_config};
use crate::constants::VERSION;
use crate::output;

const SKILL_CONTENT: &str = include_str!("../../skills/floo.md");

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
                        "IO_ERROR",
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
                "MISSING_ARGUMENT",
                Some("Provide --path <dir> to install or --print to output to stdout."),
            );
            process::exit(1);
        }
    };

    if let Err(e) = fs::create_dir_all(&dir) {
        output::error(
            &format!("Failed to create directory '{}': {e}", dir.display()),
            "IO_ERROR",
            None,
        );
        process::exit(1);
    }

    let file_path = dir.join("floo.md");

    let abs_path = match file_path.canonicalize().or_else(|_| {
        // Directory exists but file doesn't yet — canonicalize the parent and append filename
        dir.canonicalize().map(|d| d.join("floo.md"))
    }) {
        Ok(p) => p,
        Err(e) => {
            output::error(
                &format!("Failed to resolve path '{}': {e}", file_path.display()),
                "IO_ERROR",
                None,
            );
            process::exit(1);
        }
    };

    if let Err(e) = fs::write(&abs_path, SKILL_CONTENT) {
        output::error(
            &format!("Failed to write '{}': {e}", abs_path.display()),
            "IO_ERROR",
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
                "IO_ERROR",
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
            "CONFIG_ERROR",
            None,
        );
        process::exit(1);
    }

    output::success(
        &format!("Installed agent skill to {}", abs_path.display()),
        Some(serde_json::json!({
            "path": abs_str,
            "version": VERSION,
        })),
    );
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
                eprintln!(
                    "  Removed stale skill path (directory gone): {path_str}"
                );
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
                    eprintln!(
                        "  Warning: failed to refresh skill at {path_str}: {e}"
                    );
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
        assert!(SKILL_CONTENT.contains("## Authentication"));
        assert!(SKILL_CONTENT.contains("## Command Reference"));
        assert!(SKILL_CONTENT.contains("## Error Codes"));
        assert!(SKILL_CONTENT.contains("--json"));
    }
}
