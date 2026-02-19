use std::fs::{self, File};
use std::path::{Path, PathBuf};

use flate2::write::GzEncoder;
use flate2::Compression;

use crate::constants::MAX_ARCHIVE_SIZE_MB;
use crate::errors::FlooError;

const DEFAULT_IGNORE_PATTERNS: &[&str] = &[
    ".git",
    "node_modules",
    "__pycache__",
    ".venv",
    "venv",
    ".env",
    "*.pyc",
    ".DS_Store",
    "target", // Rust/Maven build output
    "dist",   // JS/Python distribution output
    "build",  // Generic build output
    ".next",  // Next.js build cache
];

fn load_flooignore(path: &Path) -> Result<Vec<String>, FlooError> {
    let ignore_file = path.join(".flooignore");
    if !ignore_file.exists() {
        return Ok(Vec::new());
    }
    let content = fs::read_to_string(&ignore_file).map_err(|e| {
        FlooError::with_suggestion(
            "FLOOIGNORE_READ_FAILED",
            format!("Failed to read {}: {e}", ignore_file.display()),
            "Fix .flooignore permissions or symlink target and try again.",
        )
    })?;
    Ok(content
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| l.to_string())
        .collect())
}

fn matches_pattern(name: &str, pattern: &str) -> bool {
    // Simple fnmatch-style matching: supports * and ? wildcards
    fn do_match(name: &[u8], pattern: &[u8]) -> bool {
        let (mut ni, mut pi) = (0, 0);
        let (mut star_pi, mut star_ni) = (usize::MAX, 0);

        while ni < name.len() {
            if pi < pattern.len() && (pattern[pi] == b'?' || pattern[pi] == name[ni]) {
                ni += 1;
                pi += 1;
            } else if pi < pattern.len() && pattern[pi] == b'*' {
                star_pi = pi;
                star_ni = ni;
                pi += 1;
            } else if star_pi != usize::MAX {
                pi = star_pi + 1;
                star_ni += 1;
                ni = star_ni;
            } else {
                return false;
            }
        }

        while pi < pattern.len() && pattern[pi] == b'*' {
            pi += 1;
        }

        pi == pattern.len()
    }

    do_match(name.as_bytes(), pattern.as_bytes())
}

fn should_ignore(name: &str, rel_path: &str, patterns: &[String]) -> bool {
    for pattern in patterns {
        if matches_pattern(name, pattern) || matches_pattern(rel_path, pattern) {
            return true;
        }
    }
    false
}

pub fn create_archive(path: &Path) -> Result<PathBuf, FlooError> {
    let mut patterns: Vec<String> = DEFAULT_IGNORE_PATTERNS
        .iter()
        .map(|s| s.to_string())
        .collect();
    patterns.extend(load_flooignore(path)?);

    let tmp = std::env::temp_dir().join(format!(
        "floo-{}-{}.tar.gz",
        std::process::id(),
        rand::random::<u32>()
    ));
    let file = File::create(&tmp)
        .map_err(|e| FlooError::new("ARCHIVE_ERROR", format!("Failed to create archive: {e}")))?;
    let enc = GzEncoder::new(file, Compression::default());
    let mut tar = tar::Builder::new(enc);

    add_dir_to_tar(&mut tar, path, path, &patterns)?;

    let enc = tar
        .into_inner()
        .map_err(|e| FlooError::new("ARCHIVE_ERROR", format!("Failed to finalize archive: {e}")))?;
    enc.finish()
        .map_err(|e| FlooError::new("ARCHIVE_ERROR", format!("Failed to compress archive: {e}")))?;

    let size_mb = fs::metadata(&tmp).map(|m| m.len()).unwrap_or(0) as f64 / (1024.0 * 1024.0);

    if size_mb > MAX_ARCHIVE_SIZE_MB as f64 {
        let _ = fs::remove_file(&tmp);
        return Err(FlooError::with_suggestion(
            "ARCHIVE_TOO_LARGE",
            format!("Archive is {size_mb:.0}MB, exceeding the {MAX_ARCHIVE_SIZE_MB}MB limit."),
            "Add large files to .flooignore to reduce archive size.",
        ));
    }

    Ok(tmp)
}

