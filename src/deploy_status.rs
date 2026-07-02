//! Single source of truth for classifying a deploy's status.
//!
//! Every command that polls or watches a deploy and reports its outcome routes
//! its terminal-membership and failure (exit-code) decisions through here, so
//! adding a new status is a one-place change instead of an audit across every
//! completion site. getfloo/floo#1354 shipped `cancelled` and review still found
//! two sites (`apps github connect`, `previews up --wait`) that had independently
//! hand-rolled "anything but live is a failure" and so misreported a cancelled
//! deploy as a failure — this module exists to close that class.

/// Terminal deploy statuses: a poll/stream loop stops once a deploy reaches one.
const TERMINAL: &[&str] = &["live", "failed", "superseded", "cancelled"];

/// How a deploy that has reached a terminal status should be reported.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Terminal {
    /// Reached LIVE — a genuine success.
    Live,
    /// Failed — the ONLY status that warrants a non-zero exit; operator-actionable.
    Failed,
    /// A newer deploy replaced this one — benign, exit 0.
    Superseded,
    /// The target environment was torn down before the deploy ran — benign,
    /// exit 0 (getfloo/floo#1354).
    Cancelled,
}

/// True if `status` is a terminal state (a poll/stream loop should stop here).
pub fn is_terminal(status: &str) -> bool {
    TERMINAL.contains(&status)
}

/// Classify a terminal status; `None` while the deploy is still in progress.
pub fn classify(status: &str) -> Option<Terminal> {
    match status {
        "live" => Some(Terminal::Live),
        "failed" => Some(Terminal::Failed),
        "superseded" => Some(Terminal::Superseded),
        "cancelled" => Some(Terminal::Cancelled),
        _ => None,
    }
}

/// True only for a genuine FAILURE — the single status that warrants a non-zero
/// exit. `live` and the moot terminals (`superseded`/`cancelled`) are NOT failures,
/// and neither is any in-progress status.
pub fn is_failure(status: &str) -> bool {
    classify(status) == Some(Terminal::Failed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_membership() {
        // Every settled status must be terminal or the poll/stream loops spin for
        // POLL_TIMEOUT (10 min): `superseded` was the original miss (feedback
        // 1748af72, 2026-04-24) and `cancelled` the more recent one (getfloo/floo#1354).
        for s in ["live", "failed", "superseded", "cancelled"] {
            assert!(is_terminal(s), "{s} should be terminal");
        }
        for s in [
            "pending",
            "building",
            "deploying",
            "configuring_routing",
            "",
        ] {
            assert!(!is_terminal(s), "{s} should not be terminal");
        }
    }

    #[test]
    fn only_failed_is_a_failure() {
        assert!(is_failure("failed"));
        // Moot terminals and live must NOT read as failures — they exit 0.
        assert!(!is_failure("cancelled"));
        assert!(!is_failure("superseded"));
        assert!(!is_failure("live"));
        // In-progress / unknown is not (yet) a failure.
        assert!(!is_failure("building"));
        assert!(!is_failure("whatever"));
    }

    #[test]
    fn classify_maps_each_terminal_status() {
        assert_eq!(classify("live"), Some(Terminal::Live));
        assert_eq!(classify("failed"), Some(Terminal::Failed));
        assert_eq!(classify("superseded"), Some(Terminal::Superseded));
        assert_eq!(classify("cancelled"), Some(Terminal::Cancelled));
        // Non-terminal / unknown statuses do not classify.
        assert_eq!(classify("building"), None);
        assert_eq!(classify("whatever"), None);
    }
}
