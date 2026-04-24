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

/// Should `floo version` skip the network check entirely?
///
/// Load-bearing for install.sh: the post-install `floo --version` check
/// runs with `FLOO_NO_UPDATE_CHECK=1` set so that a fresh install doesn't
/// try to reach GitHub AND doesn't try to auto-update mid-install. Any
/// change to this predicate that stops honoring the env var will make
/// install.sh hit the network on every install, slow the install flow,
/// and potentially cause infinite install→update→install loops if the
/// update path itself is broken.
///
/// Also honored for `floo-local` dev builds so local development never
/// attempts to auto-update over the developer's working binary.
fn should_skip_network_check() -> bool {
    std::env::var("FLOO_NO_UPDATE_CHECK").is_ok() || crate::config::is_local_binary()
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
    if should_skip_network_check() {
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
/// binary with no `previous_version` field.
///
/// Output routing is deliberately non-obvious because `floo version` has
/// two consumers with different expectations:
///   - JSON mode: the structured payload on stdout (via output::success).
///     Agents pipe `floo version --json | jq` and need a stable shape.
///   - Human mode: the `✓ floo X.Y.Z` status line on stderr (floo's
///     conventional colored output) PLUS a bare `X.Y.Z` on stdout
///     (Unix `--version` convention). Scripts and install.sh both
///     capture stdout from `floo --version`, so stdout must be
///     non-empty and machine-parseable. The bare tag on stdout also
///     matches what users get from `git --version`, `curl --version`,
///     etc.
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

    // In JSON mode, output::success writes the structured payload to stdout
    // and we're done. In human mode, it writes the colored status line to
    // stderr — which install.sh does NOT capture — so we must also write
    // the bare version tag to stdout via raw_value.
    if !output::is_json_mode() {
        output::raw_value(&installed);
    }
    output::success(&format!("floo {installed}"), Some(payload));
}

pub fn update(version: Option<&str>) {
    // Dry-run: resolve the target release but never download or install.
    // Previously --dry-run silently fell through to run_update(), which
    // replaces the binary on disk. See feedback 7b98b798.
    if output::is_dry_run_mode() {
        match updater::check_update(version) {
            Ok(plan) => {
                let message = if plan.already_up_to_date {
                    format!(
                        "floo {} is already the latest version.",
                        plan.current_version
                    )
                } else {
                    format!(
                        "Would update floo from {} to {}.",
                        plan.current_version,
                        display_tag(&plan.target_version),
                    )
                };
                output::dry_run_success(serde_json::json!({
                    "action": "update",
                    "current_version": plan.current_version,
                    "target_version": display_tag(&plan.target_version),
                    "install_path": plan.install_path,
                    "already_up_to_date": plan.already_up_to_date,
                    "message": message,
                }));
            }
            Err(err) => {
                output::error(&err.message, &err.code, err.suggestion.as_deref());
                process::exit(1);
            }
        }
        return;
    }

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

    /// `FLOO_NO_UPDATE_CHECK=1` MUST cause `should_skip_network_check()`
    /// to return true. install.sh depends on this: the post-install
    /// `floo --version` check runs with this env var set so that a
    /// fresh install doesn't reach GitHub or attempt to auto-update
    /// mid-install. If this contract ever silently breaks, every fresh
    /// install would start hitting the network, slowing installs and
    /// potentially causing install→update→install loops if the update
    /// path is broken.
    ///
    /// The SAFETY lock for this test is its name and doc comment, not
    /// the assertion alone — a refactor that renames the env var or
    /// moves the check elsewhere will need a reviewer to explicitly
    /// update this test, which is exactly the friction we want.
    #[test]
    fn test_floo_no_update_check_env_var_is_install_sh_contract() {
        // Save + restore any existing value so we don't pollute other
        // tests running in the same process.
        let prior = std::env::var("FLOO_NO_UPDATE_CHECK").ok();

        std::env::remove_var("FLOO_NO_UPDATE_CHECK");
        // Without the env var set (and assuming the test binary name is
        // not "floo-local"), the predicate may still return true on a
        // local dev binary. Only assert the env-var-driven branch.
        std::env::set_var("FLOO_NO_UPDATE_CHECK", "1");
        assert!(
            super::should_skip_network_check(),
            "FLOO_NO_UPDATE_CHECK=1 must short-circuit the network check — install.sh depends on this"
        );

        // Also verify an empty string counts as "set" (matches the
        // `is_ok()` check, not `!= \"\"`). Users and scripts set env
        // vars to "" all the time; the contract is "set at all".
        std::env::set_var("FLOO_NO_UPDATE_CHECK", "");
        assert!(
            super::should_skip_network_check(),
            "FLOO_NO_UPDATE_CHECK=\"\" must also short-circuit — is_ok() is the semantic"
        );

        // Restore whatever the environment had before.
        match prior {
            Some(v) => std::env::set_var("FLOO_NO_UPDATE_CHECK", v),
            None => std::env::remove_var("FLOO_NO_UPDATE_CHECK"),
        }
    }
}