fn add_dir_to_tar<W: std::io::Write>(
    tar: &mut tar::Builder<W>,
    root: &Path,
    current: &Path,
    patterns: &[String],
) -> Result<(), FlooError> {
    let entries = fs::read_dir(current)
        .map_err(|e| FlooError::new("ARCHIVE_ERROR", format!("Failed to read directory: {e}")))?;

    for entry in entries {
        let entry = entry
            .map_err(|e| FlooError::new("ARCHIVE_ERROR", format!("Failed to read entry: {e}")))?;
        let full_path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        let rel_path = full_path
            .strip_prefix(root)
            .unwrap_or(&full_path)
            .to_string_lossy()
            .to_string();

        if should_ignore(&name, &rel_path, patterns) {
            continue;
        }

        if full_path.is_dir() {
            add_dir_to_tar(tar, root, &full_path, patterns)?;
        } else {
            tar.append_path_with_name(&full_path, &rel_path)
                .map_err(|e| {
                    FlooError::new(
                        "ARCHIVE_ERROR",
                        format!("Failed to add file {rel_path}: {e}"),
                    )
                })?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    use tempfile::TempDir;

    #[test]
    fn test_basic_archive() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("index.js"), "console.log('hi')").unwrap();
        fs::write(dir.path().join("package.json"), "{}").unwrap();

        let archive = create_archive(dir.path()).unwrap();
        assert!(archive.exists());

        // Verify contents
        let file = File::open(&archive).unwrap();
        let dec = flate2::read::GzDecoder::new(file);
        let mut tar = tar::Archive::new(dec);
        let names: Vec<String> = tar
            .entries()
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.path().unwrap().to_string_lossy().to_string())
            .collect();
        assert!(names.contains(&"index.js".to_string()));
        assert!(names.contains(&"package.json".to_string()));

        let _ = fs::remove_file(&archive);
    }

    #[test]
    fn test_git_excluded() {
        let dir = TempDir::new().unwrap();
        fs::create_dir(dir.path().join(".git")).unwrap();
        fs::write(dir.path().join(".git").join("config"), "").unwrap();
        fs::write(dir.path().join("app.js"), "").unwrap();

        let archive = create_archive(dir.path()).unwrap();
        let file = File::open(&archive).unwrap();
        let dec = flate2::read::GzDecoder::new(file);
        let mut tar = tar::Archive::new(dec);
        let names: Vec<String> = tar
            .entries()
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.path().unwrap().to_string_lossy().to_string())
            .collect();
        assert!(!names.iter().any(|n| n.contains(".git")));
        assert!(names.contains(&"app.js".to_string()));

        let _ = fs::remove_file(&archive);
    }

    #[test]
    fn test_node_modules_excluded() {
        let dir = TempDir::new().unwrap();
        fs::create_dir(dir.path().join("node_modules")).unwrap();
        fs::write(dir.path().join("node_modules").join("foo.js"), "").unwrap();
        fs::write(dir.path().join("app.js"), "").unwrap();

        let archive = create_archive(dir.path()).unwrap();
        let file = File::open(&archive).unwrap();
        let dec = flate2::read::GzDecoder::new(file);
        let mut tar = tar::Archive::new(dec);
        let names: Vec<String> = tar
            .entries()
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.path().unwrap().to_string_lossy().to_string())
            .collect();
        assert!(!names.iter().any(|n| n.contains("node_modules")));

        let _ = fs::remove_file(&archive);
    }

    #[test]
    fn test_flooignore_patterns() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join(".flooignore"), "*.log\nbuild").unwrap();
        fs::write(dir.path().join("app.js"), "").unwrap();
        fs::write(dir.path().join("debug.log"), "").unwrap();
        fs::create_dir(dir.path().join("build")).unwrap();
        fs::write(dir.path().join("build").join("out.js"), "").unwrap();

        let archive = create_archive(dir.path()).unwrap();
        let file = File::open(&archive).unwrap();
        let dec = flate2::read::GzDecoder::new(file);
        let mut tar = tar::Archive::new(dec);
        let names: Vec<String> = tar
            .entries()
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.path().unwrap().to_string_lossy().to_string())
            .collect();
        assert!(names.contains(&"app.js".to_string()));
        assert!(!names.iter().any(|n| n.contains("debug.log")));
        assert!(!names.iter().any(|n| n.contains("build")));

        let _ = fs::remove_file(&archive);
    }

    #[cfg(unix)]
    #[test]
    fn test_unreadable_flooignore_fails() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("app.js"), "").unwrap();
        let ignore_path = dir.path().join(".flooignore");
        fs::write(&ignore_path, "*.log\n").unwrap();

        let original_mode = fs::metadata(&ignore_path).unwrap().permissions().mode();
        fs::set_permissions(&ignore_path, fs::Permissions::from_mode(0o000)).unwrap();

        let result = create_archive(dir.path());

        fs::set_permissions(&ignore_path, fs::Permissions::from_mode(original_mode)).unwrap();
        let error = result.unwrap_err();
        assert_eq!(error.code, "FLOOIGNORE_READ_FAILED");
        assert!(error.message.contains(".flooignore"));
    }

    #[test]
    fn test_pyc_excluded() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("app.py"), "").unwrap();
        fs::write(dir.path().join("app.pyc"), "").unwrap();

        let archive = create_archive(dir.path()).unwrap();
        let file = File::open(&archive).unwrap();
        let dec = flate2::read::GzDecoder::new(file);
        let mut tar = tar::Archive::new(dec);
        let names: Vec<String> = tar
            .entries()
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.path().unwrap().to_string_lossy().to_string())
            .collect();
        assert!(names.contains(&"app.py".to_string()));
        assert!(!names.contains(&"app.pyc".to_string()));

        let _ = fs::remove_file(&archive);
    }

    #[test]
    fn test_matches_pattern() {
        assert!(matches_pattern("foo.pyc", "*.pyc"));
        assert!(matches_pattern(".git", ".git"));
        assert!(matches_pattern("node_modules", "node_modules"));
        assert!(!matches_pattern("app.js", "*.pyc"));
        assert!(matches_pattern(".DS_Store", ".DS_Store"));
    }

    #[test]
    fn test_build_dirs_excluded() {
        let dir = TempDir::new().unwrap();

        for build_dir in &["target", "dist", "build", ".next"] {
            fs::create_dir(dir.path().join(build_dir)).unwrap();
            fs::write(dir.path().join(build_dir).join("output.js"), "").unwrap();
        }
        fs::write(dir.path().join("app.js"), "").unwrap();

        let archive = create_archive(dir.path()).unwrap();
        let file = File::open(&archive).unwrap();
        let dec = flate2::read::GzDecoder::new(file);
        let mut tar = tar::Archive::new(dec);
        let names: Vec<String> = tar
            .entries()
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.path().unwrap().to_string_lossy().to_string())
            .collect();

        assert!(names.contains(&"app.js".to_string()));
        for build_dir in &["target", "dist", "build", ".next"] {
            assert!(
                !names.iter().any(|n| n.starts_with(build_dir)),
                "{build_dir} should be excluded"
            );
        }

        let _ = fs::remove_file(&archive);
    }
}
