//! `floo commands` — the agent-facing command index.
//!
//! The index is a pure projection of the real clap command tree
//! (`Cli::command()`): names, descriptions, structure, and usage are all read
//! from the live `clap::Command`, so the index cannot drift from the CLI it
//! documents. The only curated metadata is which commands skip authentication
//! (`AUTH_EXEMPT_PATHS`), and that list is pinned to real commands by a test.

use crate::cli::Cli;
use crate::constants::VERSION;
use crate::output;

use clap::CommandFactory;
use serde::Serialize;

/// Command paths (relative to `floo`) that do NOT require an API key.
///
/// Every command not listed here requires authentication. This is the only
/// hand-maintained metadata in the index — everything else is projected from
/// clap. `auth_exempt_paths_all_exist` fails if any entry stops matching a
/// real command, so a rename or removal can't leave a stale entry behind.
const AUTH_EXEMPT_PATHS: &[&[&str]] = &[
    &["preflight"],
    &["auth"],
    &["auth", "login"],
    &["auth", "logout"],
    &["auth", "register"],
    &["billing", "contact"],
    &["docs"],
    &["commands"],
    &["skills"],
    &["skills", "install"],
    &["version"],
    &["update"],
];

fn requires_auth(path: &[&str]) -> bool {
    !AUTH_EXEMPT_PATHS.contains(&path)
}

#[derive(Serialize)]
struct CommandInfo {
    name: String,
    description: String,
    usage: String,
    requires_auth: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    subcommands: Vec<CommandInfo>,
}

/// The synthetic `help` subcommand clap injects is not a floo command — skip it
/// (along with any explicitly hidden command) so the index matches `floo --help`.
fn is_listable(cmd: &clap::Command) -> bool {
    !cmd.is_hide_set() && cmd.get_name() != "help"
}

/// Build the index by walking the live clap command tree.
fn build_tree() -> Vec<CommandInfo> {
    Cli::command()
        .get_subcommands()
        .filter(|c| is_listable(c))
        .map(|c| command_info(c, &[c.get_name()]))
        .collect()
}

/// Project one `clap::Command` (and its subtree) into a `CommandInfo`.
/// `path` is the full command path below `floo`, ending in this command's name.
fn command_info(cmd: &clap::Command, path: &[&str]) -> CommandInfo {
    let subcommands: Vec<CommandInfo> = cmd
        .get_subcommands()
        .filter(|c| is_listable(c))
        .map(|c| {
            let mut child_path = path.to_vec();
            child_path.push(c.get_name());
            command_info(c, &child_path)
        })
        .collect();

    CommandInfo {
        name: cmd.get_name().to_string(),
        description: cmd.get_about().map(|s| s.to_string()).unwrap_or_default(),
        usage: render_usage(path, cmd, !subcommands.is_empty()),
        requires_auth: requires_auth(path),
        subcommands,
    }
}

/// Render an authoritative usage line: `floo <path> <positionals> [<subcommand>]`.
/// Positionals come from clap (required → `<NAME>`, optional → `[NAME]`); detailed
/// flags live behind `floo <command> --help`, which the human footer points to.
fn render_usage(path: &[&str], cmd: &clap::Command, has_subcommands: bool) -> String {
    let mut parts: Vec<String> = Vec::with_capacity(path.len() + 3);
    parts.push("floo".to_string());
    parts.extend(path.iter().map(|s| s.to_string()));

    for arg in cmd.get_positionals() {
        let name = arg
            .get_value_names()
            .and_then(|names| names.first())
            .map(|s| s.to_string())
            .unwrap_or_else(|| arg.get_id().as_str().to_uppercase());
        if arg.is_required_set() {
            parts.push(format!("<{name}>"));
        } else {
            parts.push(format!("[{name}]"));
        }
    }

    if has_subcommands {
        parts.push("<subcommand>".to_string());
    }

    parts.join(" ")
}

/// Render the human-readable index. Each sibling group aligns its descriptions
/// to its own longest name plus two spaces, so a name can never abut its
/// description regardless of length or nesting depth.
fn render_human(tree: &[CommandInfo]) -> String {
    let mut out = String::from("Commands:\n");
    render_level(tree, 0, &mut out);
    out.push('\n');
    out.push_str("Run `floo <command> --help` for details.\n");
    out
}

fn render_level(cmds: &[CommandInfo], depth: usize, out: &mut String) {
    let indent = 2 + depth * 2;
    let column = cmds.iter().map(|c| c.name.len()).max().unwrap_or(0) + 2;
    for cmd in cmds {
        let gap = column - cmd.name.len(); // column >= name.len() + 2, so gap >= 2
        out.push_str(&" ".repeat(indent));
        out.push_str(&cmd.name);
        out.push_str(&" ".repeat(gap));
        out.push_str(&cmd.description);
        out.push('\n');
        if !cmd.subcommands.is_empty() {
            render_level(&cmd.subcommands, depth + 1, out);
        }
    }
}

