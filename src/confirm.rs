//! Destructive-command confirmation — CLI-wide tier system.
//!
//! Every destructive floo CLI command picks a tier at design time. The tier
//! decides the confirmation UX, the flag names, and the JSON metadata agents
//! see in the contract. Inconsistency here is a safety regression — users and
//! agents develop reflexes around these patterns and every one-off UX decays
//! the reflex.
//!
//! See docs/knowledge/running-the-firm/state-and-audit-doctrine.md →
//! "Confirmation tiers" for the canonical definitions.
//!
//! ## The three tiers
//!
//! | Tier | When to use | UX |
//! |------|-------------|----|
//! | 1 | Reversible, no data (env unset, scaling) | No prompt, no flag |
//! | 2 | Destructive but recoverable from code (domain remove, rollback) | `y/N` prompt, `--yes` to skip |
//! | 3 | Unrecoverable data loss (apps delete, managed-service remove) | Type the resource name, `--yes-i-know-this-destroys-data` to skip |
//!
//! **Tier 3 never uses a plain `--yes`.** Destroying user data must be
//! physically harder to shortcut than every other destructive operation.

use std::io::{self, IsTerminal, Write};

use serde::Serialize;

use crate::errors::ErrorCode;
use crate::output;

/// Destructive-command tier. Determines confirmation UX and JSON metadata.
///
/// Tier::One exists in the enum so the doctrine (three tiers, not two) is
/// represented in code, but tier-1 commands never need to call any confirm_*
/// helper — they're reversible and idempotent. The variant is kept for future
/// tier-1 callers that want to emit RiskMetadata in their JSON output, and
/// for exhaustive matching in helpers that treat all three tiers uniformly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[allow(dead_code)]
pub enum Tier {
    /// Reversible, no data loss (e.g., env remove, scaling).
    One = 1,
    /// Destructive but recoverable from code (e.g., domain remove, rollback).
    Two = 2,
    /// Unrecoverable data loss (e.g., apps delete, managed-service remove).
    Three = 3,
}

impl Tier {
    pub fn is_destructive(self) -> bool {
        !matches!(self, Tier::One)
    }

    pub fn implies_data_loss(self) -> bool {
        matches!(self, Tier::Three)
    }
}

/// Outcome of a confirmation prompt. The caller decides what to do with `Aborted`
/// (usually: exit 0 with "Cancelled." message); `Refused` is a hard error
/// because it means the command was invoked in a way that is structurally
/// unable to confirm (JSON mode or non-interactive without the required flag).
pub enum ConfirmOutcome {
    Proceed,
    Aborted,
    Refused { suggestion: String },
}

/// Tier-2 confirmation: `y/N` prompt, `--yes` bypasses.
///
/// `action` is the short verb-noun the user will see (e.g. "Remove domain",
/// "Cancel deploy"). `subject` is the specific resource (e.g. "api.example.com
/// on my-app"). Together they become `"{action} {subject}?"`.
pub fn confirm_tier2(action: &str, subject: &str, yes_flag: bool) -> ConfirmOutcome {
    if yes_flag {
        return ConfirmOutcome::Proceed;
    }

    if output::is_json_mode() {
        return ConfirmOutcome::Refused {
            suggestion: "Pass --yes to confirm in non-interactive mode.".to_string(),
        };
    }

    if !io::stdin().is_terminal() {
        return ConfirmOutcome::Refused {
            suggestion:
                "Pass --yes to confirm when stdin isn't a terminal (CI, pipes, etc.)."
                    .to_string(),
        };
    }

    eprint!("{action} {subject}? [y/N] ");
    let _ = io::stderr().flush();
    let mut input = String::new();
    match io::stdin().read_line(&mut input) {
        Ok(_) => {
            if matches!(input.trim().to_lowercase().as_str(), "y" | "yes") {
                ConfirmOutcome::Proceed
            } else {
                ConfirmOutcome::Aborted
            }
        }
        Err(_) => ConfirmOutcome::Aborted,
    }
}

