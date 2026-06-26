//! Substrate guard for CLI-emitted documentation links.
//!
//! floo's docs moved off the `docs.getfloo.com` subdomain (retired, no DNS)
//! behind the `getfloo.com/docs` proxy in getfloo/floo#1140 / #1144. The CLI's
//! built-in docs (`floo docs`, config-error hints, README) kept pointing at the
//! dead subdomain — getfloo/floo#1159. This test fails CI if any source file
//! reintroduces a reference to it, so the whole class stays closed instead of
//! relying on reviewers to spot a rotted host.
//!
//! The needle literal lives here in `tests/`, never under `src/`, so the scan
//! of `src/` below never matches its own definition.

use std::fs;
use std::path::Path;

#[test]
fn no_source_file_references_the_retired_docs_subdomain() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let needle = "docs.getfloo.com";
    let mut offenders = Vec::new();
    collect_offenders(&src, needle, &mut offenders);
    assert!(
        offenders.is_empty(),
        "these source files still reference the retired {needle} subdomain \
         (use getfloo.com/docs/<path> instead): {offenders:#?}",
    );
}

fn collect_offenders(dir: &Path, needle: &str, offenders: &mut Vec<String>) {
    for entry in fs::read_dir(dir).expect("src/ is readable") {
        let path = entry.expect("dir entry is readable").path();
        if path.is_dir() {
            collect_offenders(&path, needle, offenders);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            let body = fs::read_to_string(&path).expect("source file is readable");
            if body.contains(needle) {
                offenders.push(path.display().to_string());
            }
        }
    }
}
