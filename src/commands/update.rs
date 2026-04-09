use std::process;

use crate::constants::VERSION;
use crate::output;
use crate::updater;

pub fn version() {
    let update_available = crate::version_check::check_latest_version(VERSION);

    if !output::is_json_mode() {
        if let Some(ref newer) = update_available {
            output::success(
                &format!("floo {VERSION}"),
                None,
            );
            let newer_display = newer.strip_prefix('v').unwrap_or(newer);
            eprintln!("  Update available: {newer_display} → will auto-apply on next run, or run `floo update`");
            return;
        }
    }

    output::success(
        &format!("floo {VERSION}"),
        Some(serde_json::json!({
            "version": VERSION,
            "update_available": update_available,
        })),
    );
}

pub fn update(version: Option<&str>) {
    if !output::is_json_mode() {
        let label = version.unwrap_or("latest");
        output::info(&format!("Checking for Floo updates ({label})..."), None);
    }

    match updater::run_update(version) {
        Ok(result) => {
            let refreshed = super::skills::refresh_skill_files();

            if !output::is_json_mode() {
                for path in &refreshed {
                    eprintln!("  Refreshed agent skill at {path}");
                }
            }

            output::success(
                &format!("Updated Floo to {}.", result.version),
                Some(serde_json::json!({
                    "version": result.version,
                    "path": result.install_path,
                    "refreshed_skills": refreshed,
                })),
            );
        }
        Err(err) if err.code == crate::errors::ErrorCode::AlreadyUpToDate => {
            let refreshed = super::skills::refresh_skill_files();
            if !output::is_json_mode() {
                for path in &refreshed {
                    eprintln!("  Refreshed agent skill at {path}");
                }
            }
            output::success(
                &err.message,
                Some(serde_json::json!({
                    "version": crate::constants::VERSION,
                    "already_latest": true,
                    "refreshed_skills": refreshed,
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
