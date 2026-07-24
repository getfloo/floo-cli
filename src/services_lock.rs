//! Lock file for managed services: `.floo/services.lock`.
//!
//! Auto-generated on `floo services add/remove/migrate`. Committed to the repo
//! so PR reviewers see managed-service state in `git diff` alongside code
//! changes — the GitOps visibility story for stateful resources that live in
//! the CLI rather than `floo.app.toml`.
//!
//! The lock file is a **record** of state, not a source. Hand-editing it does
//! nothing. `floo services add` updates it; the platform is the source of truth.
//! Same model as npm's `package-lock.json`, Cargo.lock, poetry.lock.
//!
//! See docs/knowledge/domains/managed-services.md in getfloo/floo for the
//! canonical spec.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::api_types::ManagedServiceDetail;

pub const LOCK_DIR: &str = ".floo";
pub const LOCK_FILE: &str = "services.lock";

const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ServicesLockFile {
    pub version: u32,
    #[serde(default)]
    pub managed_services: Vec<LockManagedService>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockManagedService {
    #[serde(rename = "type")]
    pub service_type: String,
    pub name: String,
    pub status: String,
    pub created_at: Option<String>,
}

#[derive(Debug)]
pub enum LockError {
    Io(std::io::Error),
    Parse(serde_json::Error),
    Config(String),
    NoProject,
}

impl std::fmt::Display for LockError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LockError::Io(e) => write!(f, "I/O error: {e}"),
            LockError::Parse(e) => write!(f, "parse error: {e}"),
            LockError::Config(message) => write!(f, "config error: {message}"),
            LockError::NoProject => write!(
                f,
                "no floo.app.toml or floo.service.toml found in current directory tree"
            ),
        }
    }
}

impl std::error::Error for LockError {}

impl From<std::io::Error> for LockError {
    fn from(e: std::io::Error) -> Self {
        LockError::Io(e)
    }
}

impl From<serde_json::Error> for LockError {
    fn from(e: serde_json::Error) -> Self {
        LockError::Parse(e)
    }
}

/// Walk upward from `start` to find a floo project root (has floo.app.toml or
/// floo.service.toml). Returns the project root path or None.
fn find_project_root(start: &Path) -> Option<PathBuf> {
    let mut cursor = Some(start);
    while let Some(dir) = cursor {
        if dir.join("floo.app.toml").is_file() || dir.join("floo.service.toml").is_file() {
            return Some(dir.to_path_buf());
        }
        cursor = dir.parent();
    }
    None
}

fn lock_path(project_root: &Path) -> PathBuf {
    project_root.join(LOCK_DIR).join(LOCK_FILE)
}

pub fn read(project_root: &Path) -> Result<ServicesLockFile, LockError> {
    let path = lock_path(project_root);
    if !path.exists() {
        return Ok(ServicesLockFile {
            version: SCHEMA_VERSION,
            managed_services: Vec::new(),
        });
    }
    let contents = fs::read_to_string(&path)?;
    let parsed: ServicesLockFile = serde_json::from_str(&contents)?;
    Ok(parsed)
}

fn write(project_root: &Path, lock: &ServicesLockFile) -> Result<(), LockError> {
    let dir = project_root.join(LOCK_DIR);
    fs::create_dir_all(&dir)?;
    let path = lock_path(project_root);
    // Pretty-printed so git diffs are reviewable. Trailing newline so
    // POSIX-friendly editors don't show "no newline at end of file" warnings.
    let mut body = serde_json::to_string_pretty(lock)?;
    body.push('\n');
    fs::write(path, body)?;
    Ok(())
}

