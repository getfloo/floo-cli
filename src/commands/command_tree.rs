use crate::constants::VERSION;
use crate::output;

use serde::Serialize;

#[derive(Serialize)]
struct CommandInfo {
    name: &'static str,
    description: &'static str,
    usage: &'static str,
    requires_auth: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    subcommands: Vec<CommandInfo>,
}

fn command_tree() -> Vec<CommandInfo> {
    vec![
        CommandInfo {
            name: "deploy",
            description: "Deploy a project to Floo",
            usage: "floo deploy [PATH] [OPTIONS]",
            requires_auth: true,
            subcommands: vec![
                CommandInfo {
                    name: "list",
                    description: "List deploy history for an app",
                    usage: "floo deploy list --app <name>",
                    requires_auth: true,
                    subcommands: vec![],
                },
                CommandInfo {
                    name: "logs",
                    description: "Show build logs for a specific deploy",
                    usage: "floo deploy logs <deploy-id> --app <name>",
                    requires_auth: true,
                    subcommands: vec![],
                },
                CommandInfo {
                    name: "watch",
                    description: "Stream deploy progress in real-time",
                    usage: "floo deploy watch --app <name>",
                    requires_auth: true,
                    subcommands: vec![],
                },
                CommandInfo {
                    name: "rollback",
                    description: "Rollback to a previous deploy",
                    usage: "floo deploy rollback <app> <deploy-id>",
                    requires_auth: true,
                    subcommands: vec![],
                },
            ],
        },
        CommandInfo {
            name: "init",
            description: "Initialize a new Floo project (creates config files)",
            usage: "floo init [NAME] [--path <dir>]",
            requires_auth: true,
            subcommands: vec![],
        },
        CommandInfo {
            name: "check",
            description: "Validate project config before deploying",
            usage: "floo check [PATH]",
            requires_auth: false,
            subcommands: vec![],
        },
        CommandInfo {
            name: "apps",
            description: "Manage your apps (list, status, delete)",
            usage: "floo apps <subcommand>",
            requires_auth: true,
            subcommands: vec![
                CommandInfo {
                    name: "list",
                    description: "List all your apps",
                    usage: "floo apps list",
                    requires_auth: true,
                    subcommands: vec![],
                },
                CommandInfo {
                    name: "status",
                    description: "Show details for an app",
                    usage: "floo apps status <name>",
                    requires_auth: true,
                    subcommands: vec![],
                },
                CommandInfo {
                    name: "delete",
                    description: "Delete an app",
                    usage: "floo apps delete <name> [--force]",
                    requires_auth: true,
                    subcommands: vec![],
                },
                CommandInfo {
                    name: "github",
                    description: "Manage GitHub integration",
                    usage: "floo apps github <subcommand>",
                    requires_auth: true,
                    subcommands: vec![
                        CommandInfo {
                            name: "connect",
                            description: "Connect a GitHub repo for auto-deploy",
                            usage: "floo apps github connect <owner/repo> --app <name>",
                            requires_auth: true,
                            subcommands: vec![],
                        },
                        CommandInfo {
                            name: "disconnect",
                            description: "Disconnect a GitHub repo",
                            usage: "floo apps github disconnect --app <name>",
                            requires_auth: true,
                            subcommands: vec![],
                        },
                        CommandInfo {
                            name: "status",
                            description: "Show GitHub connection status",
                            usage: "floo apps github status --app <name>",
                            requires_auth: true,
                            subcommands: vec![],
                        },
                    ],
                },
                CommandInfo {
                    name: "password",
                    description: "Show the shared password for a password-protected app",
                    usage: "floo apps password <name>",
                    requires_auth: true,
                    subcommands: vec![],
                },
            ],
        },
        CommandInfo {
            name: "env",
            description: "Manage environment variables",
            usage: "floo env <subcommand>",
            requires_auth: true,
            subcommands: vec![
                CommandInfo {
                    name: "set",
                    description: "Set an environment variable",
                    usage: "floo env set KEY=VALUE --app <name>",
                    requires_auth: true,
                    subcommands: vec![],
                },
                CommandInfo {
                    name: "get",
                    description: "Get an environment variable's value",
                    usage: "floo env get KEY --app <name>",
                    requires_auth: true,
                    subcommands: vec![],
                },
                CommandInfo {
                    name: "list",
                    description: "List environment variables",
                    usage: "floo env list --app <name>",
                    requires_auth: true,
                    subcommands: vec![],
                },
                CommandInfo {
                    name: "remove",
                    description: "Remove an environment variable",
                    usage: "floo env remove KEY --app <name>",
                    requires_auth: true,
                    subcommands: vec![],
                },
                CommandInfo {
                    name: "import",
                    description: "Import env vars from a .env file",
                    usage: "floo env import [FILE] --app <name>",
                    requires_auth: true,
                    subcommands: vec![],
                },
            ],
        },
        CommandInfo {
            name: "services",
            description: "Manage services for an app",
            usage: "floo services <subcommand>",
            requires_auth: true,
            subcommands: vec![
                CommandInfo {
                    name: "list",
                    description: "List all services for an app",
                    usage: "floo services list --app <name>",
                    requires_auth: true,
                    subcommands: vec![],
                },
                CommandInfo {
                    name: "info",
                    description: "Show details for a service",
                    usage: "floo services info <service> --app <name>",
                    requires_auth: true,
                    subcommands: vec![],
                },
                CommandInfo {
                    name: "add",
                    description: "Add a service to the project config",
                    usage: "floo services add <name> <path>",
                    requires_auth: false,
                    subcommands: vec![],
                },
                CommandInfo {
                    name: "rm",
                    description: "Remove a service from the project config",
                    usage: "floo services rm <name>",
                    requires_auth: false,
                    subcommands: vec![],
                },
            ],
        },
        CommandInfo {
            name: "domains",
            description: "Manage custom domains",
            usage: "floo domains <subcommand>",
            requires_auth: true,
            subcommands: vec![
                CommandInfo {
                    name: "add",
                    description: "Add a custom domain to an app",
                    usage: "floo domains add <hostname> --app <name>",
                    requires_auth: true,
                    subcommands: vec![],
                },
                CommandInfo {
                    name: "list",
                    description: "List custom domains for an app",
                    usage: "floo domains list --app <name>",
                    requires_auth: true,
                    subcommands: vec![],
                },
                CommandInfo {
                    name: "remove",
                    description: "Remove a custom domain",
                    usage: "floo domains remove <hostname> --app <name>",
                    requires_auth: true,
                    subcommands: vec![],
                },
            ],
        },
        CommandInfo {
            name: "logs",
            description: "View runtime logs for an app",
            usage: "floo logs --app <name> [OPTIONS]",
            requires_auth: true,
            subcommands: vec![],
        },
        CommandInfo {
            name: "analytics",
            description: "View traffic analytics for an app or org",
            usage: "floo analytics [APP] [--period 7d|30d|90d]",
            requires_auth: true,
            subcommands: vec![],
        },
        CommandInfo {
            name: "releases",
            description: "Manage releases and promote to prod",
            usage: "floo releases <subcommand>",
            requires_auth: true,
            subcommands: vec![
                CommandInfo {
                    name: "list",
                    description: "List releases for an app",
                    usage: "floo releases list --app <name>",
                    requires_auth: true,
                    subcommands: vec![],
                },
                CommandInfo {
                    name: "show",
                    description: "Show details for a release",
                    usage: "floo releases show <release-id> --app <name>",
                    requires_auth: true,
                    subcommands: vec![],
                },
                CommandInfo {
                    name: "promote",
                    description: "Promote an app to prod via GitHub release",
                    usage: "floo releases promote --app <name>",
                    requires_auth: true,
                    subcommands: vec![],
                },
            ],
        },
        CommandInfo {
            name: "auth",
            description: "Authenticate and manage your account",
            usage: "floo auth <subcommand>",
            requires_auth: false,
            subcommands: vec![
                CommandInfo {
                    name: "login",
                    description: "Authenticate with the Floo API",
                    usage: "floo auth login",
                    requires_auth: false,
                    subcommands: vec![],
                },
                CommandInfo {
                    name: "logout",
                    description: "Clear stored credentials",
                    usage: "floo auth logout",
                    requires_auth: false,
                    subcommands: vec![],
                },
                CommandInfo {
                    name: "whoami",
                    description: "Show the currently authenticated user",
                    usage: "floo auth whoami",
                    requires_auth: true,
                    subcommands: vec![],
                },
                CommandInfo {
                    name: "token",
                    description: "Print the current API key",
                    usage: "floo auth token",
                    requires_auth: true,
                    subcommands: vec![],
                },
                CommandInfo {
                    name: "register",
                    description: "Create a new Floo account",
                    usage: "floo auth register <email>",
                    requires_auth: false,
                    subcommands: vec![],
                },
            ],
        },
        CommandInfo {
            name: "orgs",
            description: "Manage your organization",
            usage: "floo orgs <subcommand>",
            requires_auth: true,
            subcommands: vec![CommandInfo {
                name: "members",
                description: "Manage org members",
                usage: "floo orgs members <subcommand>",
                requires_auth: true,
                subcommands: vec![
                    CommandInfo {
                        name: "list",
                        description: "List members of the current org",
                        usage: "floo orgs members list",
                        requires_auth: true,
                        subcommands: vec![],
                    },
                    CommandInfo {
                        name: "set-role",
                        description: "Change a member's role",
                        usage: "floo orgs members set-role <user-id> <role>",
                        requires_auth: true,
                        subcommands: vec![],
                    },
                ],
            }],
        },
        CommandInfo {
            name: "billing",
            description: "Manage billing and spend caps",
            usage: "floo billing <subcommand>",
            requires_auth: true,
            subcommands: vec![
                CommandInfo {
                    name: "spend-cap",
                    description: "Manage compute spend cap",
                    usage: "floo billing spend-cap <get|set>",
                    requires_auth: true,
                    subcommands: vec![],
                },
                CommandInfo {
                    name: "upgrade",
                    description: "Upgrade your plan",
                    usage: "floo billing upgrade [--plan growth|team]",
                    requires_auth: true,
                    subcommands: vec![],
                },
                CommandInfo {
                    name: "contact",
                    description: "Print enterprise contact information",
                    usage: "floo billing contact",
                    requires_auth: false,
                    subcommands: vec![],
                },
            ],
        },
        CommandInfo {
            name: "docs",
            description: "Built-in platform documentation",
            usage: "floo docs [TOPIC]",
            requires_auth: false,
            subcommands: vec![],
        },
        CommandInfo {
            name: "commands",
            description: "List all commands (structured for agents)",
            usage: "floo commands [--json]",
            requires_auth: false,
            subcommands: vec![],
        },
        CommandInfo {
            name: "skills",
            description: "Install agent skills for AI coding assistants",
            usage: "floo skills install [--path <dir>] [--print]",
            requires_auth: false,
            subcommands: vec![],
        },
        CommandInfo {
            name: "version",
            description: "Print installed CLI version",
            usage: "floo version",
            requires_auth: false,
            subcommands: vec![],
        },
        CommandInfo {
            name: "update",
            description: "Update the CLI binary in-place",
            usage: "floo update [--version <tag>]",
            requires_auth: false,
            subcommands: vec![],
        },
    ]
}