/// Tier-3 confirmation: require typing the resource name.
/// `--yes-i-know-this-destroys-data` bypasses. Never a plain `--yes`.
///
/// `resource_name` is the string the user must type back to confirm (e.g. the
/// app name, the service name). `preamble_lines` are rendered before the
/// prompt so the user sees exactly what's about to be destroyed.
pub fn confirm_tier3(
    resource_name: &str,
    preamble_lines: &[String],
    data_loss_flag: bool,
) -> ConfirmOutcome {
    if data_loss_flag {
        return ConfirmOutcome::Proceed;
    }

    let suggestion = "Pass --yes-i-know-this-destroys-data to confirm in non-interactive mode. This flag is deliberately verbose; a script using it must have user authorization for this specific resource.".to_string();

    if output::is_json_mode() {
        return ConfirmOutcome::Refused { suggestion };
    }

    if !io::stdin().is_terminal() {
        return ConfirmOutcome::Refused { suggestion };
    }

    eprintln!();
    for line in preamble_lines {
        eprintln!("{line}");
    }
    eprintln!();
    eprintln!("This is irreversible. Data will not be recoverable.");
    eprintln!("Type the resource name ({resource_name}) to confirm, or anything else to cancel:");
    eprint!("> ");
    let _ = io::stderr().flush();

    let mut input = String::new();
    match io::stdin().read_line(&mut input) {
        Ok(_) => {
            if input.trim() == resource_name {
                ConfirmOutcome::Proceed
            } else {
                ConfirmOutcome::Aborted
            }
        }
        Err(_) => ConfirmOutcome::Aborted,
    }
}

/// Error out with the "confirmation required" error code. Used by callers who
/// got a `ConfirmOutcome::Refused` and want the canonical exit path.
pub fn exit_refused(message: &str, suggestion: &str) -> ! {
    output::error(message, &ErrorCode::ConfirmationRequired, Some(suggestion));
    std::process::exit(1);
}

/// Structured risk metadata that destructive commands include in their JSON
/// output before executing, so agents can reason about the action from the
/// contract (not the prompt text).
#[derive(Debug, Clone, Serialize)]
pub struct RiskMetadata {
    pub destructive: bool,
    pub data_loss: bool,
    pub tier: u8,
}

impl From<Tier> for RiskMetadata {
    fn from(tier: Tier) -> Self {
        Self {
            destructive: tier.is_destructive(),
            data_loss: tier.implies_data_loss(),
            tier: tier as u8,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tier_flags_match_doctrine() {
        assert!(!Tier::One.is_destructive());
        assert!(!Tier::One.implies_data_loss());

        assert!(Tier::Two.is_destructive());
        assert!(!Tier::Two.implies_data_loss());

        assert!(Tier::Three.is_destructive());
        assert!(Tier::Three.implies_data_loss());
    }

    #[test]
    fn risk_metadata_derives_from_tier() {
        let m: RiskMetadata = Tier::Three.into();
        assert!(m.destructive);
        assert!(m.data_loss);
        assert_eq!(m.tier, 3);

        let m: RiskMetadata = Tier::Two.into();
        assert!(m.destructive);
        assert!(!m.data_loss);
        assert_eq!(m.tier, 2);

        let m: RiskMetadata = Tier::One.into();
        assert!(!m.destructive);
        assert!(!m.data_loss);
        assert_eq!(m.tier, 1);
    }

    #[test]
    fn tier2_short_circuits_with_yes_flag() {
        match confirm_tier2("Remove domain", "example.com", true) {
            ConfirmOutcome::Proceed => {}
            _ => panic!("--yes must short-circuit to Proceed"),
        }
    }

    #[test]
    fn tier3_short_circuits_with_data_loss_flag() {
        match confirm_tier3("myapp", &[], true) {
            ConfirmOutcome::Proceed => {}
            _ => panic!("--yes-i-know-this-destroys-data must short-circuit to Proceed"),
        }
    }
}
