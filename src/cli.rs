use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

use crate::commands;
use crate::constants::VERSION;
use crate::output;

#[derive(Parser)]
#[command(
    name = "floo",
    about = "Deploy, manage, and observe web apps.",
    version = VERSION,
)]
pub struct Cli {
    /// Output JSON to stdout (for agents).
    #[arg(long, global = true)]
    pub json: bool,

    /// Preview what a command would do without executing it.
    #[arg(long, global = true)]
    pub dry_run: bool,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// View traffic analytics for an app or org.
    Analytics {
        /// App name or ID. Omit for org-level overview.
        app: Option<String>,

        /// Time period: 7d, 30d, or 90d.
        #[arg(short, long, default_value = "30d", value_parser = ["7d", "30d", "90d"])]
        period: String,
    },

    /// Initialize a new Floo project (creates config files).
    #[command(after_help = "\
Examples:
  floo init my-app                         Scaffold config in current directory
  floo init my-app --path ./project        Scaffold config in ./project")]
    Init {
        /// App name (required in non-interactive/JSON mode).
        name: Option<String>,

        /// Project directory.
        #[arg(short, long, default_value = ".")]
        path: PathBuf,
    },

    /// Deploy a project to Floo, or manage deploy history.
    #[command(after_help = "\
Examples:
  floo deploy                              Deploy current directory
  floo deploy ./app --json                 Deploy ./app with JSON output
  floo deploy --app my-app --restart       Restart without rebuilding
  floo deploy --dry-run --json             Preview what would be deployed
  floo deploy list --app my-app            Show deploy history
  floo deploy rollback my-app abc123       Rollback to a previous deploy")]
    Deploy(DeployArgs),

    /// Authenticate and manage your account.
    #[command(subcommand)]
    Auth(AuthCommands),

    /// Manage your organization.
    #[command(subcommand)]
    Orgs(OrgsCommands),

    /// Manage billing and spend caps.
    #[command(subcommand)]
    Billing(BillingCommands),

    /// Manage your apps.
    #[command(
        subcommand,
        after_help = "\
Examples:
  floo apps list --json                    List all apps
  floo apps status my-app --json           App details and service info
  floo apps delete my-app --force          Delete without confirmation"
    )]
    Apps(AppsCommands),

    /// Manage environment variables.
    #[command(
        subcommand,
        after_help = "\
Examples:
  floo env set API_KEY=secret --app my-app           Set an env var
  floo env set API_KEY=secret --app my-app --restart  Set and restart
  floo env list --app my-app --json                   List all env vars
  floo env import .env --app my-app                   Import from .env file"
    )]
    Env(EnvCommands),

    /// Manage services for an app.
    #[command(subcommand)]
    Services(ServicesCommands),

    /// Manage custom domains.
    #[command(subcommand)]
    Domains(DomainsCommands),

    /// Manage releases and promote to prod.
    #[command(subcommand)]
    Releases(ReleasesCommands),

    /// View runtime logs for an app.
    #[command(after_help = "\