/// Insert or update an entry for a managed service at the given project root.
pub fn record_add_at(project_root: &Path, detail: &ManagedServiceDetail) -> Result<(), LockError> {
    let mut lock = read(project_root)?;
    if lock.version == 0 {
        lock.version = SCHEMA_VERSION;
    }

    let entry = LockManagedService {
        service_type: detail.service_type.clone(),
        name: detail.name.clone(),
        status: detail.status.clone(),
        created_at: detail.created_at.clone(),
    };

    if let Some(existing) = lock
        .managed_services
        .iter_mut()
        .find(|e| e.service_type == entry.service_type && e.name == entry.name)
    {
        *existing = entry;
    } else {
        lock.managed_services.push(entry);
    }

    // Keep deterministic order so diffs stay stable.
    lock.managed_services.sort_by(|a, b| {
        (a.service_type.as_str(), a.name.as_str()).cmp(&(&b.service_type, &b.name))
    });

    write(project_root, &lock)
}

/// Remove the entry for a managed service at the given project root.
pub fn record_remove_at(
    project_root: &Path,
    service_type: &str,
    name: &str,
) -> Result<(), LockError> {
    let mut lock = read(project_root)?;
    lock.managed_services
        .retain(|e| !(e.service_type == service_type && e.name == name));
    write(project_root, &lock)
}

/// Insert or update an entry, resolving project root from the current working
/// directory. Thin wrapper over `record_add_at` for CLI callers.
pub fn record_add(detail: &ManagedServiceDetail) -> Result<(), LockError> {
    let root = current_project_root()?;
    record_add_at(&root, detail)
}

/// Remove an entry, resolving project root from the current working directory.
pub fn record_remove(service_type: &str, name: &str) -> Result<(), LockError> {
    let root = current_project_root()?;
    record_remove_at(&root, service_type, name)
}

fn contains_for_app_at(
    project_root: &Path,
    app_name: &str,
    service_type: &str,
    name: &str,
) -> Result<bool, LockError> {
    let resolved = crate::project_config::resolve_app_context(project_root, None)
        .map_err(|error| LockError::Config(error.message))?;
    if resolved.app_name != app_name {
        return Ok(false);
    }
    let lock = read(project_root)?;
    Ok(lock
        .managed_services
        .iter()
        .any(|entry| entry.service_type == service_type && entry.name == name))
}

/// Whether the matching app's current-project lock records a managed service.
pub fn contains_for_app(app_name: &str, service_type: &str, name: &str) -> Result<bool, LockError> {
    let root = current_project_root()?;
    contains_for_app_at(&root, app_name, service_type, name)
}

