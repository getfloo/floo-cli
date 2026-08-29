use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const CAPITALIZED_PRODUCT_NAME: &str = concat!("Fl", "oo");

/// Every file Git tracks, and nothing else.
///
/// A directory walk that skips only `.git` and `target` still descends into
/// `.claude/worktrees/`, where this repo keeps its agent worktrees. Those hold
/// older checkouts of this same repository, so the walk reported their stale
/// capitalizations as violations of the current tree. That failed for anyone
/// using a worktree while passing in CI, where a fresh clone has no such
/// directory.
///
/// Asking Git keeps the scan to what the repository actually ships, and needs
/// no exclusion list to maintain as new tooling directories appear.
fn tracked_files(root: &Path) -> Vec<PathBuf> {
    let output = Command::new("git")
        .args(["ls-files", "-z"])
        .current_dir(root)
        .output()
        .expect("run `git ls-files` from the repository root");

    assert!(
        output.status.success(),
        "`git ls-files` failed in {}: {}",
        root.display(),
        String::from_utf8_lossy(&output.stderr).trim()
    );

    let stdout = String::from_utf8(output.stdout).expect("`git ls-files` emits UTF-8 paths");
    let files: Vec<PathBuf> = stdout
        .split('\0')
        .filter(|entry| !entry.is_empty())
        .map(|entry| root.join(entry))
        .collect();

    assert!(
        !files.is_empty(),
        "`git ls-files` returned no files in {}; the scan would pass vacuously",
        root.display()
    );

    files
}

fn has_non_protocol_product_name(line: &str) -> bool {
    line.match_indices(CAPITALIZED_PRODUCT_NAME)
        .any(|(index, product_name)| {
            let bytes = line.as_bytes();
            let previous = bytes[..index].last().copied();
            let next = bytes[index + product_name.len()..].first().copied();
            let is_word_byte = |byte: u8| byte.is_ascii_alphanumeric() || byte == b'_';
            let is_standalone = previous.is_none_or(|byte| !is_word_byte(byte))
                && next.is_none_or(|byte| !is_word_byte(byte));

            is_standalone && !line[..index].ends_with("X-")
        })
}

#[test]
fn product_name_is_lowercase_across_the_cli_repository() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let files = tracked_files(root);

    let mut violations = Vec::new();
    for path in files {
        let Ok(contents) = fs::read_to_string(&path) else {
            continue;
        };
        for (line_index, line) in contents.lines().enumerate() {
            if has_non_protocol_product_name(line) {
                let relative = path.strip_prefix(root).unwrap_or(&path);
                violations.push(format!("{}:{}: {line}", relative.display(), line_index + 1));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "the floo product name must be lowercase; protocol header names are the only exception:\n{}",
        violations.join("\n")
    );
}