Examples:
  floo logs --app my-app                   Last 100 log lines
  floo logs --app my-app --since 1h --error  Errors in the last hour
  floo logs --app my-app --live            Stream logs in real-time
  floo logs --app my-app --json            Structured JSON output")]
    Logs {
        /// App name or ID (overrides config file).
        #[arg(short, long)]
        app: Option<String>,

        /// Number of log lines to show.
        #[arg(short, long, default_value = "100")]
        tail: u32,

        /// Show logs since a time (e.g., 1h, 30m, 2d, or ISO timestamp).
        #[arg(short, long)]
        since: Option<String>,

        /// Filter to errors only (shorthand for --severity ERROR).
        #[arg(short, long)]
        error: bool,

        /// Minimum severity level (DEBUG, INFO, WARNING, ERROR, CRITICAL).
        #[arg(long)]
        severity: Option<String>,

        /// Filter logs to specific services (repeatable).
        #[arg(long)]
        services: Vec<String>,

        /// Filter log messages by text (case-insensitive).
        #[arg(long)]
        search: Option<String>,

        /// Stream logs in real-time (poll every 2s).
        #[arg(short = 'f', long, conflicts_with = "output")]
        live: bool,

        /// Write logs to a file (JSON or plain text based on --json flag).
        #[arg(short, long)]
        output: Option<PathBuf>,
    },

    /// Install agent skills for AI coding assistants.
    #[command(subcommand)]
    Skills(SkillsCommands),

    /// Built-in platform documentation.
    Docs {
        /// Topic: services, config, deploy. Omit for overview.
        topic: Option<String>,
    },

    /// List all commands (structured for agents in --json mode).
    #[command(name = "commands")]
    Discover,

    /// Print installed CLI version.
    Version,

    /// Update the CLI binary in-place.
    Update {
        /// Specific release tag to install (e.g. v0.2.0).
        #[arg(long)]
        version: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum AuthCommands {
    /// Authenticate with the Floo API (opens browser).
    Login {
        /// Use an existing API key instead of browser auth.
        #[arg(long)]
        api_key: Option<String>,

        /// Skip existing key validation and force re-authentication.
        #[arg(long)]
        force: bool,
    },
    /// Clear stored credentials.
    Logout,
    /// Show the currently authenticated user.
    Whoami,
    /// Print the current API key to stdout.
    Token,
    /// Create a new Floo account.
    Register {
        /// Account email address.
        email: String,
    },
    /// Update your display name.
    UpdateProfile {
        /// New display name.
        #[arg(long)]
        name: String,
    },
}

#[derive(Subcommand)]
pub enum BillingCommands {
    /// Manage your org's compute spend cap.
    #[command(subcommand)]
    SpendCap(SpendCapCommands),

    /// Upgrade your plan via Stripe Checkout or Billing Portal.
    Upgrade {
        /// Plan to upgrade to: hobby, pro, or team. Omit to open billing portal.
        #[arg(long, value_parser = ["hobby", "pro", "team"])]
        plan: Option<String>,
    },

    /// Show current usage, spend cap, and plan details.
    Usage,

    /// Print enterprise contact information.
    Contact,
}

#[derive(Subcommand)]
pub enum SpendCapCommands {
    /// Show current spend cap and usage.
    Get,
    /// Set the monthly spend cap (in dollars). 0 = no cap.
    Set {
        /// Spend cap amount in dollars (e.g., 20.00). Use 0 for no cap.
        amount: f64,
    },
}

#[derive(Subcommand)]
pub enum OrgsCommands {
    /// Manage org members.
    #[command(subcommand)]
    Members(MembersCommands),
}

#[derive(Subcommand)]
pub enum MembersCommands {
    /// List members of the current org.
    List,
    /// Change a member's role.
    SetRole {
        /// User ID of the member.
        user_id: String,
        /// New role (admin, member, or viewer).
        role: String,
    },
}

#[derive(Subcommand)]
pub enum AppsCommands {
    /// List all your apps.
    List {
        /// Page number.
        #[arg(long, default_value = "1")]
        page: u32,

        /// Results per page.
        #[arg(long, default_value = "50")]
        per_page: u32,
    },

    /// Show details for an app.
    Status {
        /// App name or ID.
        app_name: String,
    },

    /// Delete an app.
    Delete {
        /// App name or ID.
        app_name: String,

        /// Skip confirmation.
        #[arg(short, long)]
        force: bool,
    },

    /// Manage GitHub integration.
    #[command(subcommand)]
    Github(GitHubCommands),

    /// Show the shared password for a password-protected app.
    Password {
        /// App name or ID.
        app_name: String,
    },
}

#[derive(Subcommand)]
pub enum GitHubCommands {
    /// Connect a GitHub repo to an app for auto-deploy.
    Connect {
        /// GitHub repo (owner/repo).
        repo: String,

        /// App name or ID (uses config file if omitted).
        #[arg(short, long)]
        app: Option<String>,

        /// Default branch (defaults to "main").
        #[arg(short, long)]
        branch: Option<String>,

        /// Skip env var check for webhook deploys.
        #[arg(long)]
        skip_env_check: bool,

        /// Skip triggering a deploy after connecting.
        #[arg(long)]
        no_deploy: bool,
    },

    /// Disconnect a GitHub repo from an app.
    Disconnect {
        /// App name or ID (uses config file if omitted).
        #[arg(short, long)]
        app: Option<String>,
    },

    /// Show GitHub connection status for an app.
    Status {
        /// App name or ID (uses config file if omitted).
        #[arg(short, long)]
        app: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum EnvCommands {
    /// Set an environment variable on an app.
    Set {
        /// KEY=VALUE pair to set.
        key_value: String,

        /// App name or ID (reads from config if omitted).
        #[arg(short, long)]
        app: Option<String>,

        /// Target specific services (repeatable).
        #[arg(long)]
        services: Vec<String>,

        /// Restart the app after setting the env var (redeploy with fresh env vars).
        #[arg(long)]
        restart: bool,
    },

    /// List environment variables for an app.
    List {
        /// App name or ID (reads from config if omitted).
        #[arg(short, long)]
        app: Option<String>,

        /// Target specific services (repeatable).
        #[arg(long)]
        services: Vec<String>,
    },

    /// Remove an environment variable from an app.
    Remove {
        /// Environment variable key to remove.
        key: String,

        /// App name or ID (reads from config if omitted).
        #[arg(short, long)]
        app: Option<String>,

        /// Target specific services (repeatable).
        #[arg(long)]
        services: Vec<String>,
    },

    /// Get an environment variable's plaintext value.
    Get {
        /// Environment variable key.
        key: String,

        /// App name or ID (reads from config if omitted).
        #[arg(short, long)]
        app: Option<String>,

        /// Target a specific service.
        #[arg(long)]
        service: Option<String>,
    },

    /// Import environment variables from a .env file.
    Import {
        /// Path to .env file (defaults to env_file from config or .env).
        #[arg(conflicts_with = "all")]
        file: Option<PathBuf>,

        /// App name or ID (reads from config if omitted).
        #[arg(short, long)]
        app: Option<String>,

        /// Target specific services (repeatable).
        #[arg(long, conflicts_with = "all")]
        services: Vec<String>,

        /// Import env vars for all services using their configured env_file paths.
        #[arg(long)]
        all: bool,
    },
}

#[derive(Subcommand)]
pub enum ServicesCommands {
    /// List all services for an app.
    List {
        /// App name or ID (uses config file if omitted).
        #[arg(short, long)]
        app: Option<String>,
    },

    /// Show details for a service (managed or user-managed).
    Info {
        /// Service name.
        service_name: String,

        /// App name or ID (uses config file if omitted).
        #[arg(short, long)]
        app: Option<String>,
    },

    /// Add a service to the project config.
    Add {
        /// Service name (DNS-safe: lowercase, digits, hyphens).
        name: String,

        /// Relative path to the service directory.
        path: String,

        /// Port the service listens on.
        #[arg(long)]
        port: Option<u16>,

        /// Service type: web, api, or worker.
        #[arg(long = "type")]
        service_type: Option<String>,

        /// Ingress mode: public or internal.
        #[arg(long)]
        ingress: Option<String>,

        /// Path to .env file relative to service directory.
        #[arg(long)]
        env_file: Option<String>,
    },

    /// Remove a service from the project config.
    Rm {
        /// Service name to remove.
        name: String,

        /// Also delete the service's floo.service.toml file.
        #[arg(long)]
        delete_config: bool,
    },
}

#[derive(Subcommand)]
pub enum DomainsCommands {
    /// Add a custom domain to an app.
    Add {
        /// Domain hostname (e.g. app.example.com).
        hostname: String,

        /// App name or ID (uses config file if omitted).
        #[arg(short, long)]
        app: Option<String>,

        /// Target service name (required for multi-service apps).
        #[arg(long)]
        services: Option<String>,
    },

    /// List custom domains for an app.
    List {
        /// App name or ID (uses config file if omitted).
        #[arg(short, long)]
        app: Option<String>,

        /// Target service name (required for multi-service apps).
        #[arg(long)]
        services: Option<String>,
    },

    /// Remove a custom domain from an app.
    Remove {
        /// Domain hostname to remove.
        hostname: String,

        /// App name or ID (uses config file if omitted).
        #[arg(short, long)]
        app: Option<String>,

        /// Target service name (required for multi-service apps).
        #[arg(long)]
        services: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum ReleasesCommands {
    /// List releases for an app.
    List {
        /// App name or ID (uses config file if omitted).
        #[arg(short, long)]
        app: Option<String>,
    },

    /// Show details for a release.
    Show {
        /// Release ID.
        release_id: String,

        /// App name or ID (uses config file if omitted).
        #[arg(short, long)]
        app: Option<String>,
    },

    /// Promote an app to prod by creating a GitHub release.
    Promote {
        /// App name or ID (uses config file if omitted).
        #[arg(short, long)]
        app: Option<String>,

        /// Release tag (auto-generated if omitted).
        #[arg(short, long)]
        tag: Option<String>,
    },
}

#[derive(Args)]
#[command(args_conflicts_with_subcommands = true)]
pub struct DeployArgs {
    #[command(subcommand)]
    pub sub: Option<DeploySubcommands>,

    #[command(flatten)]
    pub run: DeployRunArgs,
}

#[derive(Subcommand)]
pub enum DeploySubcommands {
    /// List deploy history for an app.
    List {
        /// App name or ID (uses config file if omitted).
        #[arg(short, long)]
        app: Option<String>,
    },

    /// Show build logs for a specific deploy.
    Logs {
        /// Deploy ID.
        deploy_id: String,

        /// App name or ID (uses config file if omitted).
        #[arg(short, long)]
        app: Option<String>,
    },

    /// Stream deploy progress in real-time.
    Watch {
        /// App name or ID (uses config file if omitted).
        #[arg(short, long)]
        app: Option<String>,

        /// Match a deploy by commit SHA prefix (waits up to 120s for it to appear).
        #[arg(short, long)]
        commit: Option<String>,
    },

    /// Rollback to a previous deploy.
    Rollback {
        /// App name or ID.
        app: String,

        /// Deploy ID to rollback to.
        deploy_id: String,

        /// Skip confirmation prompt.
        #[arg(short, long)]
        force: bool,
    },
}

#[derive(Args)]
pub struct DeployRunArgs {
    /// Project directory to deploy.
    #[arg(default_value = ".")]
    pub path: PathBuf,

    /// Existing app ID or name to deploy to.
    #[arg(short, long)]
    pub app: Option<String>,

    /// Deploy only these services (repeatable: --services api --services web).
    #[arg(short, long = "services")]
    pub services: Vec<String>,

    /// Restart the app without rebuilding (redeploy existing images with fresh env vars).
    #[arg(long)]
    pub restart: bool,

    /// Re-sync env vars from configured env_file before deploying.
    #[arg(long)]
    pub sync_env: bool,
}

#[derive(Subcommand)]
pub enum SkillsCommands {
    /// Install the floo agent skill file to a directory.
    Install {
        /// Directory to write SKILL.md into (e.g. .claude/skills/floo/).
        #[arg(long)]
        path: Option<PathBuf>,

        /// Print skill content to stdout instead of writing a file.
        #[arg(long)]
        print: bool,
    },
}

fn should_check_version(cli: &Cli) -> bool {
    if cli.json {
        return false;
    }
    if std::env::var("FLOO_NO_UPDATE_CHECK").is_ok() {
        return false;
    }
    !matches!(
        cli.command,
        Commands::Update { .. } | Commands::Version | Commands::Docs { .. } | Commands::Discover
    )
}

/// Reject `--dry-run` for mutating commands that don't implement it yet.
/// Read-only commands silently ignore the flag; unsupported mutators error early.
fn reject_unsupported_dry_run(command: &Commands) {
    let unsupported = matches!(
        command,
        Commands::Init { .. }
            | Commands::Apps(AppsCommands::Github(
                GitHubCommands::Connect { .. } | GitHubCommands::Disconnect { .. },
            ))
            | Commands::Releases(ReleasesCommands::Promote { .. })
            | Commands::Services(ServicesCommands::Add { .. } | ServicesCommands::Rm { .. })
            | Commands::Orgs(OrgsCommands::Members(MembersCommands::SetRole { .. }))
            | Commands::Billing(BillingCommands::SpendCap(SpendCapCommands::Set { .. }))
    );
    if unsupported {
        output::error(
            "--dry-run is not supported for this command.",
            &crate::errors::ErrorCode::InvalidFormat,
            Some("Supported commands: deploy, env set/remove/import, apps delete, domains add/remove, deploy rollback."),
        );
        std::process::exit(1);
    }
}

pub fn run() {
    let cli = Cli::parse();

    if cli.json {
        output::set_json_mode(true);
    }
    if cli.dry_run {
        output::set_dry_run_mode(true);
        reject_unsupported_dry_run(&cli.command);
    }

    let do_version_check = should_check_version(&cli);

    // Phase 2: Auto-apply any staged update (before command dispatch)
    if do_version_check {
        crate::version_check::apply_staged_update(VERSION);
    }

    // Phase 1: Spawn background check + download (non-blocking)
    let version_handle = if do_version_check {
        crate::version_check::spawn_check(VERSION)
    } else {
        None
    };

    match cli.command {
        Commands::Analytics { app, period } => commands::analytics::analytics(app, &period),

        Commands::Init { name, path } => commands::init::init(name, path),

        Commands::Deploy(args) => {
            if let Some(sub) = args.sub {
                match sub {
                    DeploySubcommands::List { app } => commands::deploys::list(app.as_deref()),
                    DeploySubcommands::Logs { deploy_id, app } => {
                        commands::deploys::logs(&deploy_id, app.as_deref())
                    }
                    DeploySubcommands::Watch { app, commit } => {
                        commands::deploys::watch(app.as_deref(), commit.as_deref())
                    }
                    DeploySubcommands::Rollback {
                        app,
                        deploy_id,
                        force,
                    } => commands::rollbacks::rollback(&app, &deploy_id, force),
                }
            } else {
                let r = args.run;
                commands::deploy::deploy(r.path, r.app, r.services, r.restart, r.sync_env)
            }
        }
        Commands::Auth(sub) => match sub {
            AuthCommands::Login { api_key, force } => {
                commands::auth::login(api_key.as_deref(), force)
            }
            AuthCommands::Logout => commands::auth::logout(),
            AuthCommands::Whoami => commands::auth::whoami(),
            AuthCommands::Token => commands::auth::token(),
            AuthCommands::Register { email } => commands::auth::register(&email),
            AuthCommands::UpdateProfile { name } => commands::auth::update_profile(&name),
        },

        Commands::Billing(sub) => match sub {
            BillingCommands::SpendCap(cap_sub) => match cap_sub {
                SpendCapCommands::Get => commands::billing::spend_cap_get(),
                SpendCapCommands::Set { amount } => commands::billing::spend_cap_set(amount),
            },
            BillingCommands::Upgrade { plan } => commands::billing::upgrade(plan),
            BillingCommands::Usage => commands::billing::usage(),
            BillingCommands::Contact => commands::billing::contact(),
        },

        Commands::Orgs(sub) => match sub {
            OrgsCommands::Members(members_sub) => match members_sub {
                MembersCommands::List => commands::orgs::list_members(),
                MembersCommands::SetRole { user_id, role } => {
                    commands::orgs::set_role(&user_id, &role)
                }
            },
        },

        Commands::Apps(sub) => match sub {
            AppsCommands::List { page, per_page } => commands::apps::list(page, per_page),
            AppsCommands::Status { app_name } => commands::apps::status(&app_name),
            AppsCommands::Delete { app_name, force } => commands::apps::delete(&app_name, force),
            AppsCommands::Github(gh_sub) => match gh_sub {
                GitHubCommands::Connect {
                    repo,
                    app,
                    branch,
                    skip_env_check,
                    no_deploy,
                } => commands::github::connect(
                    &repo,
                    app.as_deref(),
                    branch.as_deref(),
                    skip_env_check,
                    no_deploy,
                ),
                GitHubCommands::Disconnect { app } => commands::github::disconnect(app.as_deref()),
                GitHubCommands::Status { app } => commands::github::status(app.as_deref()),
            },
            AppsCommands::Password { app_name } => commands::apps::show_password(&app_name),
        },

        Commands::Env(sub) => match sub {
            EnvCommands::Set {
                key_value,
                app,
                services,
                restart,
            } => commands::env::set(&key_value, app.as_deref(), &services, restart),
            EnvCommands::List { app, services } => commands::env::list(app.as_deref(), &services),
            EnvCommands::Remove { key, app, services } => {
                commands::env::remove(&key, app.as_deref(), &services)
            }
            EnvCommands::Get { key, app, service } => {
                commands::env::get(&key, app.as_deref(), service.as_deref())
            }
            EnvCommands::Import {
                file,
                app,
                services,
                all,
            } => {
                if all {
                    commands::env::import_all_services(app.as_deref());
                } else {
                    commands::env::import_vars(file.as_deref(), app.as_deref(), &services);
                }
            }
        },

        Commands::Services(sub) => match sub {
            ServicesCommands::List { app } => commands::services::list(app.as_deref()),
            ServicesCommands::Info { service_name, app } => {
                commands::services::info(&service_name, app.as_deref())
            }
            ServicesCommands::Add {
                name,
                path,
                port,
                service_type,
                ingress,
                env_file,
            } => commands::service_mgmt::add(
                &name,
                &path,
                port,
                service_type.as_deref(),
                ingress.as_deref(),
                env_file.as_deref(),
            ),
            ServicesCommands::Rm {
                name,
                delete_config,
            } => commands::service_mgmt::rm(&name, delete_config),
        },

        Commands::Domains(sub) => match sub {
            DomainsCommands::Add {
                hostname,
                app,
                services,
            } => commands::domains::add(&hostname, app.as_deref(), services.as_deref()),
            DomainsCommands::List { app, services } => {
                commands::domains::list(app.as_deref(), services.as_deref())
            }
            DomainsCommands::Remove {
                hostname,
                app,
                services,
            } => commands::domains::remove(&hostname, app.as_deref(), services.as_deref()),
        },

        Commands::Releases(sub) => match sub {
            ReleasesCommands::List { app } => commands::releases::list(app.as_deref()),
            ReleasesCommands::Show { release_id, app } => {
                commands::releases::show(&release_id, app.as_deref())
            }
            ReleasesCommands::Promote { app, tag } => {
                commands::releases::promote(app.as_deref(), tag.as_deref())
            }
        },

        Commands::Skills(sub) => match sub {
            SkillsCommands::Install { path, print } => commands::skills::install(path, print),
        },

        Commands::Docs { topic } => commands::docs::docs(topic.as_deref()),

        Commands::Discover => commands::command_tree::commands(),

        Commands::Logs {
            app,
            tail,
            since,
            error,
            severity,
            services,
            search,
            live,
            output,
        } => {
            let severity = if error {
                Some("ERROR".to_string())
            } else {
                severity
            };
            commands::logs::logs(commands::logs::LogsArgs {
                app_flag: app,
                tail,
                since,
                severity,
                services,
                search,
                live,
                output_path: output,
            });
        }
        Commands::Version => commands::update::version(),
        Commands::Update { version } => commands::update::update(version.as_deref()),
    }

    // Post-command: print notice if download completed during this run
    if let Some(handle) = version_handle {
        handle.print_notice();
    }
}
