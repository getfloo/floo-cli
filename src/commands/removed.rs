//! Commands the deploy rework removed. Each one fails with the command that
//! replaced it, so people and agents that learned the old name correct
//! themselves instead of reading a generic "unrecognized subcommand".

use std::ffi::OsString;

use serde_json::json;

use crate::errors::ErrorCode;
use crate::output;

/// Old command path, the command that replaces it, and anything else to know.
const REMOVED: &[(&[&str], &str, &str)] = &[
    (&["projects", "create"], "floo deploy --hosted", ""),
    (&["projects", "list"], "floo apps list", ""),
    (&["projects", "clone"], "floo clone <app>", ""),
    (
        &["apps", "github", "setup"],
        "floo deploy",
        " It starts GitHub setup when floo needs it.",
    ),
    (
        &["apps", "github", "connect"],
        "floo connect owner/repo",
        "",
    ),
    (
        &["apps", "github", "disconnect"],
        "floo connect owner/repo",
        " Every app keeps one source; this changes it.",
    ),
    (&["apps", "github", "status"], "floo status", ""),
    (
        &["previews", "up"],
        "floo deploy",
        " Run it on the branch: any branch but the default one gets a preview URL.",
    ),
    (&["deploys", "status"], "floo status", ""),
    (&["deploys", "rollback"], "floo rollback", ""),
    (&["doctor", "push"], "floo status", ""),
    (&["releases", "promote"], "floo promote", ""),
];

/// The leading command words of argv: global flags (none of which take a
/// value) are skipped, and parsing stops at the first flag after a word.
fn command_words(args: &[OsString]) -> Vec<&str> {
    let mut words = Vec::new();
    for arg in args.iter().skip(1) {
        let Some(arg) = arg.to_str() else { break };
        if arg == "--" {
            break;
        }
        if arg.starts_with('-') {
            if words.is_empty() {
                continue;
            }
            break;
        }
        words.push(arg);
    }
    words
}

/// The removed command at the start of argv, and its replacement.
pub(crate) fn replacement(args: &[OsString]) -> Option<(String, &'static str, &'static str)> {
    let words = command_words(args);
    REMOVED
        .iter()
        .find(|(path, _, _)| words.len() >= path.len() && words[..path.len()] == **path)
        .map(|(path, instead, note)| (format!("floo {}", path.join(" ")), *instead, *note))
}

/// Exit with the replacement when argv names a removed command.
pub(crate) fn exit_if_removed(args: &[OsString]) {
    let Some((old, instead, note)) = replacement(args) else {
        return;
    };
    output::error_with_details(
        &format!("`{old}` was removed. Use `{instead}`.{note}"),
        &ErrorCode::Other("COMMAND_REMOVED".to_string()),
        Some(instead),
        Some(&json!({ "removed": old, "replacement": instead })),
    );
    std::process::exit(2);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(words: &[&str]) -> Vec<OsString> {
        std::iter::once("floo")
            .chain(words.iter().copied())
            .map(OsString::from)
            .collect()
    }

    #[test]
    fn a_removed_command_names_its_replacement() {
        let found = replacement(&argv(&["--json", "apps", "github", "connect", "acme/shop"]));
        assert_eq!(
            found.map(|(old, instead, _)| (old, instead)),
            Some((
                "floo apps github connect".to_string(),
                "floo connect owner/repo"
            ))
        );
        assert_eq!(
            replacement(&argv(&["releases", "promote", "--app", "shop"])).map(|(_, i, _)| i),
            Some("floo promote")
        );
    }

    #[test]
    fn commands_that_stay_are_not_matched() {
        assert_eq!(replacement(&argv(&["apps", "list"])), None);
        assert_eq!(
            replacement(&argv(&["deploys", "list", "--app", "status"])),
            None
        );
        assert_eq!(replacement(&argv(&["logs", "--app", "status"])), None);
        assert_eq!(replacement(&argv(&["preflight", "./app"])), None);
    }

    #[test]
    fn every_replacement_names_a_command_that_exists() {
        use clap::CommandFactory;
        let root = crate::cli::Cli::command();
        for (path, instead, _) in REMOVED {
            let mut command = &root;
            let words = instead
                .split_whitespace()
                .skip(1)
                .take_while(|w| !w.starts_with('-') && !w.starts_with('<') && !w.contains('/'));
            for word in words {
                command = command
                    .find_subcommand(word)
                    .unwrap_or_else(|| panic!("{instead} (for {path:?}) is not a command"));
            }
            assert!(
                root.clone()
                    .try_get_matches_from(std::iter::once("floo").chain(path.iter().copied()))
                    .is_err(),
                "{path:?} still parses, so its removal message never shows"
            );
        }
    }
}
