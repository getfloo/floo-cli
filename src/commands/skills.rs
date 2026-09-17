use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process;

use colored::Colorize;
use sha2::{Digest, Sha256};

use crate::config::{load_config, save_config};
use crate::constants::VERSION;
use crate::errors::ErrorCode;
use crate::output;

const SKILL_CONTENT: &str = include_str!("../../plugin/skills/floo/SKILL.md");
// Exact hashes of previously bundled skills. Preserve user-edited files.
const RETIRED_SKILLS: &[(&str, &[&str])] = &[
    (
        "floo-services",
        &[
            "20fddf528159335b25121ff369d59dcef33c34281d668709aaa6d40f4f5d889e",
            "2d053bd8886486ecc2192356bf3ff69a1972cb6e9d3a5512c0b7b7891392c4d1",
            "3633afa6a160b7177701bf0be11c1ef8765f55316551132fb2ecbd6ef4298cef",
            "7dc5606d43e323b04324d34b09aab4190517e4d1486d4aa61dfbeeae8ac88566",
            "9b3afdf8e98edcc97b1671c9c5cea5eb0518eefe7bf825470bd31bd4e908bc29",
            "a5e23e6ebfc15ecbba91b4bcf46e176db48a3d1f8738e78051986cdb4638804b",
            "c79d5ce7c3915f1ab977a001a8ba75a9471980d70a054e32f543c6a4f31c8883",
            "fee270e9757f20c74b00d5ef0a443e35f34b794165a4e8fae756cde0aa843d24",
        ],
    ),
    (
        "floo-security",
        &[
            "597693fa1f42d69c5375ee7bc7f38a5a57ea648f83c75371f3a7718fd0f71df0",
            "77199cdde5970bc7d9dabf9542f37ef3365261a723af10cf353df3c41a789e24",
            "86bdb9f5692370b20b47545607c9afe38af492977fc4fa1e4a4886177f318974",
            "f78bac846c502d44cd87c0d3d76b9978602efcac9714115cb11932b52b5679a3",
        ],
    ),
];

pub fn install(path: Option<PathBuf>, print: bool) {
    if print {
        if output::is_json_mode() {
            output::success(
                "Skill content",
                Some(serde_json::json!({
                    "content": SKILL_CONTENT,
                    "plugin_skills": [],
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

    if let Err(e) = remove_retired_skills(&dir, RETIRED_SKILLS) {
        output::error(
            &format!("Failed to retire old skills: {e}"),
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
            &format!("Installed floo skill to {}", dir.display()),
            Some(serde_json::json!({
                "path": abs_str,
                "plugin_skills": [],
                "version": VERSION,
                "recommended_permissions": {
                    "read_only": read_only,
                    "read_write": read_write,
                },
            })),
        );
    } else {
        output::success(&format!("Installed floo skill to {}", dir.display()), None);
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

/// Remove only unchanged retired skill files; never follow user-created symlinks.
fn remove_retired_skills(dir: &Path, retired: &[(&str, &[&str])]) -> io::Result<bool> {
    let mut changed = false;
    for (name, hashes) in retired {
        let directory = dir.join(name);
        let path = directory.join("SKILL.md");
        if directory.is_symlink() || path.is_symlink() {
            continue;
        }
        let content = match fs::read(&path) {
            Ok(content) => content,
            Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e),
        };
        let hash = format!("{:x}", Sha256::digest(&content));
        if hashes.contains(&hash.as_str()) {
            fs::remove_file(path)?;
            changed = true;
        }
    }
    Ok(changed)
}

/// Refresh the tracked entry point and retire unchanged companion skills.
fn refresh_skill_bundle(path: &Path) -> io::Result<bool> {
    let mut changed = write_if_changed(path, SKILL_CONTENT)?;
    if let Some(parent) = path.parent() {
        changed |= remove_retired_skills(parent, RETIRED_SKILLS)?;
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
    permission!(ReadOnly, ["domains", "show"]),
    permission!(ReadOnly, ["domains", "watch"]),
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
    fn skill_refresh_updates_stale_content_once() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("SKILL.md");
        fs::write(&path, "stale entry point").unwrap();
        assert!(refresh_skill_bundle(&path).unwrap());
        assert_eq!(fs::read_to_string(&path).unwrap(), SKILL_CONTENT);
        assert!(!refresh_skill_bundle(&path).unwrap());
    }

    #[test]
    fn retirement_removes_only_known_content_and_preserves_other_files() {
        let dir = tempfile::tempdir().unwrap();
        let legacy_dir = dir.path().join("floo-services");
        fs::create_dir(&legacy_dir).unwrap();
        let path = legacy_dir.join("SKILL.md");
        fs::write(&path, "old bundled skill").unwrap();
        fs::write(legacy_dir.join("notes.md"), "user notes").unwrap();
        let hash = format!("{:x}", Sha256::digest(b"old bundled skill"));
        let retired = [("floo-services", &[hash.as_str()][..])];

        assert!(remove_retired_skills(dir.path(), &retired).unwrap());
        assert!(!path.exists());
        assert_eq!(
            fs::read_to_string(legacy_dir.join("notes.md")).unwrap(),
            "user notes"
        );
        assert!(!remove_retired_skills(dir.path(), &retired).unwrap());
    }

    #[test]
    fn retirement_preserves_customized_skills() {
        let dir = tempfile::tempdir().unwrap();
        let legacy_dir = dir.path().join("floo-security");
        fs::create_dir(&legacy_dir).unwrap();
        let path = legacy_dir.join("SKILL.md");
        fs::write(&path, "custom instructions").unwrap();

        assert!(!remove_retired_skills(dir.path(), RETIRED_SKILLS).unwrap());
        assert_eq!(fs::read_to_string(path).unwrap(), "custom instructions");
    }

    #[cfg(unix)]
    #[test]
    fn retirement_does_not_follow_symlinked_directories() {
        let dir = tempfile::tempdir().unwrap();
        let external = tempfile::tempdir().unwrap();
        fs::write(external.path().join("SKILL.md"), "old bundled skill").unwrap();
        std::os::unix::fs::symlink(external.path(), dir.path().join("floo-services")).unwrap();
        let hash = format!("{:x}", Sha256::digest(b"old bundled skill"));
        let retired = [("floo-services", &[hash.as_str()][..])];

        assert!(!remove_retired_skills(dir.path(), &retired).unwrap());
        assert!(external.path().join("SKILL.md").exists());
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