pub fn commands() {
    let tree = build_tree();

    if output::is_json_mode() {
        output::success(
            "Command tree",
            Some(serde_json::json!({
                "version": VERSION,
                "commands": tree,
            })),
        );
    } else {
        eprint!("{}", render_human(&tree));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    /// Collect `"a b c"` command paths from the generated index.
    fn tree_paths(cmds: &[CommandInfo], prefix: &str, out: &mut BTreeSet<String>) {
        for cmd in cmds {
            let path = if prefix.is_empty() {
                cmd.name.clone()
            } else {
                format!("{prefix} {}", cmd.name)
            };
            out.insert(path.clone());
            tree_paths(&cmd.subcommands, &path, out);
        }
    }

    /// Collect command paths directly from the real clap tree — an independent
    /// walk, so comparing it to `tree_paths` is a genuine parity check.
    fn clap_paths(cmd: &clap::Command, prefix: &str, out: &mut BTreeSet<String>) {
        for sub in cmd.get_subcommands() {
            if !is_listable(sub) {
                continue;
            }
            let path = if prefix.is_empty() {
                sub.get_name().to_string()
            } else {
                format!("{prefix} {}", sub.get_name())
            };
            out.insert(path.clone());
            clap_paths(sub, &path, out);
        }
    }

    #[test]
    fn index_matches_real_command_tree() {
        let mut from_tree = BTreeSet::new();
        tree_paths(&build_tree(), "", &mut from_tree);
        let mut from_clap = BTreeSet::new();
        clap_paths(&Cli::command(), "", &mut from_clap);
        assert_eq!(
            from_tree, from_clap,
            "`floo commands` index has drifted from the real clap command tree"
        );
    }

    #[test]
    fn pins_known_drift_regressions() {
        let mut paths = BTreeSet::new();
        tree_paths(&build_tree(), "", &mut paths);
        // Real commands the hand-maintained index used to omit (#1160).
        for real in [
            "storage",
            "doctor",
            "services add",
            "services remove",
            "db connections",
            "releases rollback",
            "deploys status",
            "previews up",
            "previews delete",
        ] {
            assert!(
                paths.contains(real),
                "index is missing real command `{real}`"
            );
        }
        // The index used to invent `apps status`; the real command is `apps show`.
        assert!(paths.contains("apps show"), "index is missing `apps show`");
        assert!(
            !paths.contains("apps status"),
            "index lists phantom command `apps status`"
        );
    }

    #[test]
    fn every_command_has_description_and_usage() {
        fn check(cmds: &[CommandInfo]) {
            for cmd in cmds {
                assert!(
                    !cmd.description.is_empty(),
                    "missing description for `{}` (add a /// doc comment in cli.rs)",
                    cmd.name
                );
                assert!(!cmd.usage.is_empty(), "missing usage for `{}`", cmd.name);
                check(&cmd.subcommands);
            }
        }
        check(&build_tree());
    }

    #[test]
    fn usage_starts_with_full_command_path() {
        let tree = build_tree();
        let apps = tree
            .iter()
            .find(|c| c.name == "apps")
            .expect("apps command");
        let show = apps
            .subcommands
            .iter()
            .find(|c| c.name == "show")
            .expect("apps show");
        assert!(
            show.usage.starts_with("floo apps show"),
            "unexpected usage: {}",
            show.usage
        );
        assert!(
            apps.usage.ends_with("<subcommand>"),
            "group usage should flag a required subcommand: {}",
            apps.usage
        );
    }

    #[test]
    fn auth_exempt_paths_all_exist() {
        let mut clap = BTreeSet::new();
        clap_paths(&Cli::command(), "", &mut clap);
        for path in AUTH_EXEMPT_PATHS {
            let joined = path.join(" ");
            assert!(
                clap.contains(&joined),
                "AUTH_EXEMPT_PATHS lists `{joined}`, which is not a real command"
            );
        }
    }

    #[test]
    fn requires_auth_classification() {
        // Pre-auth commands.
        assert!(!requires_auth(&["auth", "login"]));
        assert!(!requires_auth(&["docs"]));
        assert!(!requires_auth(&["preflight"]));
        assert!(!requires_auth(&["billing", "contact"]));
        // Authenticated commands, including auth subcommands that need a session.
        assert!(requires_auth(&["auth", "whoami"]));
        assert!(requires_auth(&["apps", "show"]));
        assert!(requires_auth(&["storage"]));
        assert!(requires_auth(&["billing"]));
    }

    #[test]
    fn human_output_never_abuts_name_and_description() {
        // Exercise render_level directly with a name far longer than any column
        // the old fixed-width formatter reserved — the bug it must not regress.
        let cmds = vec![
            CommandInfo {
                name: "x".to_string(),
                description: "short".to_string(),
                usage: String::new(),
                requires_auth: true,
                subcommands: vec![],
            },
            CommandInfo {
                name: "a-very-long-command-name".to_string(),
                description: "long".to_string(),
                usage: String::new(),
                requires_auth: true,
                subcommands: vec![],
            },
        ];
        let mut out = String::new();
        render_level(&cmds, 0, &mut out);
        for line in out.lines() {
            let (name, desc) = line.trim_start().split_once("  ").unwrap_or_else(|| {
                panic!("row has no two-space gap between name and description: {line:?}")
            });
            assert!(
                !name.is_empty() && !desc.trim().is_empty(),
                "malformed row: {line:?}"
            );
        }
    }

    #[test]
    fn human_output_has_header_and_footer() {
        let rendered = render_human(&build_tree());
        assert!(rendered.starts_with("Commands:\n"));
        assert!(rendered.contains("Run `floo <command> --help` for details."));
    }
}