pub fn commands() {
    let tree = command_tree();

    if output::is_json_mode() {
        output::success(
            "Command tree",
            Some(serde_json::json!({
                "version": VERSION,
                "commands": tree,
            })),
        );
    } else {
        eprintln!("Commands:");
        for cmd in &tree {
            eprintln!("  {:<14}{}", cmd.name, cmd.description);
            for sub in &cmd.subcommands {
                eprintln!("    {:<12}{}", sub.name, sub.description);
                for sub2 in &sub.subcommands {
                    eprintln!("      {:<10}{}", sub2.name, sub2.description);
                }
            }
        }
        eprintln!();
        eprintln!("Run `floo <command> --help` for details.");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn test_command_tree_not_empty() {
        let tree = command_tree();
        assert!(!tree.is_empty());
    }

    #[test]
    fn test_all_commands_have_descriptions() {
        fn check(cmds: &[CommandInfo]) {
            for cmd in cmds {
                assert!(
                    !cmd.description.is_empty(),
                    "missing description for {}",
                    cmd.name
                );
                assert!(!cmd.usage.is_empty(), "missing usage for {}", cmd.name);
                check(&cmd.subcommands);
            }
        }
        check(&command_tree());
    }

    #[test]
    fn test_command_names_match_cli_enum() {
        // Must match top-level Commands enum variants in cli.rs.
        let expected: BTreeSet<&str> = [
            "analytics",
            "apps",
            "auth",
            "billing",
            "check",
            "commands",
            "deploy",
            "docs",
            "domains",
            "env",
            "init",
            "logs",
            "orgs",
            "releases",
            "services",
            "skills",
            "update",
            "version",
        ]
        .into_iter()
        .collect();

        let actual: BTreeSet<&str> = command_tree().iter().map(|c| c.name).collect();
        assert_eq!(
            expected, actual,
            "command_tree and Commands enum are out of sync"
        );
    }
}
