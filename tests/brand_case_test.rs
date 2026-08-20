use std::fs;
use std::path::{Path, PathBuf};

const CAPITALIZED_PRODUCT_NAME: &str = concat!("Fl", "oo");

fn collect_files(directory: &Path, files: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(directory).expect("read repository directory") {
        let entry = entry.expect("read repository entry");
        let path = entry.path();
        let file_name = entry.file_name();

        if path.is_dir() {
            if file_name != ".git" && file_name != "target" {
                collect_files(&path, files);
            }
        } else if path.is_file() {
            files.push(path);
        }
    }
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
    let mut files = Vec::new();
    collect_files(root, &mut files);

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
