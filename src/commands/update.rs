use std::process;

use crate::constants::VERSION;
use crate::errors::ErrorCode;
use crate::output;
use crate::updater;

/// Strip the leading `v` from release tag strings for user-facing display.
/// Centralized so the JSON payload, human output, and warning strings all
/// normalize identically — otherwise you get `v0.4.2` in one field and
/// `0.4.2` in another, and that inconsistency is the kind of bug that
/// breaks agent parsing after the fact.
fn display_tag(tag: &str) -> &str {
    tag.strip_prefix('v').unwrap_or(tag)
}

/// Refresh bundled skill files and echo each refreshed path in human mode.
/// Shared by every post-install success arm in both `version()` and
/// `update()` so an install path can't silently drop the refresh (which
/// was a real bug in the earlier version-command refactor).
fn refresh_and_announce_skills() -> Vec<String> {
    let refreshed = super::skills::refresh_skill_files();
    if !output::is_json_mode() {
        for path in &refreshed {
            eprintln!("  Refreshed agent skill at {path}");
        }
    }
    refreshed
}

/// `floo version` — never fatal. The user asked a read-only question
/// ("what version am I on?") and must always get a version-shaped response
/// with exit code 0, even if the update attempt fails for any reason
/// (network, checksum, permission). Scripts like
/// `floo version || echo "floo not installed"` depend on this, and agents
/// parsing `--json` output need a stable shape across every outcome.
///
/// Respects `FLOO_NO_UPDATE_CHECK` and `floo-local` dev builds.
pub fn version() {
    let skip_network =
        std::env::var("FLOO_NO_UPDATE_CHECK").is_ok() || crate::config::is_local_binary();

    if skip_network {
        emit_version(None);
        return;
    }

    // Call run_update unconditionally — it knows how to say "already up to
    // date" via the AlreadyUpToDate error code, so we don't need a separate
    // pre-flight network check. Avoiding the pre-check saves one round trip
    // to GitHub in the update-available case.
    match updater::run_update(None) {
        Ok(result) => {
            refresh_and_announce_skills();
            emit_version(Some(&result.version));
        }
        Err(err) if err.code == ErrorCode::AlreadyUpToDate => {
            refresh_and_announce_skills();
            emit_version(None);
        }
        Err(err) => {
            // Network/checksum/permission failure. Surface the reason so
            // debugging is possible, then print the current version. Never
            // propagate the error — `floo version` is read-only from the
            // user's perspective.
            output::warn(&format!("Update check failed: {}", err.message));
            if let Some(sug) = &err.suggestion {
                output::warn(sug);
            }
            emit_version(None);
        }
    }
}

/// Emit the `floo version` JSON payload + human line.
///
/// `freshly_installed` is `Some(tag)` only when an inline install just
/// completed during this invocation — in that case we report the new tag
/// as `version` and capture the old one as `previous_version` for
/// audit/logging. Otherwise (`None`) we report the currently-running
/// binary with no `previous_version` field. Using an `Option` here rather
/// than a richer enum keeps the two-state shape explicit and the payload
/// literal readable.
fn emit_version(freshly_installed: Option<&str>) {
    let (installed, payload) = match freshly_installed {
        Some(new_tag) => {
            let installed = display_tag(new_tag);
            let payload = serde_json::json!({
                "version": installed,
                "previous_version": display_tag(VERSION),
                "update_available": null,
            });
            (installed.to_string(), payload)
        }
        None => {
            let installed = display_tag(VERSION);
            let payload = serde_json::json!({
                "version": installed,
                "update_available": null,
            });
            (installed.to_string(), payload)
        }
    };
    output::success(&format!("floo {installed}"), Some(payload));
}

pub fn update(version: Option<&str>) {
    if !output::is_json_mode() {
        let label = version.unwrap_or("latest");
        output::info(&format!("Checking for Floo updates ({label})..."), None);
    }

    match updater::run_update(version) {
        Ok(result) => {
            let refreshed = refresh_and_announce_skills();
            output::success(
                &format!("Updated Floo to {}.", result.version),
                Some(serde_json::json!({
                    "version": result.version,
                    "path": result.install_path,
                    "refreshed_skills": refreshed,
                })),
            );
        }
        Err(err) if err.code == ErrorCode::AlreadyUpToDate => {
            let refreshed = refresh_and_announce_skills();
            output::success(
                &err.message,
                Some(serde_json::json!({
                    "version": VERSION,
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

    /// `floo version` in JSON mode must not panic and must not touch the
    /// network (otherwise the test overwrites the cargo test binary via
    /// `run_update` → `install_binary` → `current_exe`). Setting
    /// `FLOO_NO_UPDATE_CHECK=1` exercises the skip-network arm, which is
    /// the one most likely to be wrong in output shape because it has no
    /// network response to key off of.
    #[test]
    fn test_version_skip_network_is_stable() {
        std::env::set_var("FLOO_NO_UPDATE_CHECK", "1");
        output::set_json_mode(true);
        super::version();
        output::set_json_mode(false);
        std::env::remove_var("FLOO_NO_UPDATE_CHECK");
    }

    /// `display_tag` must strip a leading `v` if present and otherwise
    /// return the input unchanged. All three call sites (human output,
    /// JSON `version`, JSON `previous_version`) depend on this being
    /// idempotent so tags and raw version strings interop freely.
    #[test]
    fn test_display_tag_strips_leading_v() {
        assert_eq!(super::display_tag("v0.4.2"), "0.4.2");
        assert_eq!(super::display_tag("0.4.2"), "0.4.2");
        assert_eq!(super::display_tag("v2026.04.12"), "2026.04.12");
        assert_eq!(super::display_tag(""), "");
    }
}