fn current_project_root() -> Result<PathBuf, LockError> {
    let cwd = std::env::current_dir()?;
    find_project_root(&cwd).ok_or(LockError::NoProject)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn mk_project(dir: &TempDir) -> PathBuf {
        fs::write(dir.path().join("floo.app.toml"), "[app]\nname = \"test\"\n").unwrap();
        dir.path().to_path_buf()
    }

    #[test]
    fn read_returns_empty_when_no_lock_file_exists() {
        let dir = TempDir::new().unwrap();
        let root = mk_project(&dir);
        let lock = read(&root).unwrap();
        assert_eq!(lock.version, SCHEMA_VERSION);
        assert!(lock.managed_services.is_empty());
    }

    #[test]
    fn write_then_read_roundtrips() {
        let dir = TempDir::new().unwrap();
        let root = mk_project(&dir);
        let lock = ServicesLockFile {
            version: SCHEMA_VERSION,
            managed_services: vec![LockManagedService {
                service_type: "postgres".to_string(),
                name: "default".to_string(),
                status: "ready".to_string(),
                created_at: Some("2026-04-24T00:00:00Z".to_string()),
            }],
        };
        write(&root, &lock).unwrap();
        let round = read(&root).unwrap();
        assert_eq!(round.managed_services.len(), 1);
        assert_eq!(round.managed_services[0].service_type, "postgres");
    }

    #[test]
    fn lock_file_is_pretty_printed_and_has_trailing_newline() {
        let dir = TempDir::new().unwrap();
        let root = mk_project(&dir);
        let lock = ServicesLockFile {
            version: SCHEMA_VERSION,
            managed_services: vec![LockManagedService {
                service_type: "redis".to_string(),
                name: "default".to_string(),
                status: "ready".to_string(),
                created_at: None,
            }],
        };
        write(&root, &lock).unwrap();
        let contents = fs::read_to_string(lock_path(&root)).unwrap();
        assert!(contents.contains('\n'));
        assert!(contents.ends_with('\n'));
        assert!(contents.contains("\"redis\""));
    }

    fn fake_detail(service_type: &str, name: &str) -> ManagedServiceDetail {
        ManagedServiceDetail {
            id: "ms-x".to_string(),
            app_id: "app-x".to_string(),
            service_type: service_type.to_string(),
            name: name.to_string(),
            status: "ready".to_string(),
            env_var_keys: vec![],
            created_at: None,
            updated_at: None,
        }
    }

    #[test]
    fn record_add_at_writes_lock_file() {
        let dir = TempDir::new().unwrap();
        let root = mk_project(&dir);
        record_add_at(&root, &fake_detail("postgres", "default")).unwrap();

        let lock = read(&root).unwrap();
        assert_eq!(lock.managed_services.len(), 1);
        assert_eq!(lock.managed_services[0].service_type, "postgres");
    }

    #[test]
    fn record_add_at_upserts_existing_entry() {
        let dir = TempDir::new().unwrap();
        let root = mk_project(&dir);
        record_add_at(&root, &fake_detail("postgres", "default")).unwrap();
        record_add_at(&root, &fake_detail("postgres", "default")).unwrap();

        let lock = read(&root).unwrap();
        assert_eq!(lock.managed_services.len(), 1);
    }

    #[test]
    fn record_remove_at_drops_matching_entry() {
        let dir = TempDir::new().unwrap();
        let root = mk_project(&dir);
        record_add_at(&root, &fake_detail("postgres", "default")).unwrap();
        record_add_at(&root, &fake_detail("redis", "default")).unwrap();
        record_remove_at(&root, "postgres", "default").unwrap();

        let lock = read(&root).unwrap();
        assert_eq!(lock.managed_services.len(), 1);
        assert_eq!(lock.managed_services[0].service_type, "redis");
    }

    #[test]
    fn contains_for_app_at_refuses_another_apps_lock() {
        let dir = TempDir::new().unwrap();
        let root = mk_project(&dir);
        record_add_at(&root, &fake_detail("postgres", "default")).unwrap();

        assert!(!contains_for_app_at(&root, "another-app", "postgres", "default").unwrap());
        assert!(contains_for_app_at(&root, "test", "postgres", "default").unwrap());
    }

    #[test]
    fn find_project_root_walks_upward() {
        let dir = TempDir::new().unwrap();
        let root = mk_project(&dir);
        let sub = root.join("services").join("api");
        fs::create_dir_all(&sub).unwrap();
        let found = find_project_root(&sub).unwrap();
        assert_eq!(found, root);
    }

    #[test]
    fn find_project_root_returns_none_outside_a_project() {
        let dir = TempDir::new().unwrap();
        // No floo.app.toml anywhere in the tree.
        assert!(find_project_root(dir.path()).is_none());
    }

    #[test]
    fn lock_entries_stay_sorted_for_stable_diffs() {
        let dir = TempDir::new().unwrap();
        let root = mk_project(&dir);
        record_add_at(&root, &fake_detail("storage", "default")).unwrap();
        record_add_at(&root, &fake_detail("postgres", "default")).unwrap();
        record_add_at(&root, &fake_detail("redis", "cache")).unwrap();

        let lock = read(&root).unwrap();
        let order: Vec<(&str, &str)> = lock
            .managed_services
            .iter()
            .map(|e| (e.service_type.as_str(), e.name.as_str()))
            .collect();
        assert_eq!(
            order,
            vec![
                ("postgres", "default"),
                ("redis", "cache"),
                ("storage", "default"),
            ]
        );
    }
}
