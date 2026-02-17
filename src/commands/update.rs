use std::process;

use crate::constants::VERSION;
use crate::output;
use crate::updater;

pub fn version() {
    output::success(
        &format!("floo {VERSION}"),
        Some(serde_json::json!({"version": VERSION})),
    );
}

pub fn update(version: Option<&str>) {
    if !output::is_json_mode() {
        let label = version.unwrap_or("latest");
        output::info(&format!("Checking for Floo updates ({label})..."), None);
    }

    match updater::run_update(version) {
        Ok(result) => {
            output::success(
                &format!("Updated Floo to {}.", result.version),
                Some(serde_json::json!({
                    "version": result.version,
                    "path": result.install_path,
                })),
            );
        }
        Err(err) => {
            output::error(&err.message, &err.code, err.suggestion.as_deref());
            process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::output;

    #[test]
    fn test_version_in_json_mode_does_not_panic() {
        output::set_json_mode(true);
        super::version();
        output::set_json_mode(false);
    }
}
