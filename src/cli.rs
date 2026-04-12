use std::path::PathBuf;

use clap::{Parser, Subcommand};

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

    /// Run all services locally with managed-service credentials.
    #[command(after_help = "\
Examples:
  floo dev                                 Start all services defined in floo.app.toml
  floo dev --app my-app                    Explicitly specify the app")]
    Dev {
        /// App name or ID (reads from config if omitted).
        #[arg(short, long)]
        app: Option<String>,
    },

    /// Validate project config, detect runtimes, and check readiness.
    #[command(after_help = "\
Examples:
  floo preflight                           Validate current directory
  floo preflight ./app --json              Validate ./app with JSON output")]
    Preflight {
        /// Project directory to validate.
        #[arg(default_value = ".")]
        path: PathBuf,

        /// Existing app name to validate against.
        #[arg(short, long)]
        app: Option<String>,

        /// Validate only these services.
        #[arg(short, long = "services")]
        services: Vec<String>,
    },

    /// Force a redeploy (after env var changes, config updates, or to rebuild).
    #[command(after_help = "\
Examples:
  floo redeploy --app my-app               Redeploy with fresh env vars (no rebuild)
  floo redeploy --app my-app --rebuild     Force a full rebuild from latest commit
  floo redeploy                            Redeploy from current project directory
  floo redeploy --services api             Redeploy specific services only

Note: The primary way to deploy is `git push`. Use `floo redeploy` when you
need to apply env var changes or force a rebuild without a code change.")]
    Redeploy {
        /// Project directory (only needed when --app is not provided).
        #[arg(default_value = ".")]
        path: PathBuf,

        /// Existing app ID or name to deploy to.
        #[arg(short, long)]
        app: Option<String>,

        /// Deploy only these services (repeatable: --services api --services web).
        #[arg(short, long = "services")]
        services: Vec<String>,

        /// Force a full rebuild from the latest commit (re-download source and run Cloud Build).
        #[arg(long)]
        rebuild: bool,

        /// Re-sync env vars from configured env_file before deploying.
        #[arg(long)]
        sync_env: bool,
    },

    /// View and manage deploy history.
    #[command(
        name = "deploys",
        subcommand,
        after_help = "\
Examples:
  floo deploys list --app my-app            Show deploy history
  floo deploys logs <id> --follow           Stream build logs in real-time
  floo deploys watch --app my-app           Stream deploy progress
  floo deploys rollback my-app abc123       Rollback to a previous deploy

Note: To trigger a deploy, use `floo redeploy` or push to GitHub."
    )]
    Deploys(DeploysSubcommands),

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

        /// Environment to query: dev or prod.
        #[arg(long, value_parser = ["dev", "prod"])]
        env: Option<String>,
    },

    /// Install agent skills for AI coding assistants.
    #[command(subcommand)]
    Skills(SkillsCommands),

    /// Send feedback, report bugs, or request features.
    #[command(after_help = "\
Examples:
  floo feedback \"deploy logs are hard to read\"
  floo feedback --category bug \"deploys fail when Dockerfile is missing\"
  floo feedback --category feature_request \"add support for monorepos\"
  floo feedback --app my-app \"this app crashes on startup\"
  floo feedback --json \"friction with env var sync\" --category friction")]
    Feedback {
        /// Your feedback message.
        message: String,

        /// Category: bug, friction, feature_request, or general.
        #[arg(short, long, default_value = "general", value_parser = ["bug", "friction", "feature_request", "general"])]
        category: String,

        /// App name (attach feedback to a specific app).
        #[arg(short, long)]
        app: Option<String>,

        /// Extra context (error output, steps to reproduce, etc.).
        #[arg(long)]
        context: Option<String>,
    },

    /// Built-in platform documentation.
    Docs {
        /// Topic: services, config, deploy. Omit for overview.
        topic: Option<String>,
    },

    /// Query, inspect schema, and run migrations for an app's managed database.
    #[command(
        subcommand,
        after_help = "\
Examples:
  floo db query --app my-app \"SELECT * FROM users LIMIT 10\"
  floo db schema --app my-app
  floo db migrate --app my-app"
    )]
    Db(DbCommands),

    /// List and trigger scheduled cron jobs for an app.
    #[command(
        subcommand,
        after_help = "\
Examples:
  floo cron list --app my-app              List all cron jobs and their last run status
  floo cron run daily-report --app my-app  Manually trigger a cron job"
    )]
    Cron(CronCommands),

    /// Manage Reparo auto-recovery events.
    #[command(subcommand)]
    Reparo(ReparoCommands),

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

    /// Switch the default org for subsequent commands.
    Switch {
        /// Org slug or ID.
        org_slug: String,
    },
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

    /// Invite a user to an app (grant email-based access).
    Invite {
        /// Email address to invite.
        email: String,

        /// App name or ID (uses config file if omitted).
        #[arg(short, long)]
        app: Option<String>,
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

        /// Never open a browser (for agents/CI). Errors if GitHub App is not installed.
        #[arg(long)]
        no_browser: bool,
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

        /// Environment: dev or prod.
        #[arg(long, default_value = "dev", value_parser = ["dev", "prod"])]
        env: String,
    },

    /// List environment variables for an app.
    List {
        /// App name or ID (reads from config if omitted).
        #[arg(short, long)]
        app: Option<String>,

        /// Target specific services (repeatable).
        #[arg(long)]
        services: Vec<String>,

        /// Environment: dev or prod.
        #[arg(long, default_value = "dev", value_parser = ["dev", "prod"])]
        env: String,
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

        /// Environment: dev or prod.
        #[arg(long, default_value = "dev", value_parser = ["dev", "prod"])]
        env: String,
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

        /// Environment: dev or prod.
        #[arg(long, default_value = "dev", value_parser = ["dev", "prod"])]
        env: String,
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

        /// Environment: dev or prod.
        #[arg(long, default_value = "dev", value_parser = ["dev", "prod"])]
        env: String,
    },
}

#[derive(Subcommand)]
pub enum ServicesCommands {
    /// List all services for an app.
    List {
        /// App name or ID (uses config file if omitted).
        #[arg(short, long)]
        app: Option<String>,

        /// Environment to query: dev or prod.
        #[arg(long, default_value = "dev", value_parser = ["dev", "prod"])]
        env: String,
    },

    /// Show details for a service (managed or user-managed).
    Info {
        /// Service name.
        service_name: String,

        /// App name or ID (uses config file if omitted).
        #[arg(short, long)]
        app: Option<String>,
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

    /// Verify DNS for a pending custom domain.
    Verify {
        /// Domain hostname to verify.
        hostname: String,

        /// App name or ID (uses config file if omitted).
        #[arg(short, long)]
        app: Option<String>,
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

#[derive(Subcommand)]
pub enum DeploysSubcommands {
    /// List deploy history for an app.
    List {
        /// App name or ID (uses config file if omitted).
        #[arg(short, long)]
        app: Option<String>,
    },

    /// Show build logs for a deploy (defaults to the latest deploy).
    Logs {
        /// Deploy ID (defaults to latest deploy if omitted).
        deploy_id: Option<String>,

        /// App name or ID (uses config file if omitted).
        #[arg(short, long)]
        app: Option<String>,

        /// Follow logs in real-time (stream active deploys).
        #[arg(short, long)]
        follow: bool,
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

#[derive(Subcommand)]
pub enum DbCommands {
    /// Run a SQL query against the app's managed database.
    #[command(after_help = "\
Examples:
  floo db query --app my-app \"SELECT id, email FROM users LIMIT 5\"
  floo db query --app my-app \"SELECT COUNT(*) FROM orders\" --env prod
  floo db query --app my-app \"SELECT * FROM logs\" --limit 50")]
    Query {
        /// SQL query to execute.
        sql: String,

        /// App name or ID (reads from config if omitted).
        #[arg(short, long)]
        app: Option<String>,

        /// Environment to query: dev or prod.
        #[arg(long, default_value = "dev", value_parser = ["dev", "prod"])]
        env: String,

        /// Maximum number of rows to return.
        #[arg(long, default_value = "1000")]
        limit: u32,
    },

    /// Show the database schema for an app.
    Schema {
        /// App name or ID (reads from config if omitted).
        #[arg(short, long)]
        app: Option<String>,
    },

    /// Run database migrations for an app.
    Migrate {
        /// App name or ID (reads from config if omitted).
        #[arg(short, long)]
        app: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum ReparoCommands {
    /// List Reparo auto-recovery events for an app.
    Events {
        /// App name or ID (reads from config if omitted).
        #[arg(short, long)]
        app: Option<String>,

        /// Filter by event status (e.g. triggered, resolved, skipped).
        #[arg(long)]
        status: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum CronCommands {
    /// List all cron jobs for an app and their last run status.
    List {
        /// App name or ID (reads from config if omitted).
        #[arg(short, long)]
        app: Option<String>,
    },

    /// Manually trigger a cron job by name.
    Run {
        /// Cron job name (as defined in floo.app.toml [cron] section).
        name: String,

        /// App name or ID (reads from config if omitted).
        #[arg(short, long)]
        app: Option<String>,
    },
}

fn should_check_version(cli: &Cli) -> bool {
    if crate::config::is_local_binary() {
        return false;
    }
    if std::env::var("FLOO_NO_UPDATE_CHECK").is_ok() {
        return false;
    }
    // Only skip for update command (has its own update flow)
    !matches!(cli.command, Commands::Update { .. })
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
            | Commands::Orgs(
                OrgsCommands::Members(MembersCommands::SetRole { .. })
                    | OrgsCommands::Switch { .. },
            )
            | Commands::Apps(AppsCommands::Invite { .. })
            | Commands::Billing(BillingCommands::SpendCap(SpendCapCommands::Set { .. }))
    );
    if unsupported {
        output::error(
            "--dry-run is not supported for this command.",
            &crate::errors::ErrorCode::InvalidFormat,
            Some("Supported commands: redeploy, preflight, env set/remove/import, apps delete, domains add/remove, deploy rollback."),
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

    // Phase 2: Always apply staged updates (safe for any command, including --json)
    if !crate::config::is_local_binary() {
        crate::version_check::apply_staged_update(VERSION);
    }

    // Phase 1: Spawn background check + download (non-blocking, skipped for --json/version)
    let do_version_check = should_check_version(&cli);
    let version_handle = if do_version_check {
        crate::version_check::spawn_check(VERSION)
    } else {
        None
    };

    match cli.command {
        Commands::Analytics { app, period } => commands::analytics::analytics(app, &period),

        Commands::Init { name, path } => commands::init::init(name, path),

        Commands::Dev { app } => commands::dev::dev(app),

        Commands::Preflight {
            path,
            app,
            services,
        } => commands::deploy::preflight(path, app, services),

        Commands::Redeploy {
            path,
            app,
            services,
            rebuild,
            sync_env,
        } => commands::deploy::deploy(path, app, services, rebuild, sync_env),

        Commands::Deploys(sub) => match sub {
            DeploysSubcommands::List { app } => commands::deploys::list(app.as_deref()),
            DeploysSubcommands::Logs {
                deploy_id,
                app,
                follow,
            } => commands::deploys::logs(deploy_id.as_deref(), app.as_deref(), follow),
            DeploysSubcommands::Watch { app, commit } => {
                commands::deploys::watch(app.as_deref(), commit.as_deref())
            }
            DeploysSubcommands::Rollback {
                app,
                deploy_id,
                force,
            } => commands::rollbacks::rollback(&app, &deploy_id, force),
        },
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
            OrgsCommands::Switch { org_slug } => commands::orgs::switch(&org_slug),
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
                    no_browser,
                } => commands::github::connect(
                    &repo,
                    app.as_deref(),
                    branch.as_deref(),
                    skip_env_check,
                    no_deploy,
                    no_browser,
                ),
                GitHubCommands::Disconnect { app } => commands::github::disconnect(app.as_deref()),
                GitHubCommands::Status { app } => commands::github::status(app.as_deref()),
            },
            AppsCommands::Password { app_name } => commands::apps::show_password(&app_name),
            AppsCommands::Invite { email, app } => {
                commands::apps::invite(&email, app.as_deref())
            }
        },

        Commands::Env(sub) => match sub {
            EnvCommands::Set {
                key_value,
                app,
                services,
                restart,
                env,
            } => commands::env::set(&key_value, app.as_deref(), &services, restart, &env),
            EnvCommands::List { app, services, env } => {
                commands::env::list(app.as_deref(), &services, &env)
            }
            EnvCommands::Remove {
                key,
                app,
                services,
                env,
            } => commands::env::remove(&key, app.as_deref(), &services, &env),
            EnvCommands::Get {
                key,
                app,
                service,
                env,
            } => commands::env::get(&key, app.as_deref(), service.as_deref(), &env),
            EnvCommands::Import {
                file,
                app,
                services,
                all,
                env,
            } => {
                if all {
                    commands::env::import_all_services(app.as_deref(), &env);
                } else {
                    commands::env::import_vars(file.as_deref(), app.as_deref(), &services, &env);
                }
            }
        },

        Commands::Services(sub) => match sub {
            ServicesCommands::List { app, env } => commands::services::list(app.as_deref(), &env),
            ServicesCommands::Info { service_name, app } => {
                commands::services::info(&service_name, app.as_deref())
            }
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
            DomainsCommands::Verify { hostname, app } => {
                commands::domains::verify(&hostname, app.as_deref())
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

        Commands::Db(sub) => match sub {
            DbCommands::Query { sql, app, env, limit } => {
                commands::db::query(app.as_deref(), &sql, &env, limit)
            }
            DbCommands::Schema { app } => commands::db::schema(app.as_deref()),
            DbCommands::Migrate { app } => commands::db::migrate(app.as_deref()),
        },

        Commands::Cron(sub) => match sub {
            CronCommands::List { app } => commands::cron::list(app.as_deref()),
            CronCommands::Run { name, app } => commands::cron::run(app.as_deref(), &name),
        },

        Commands::Reparo(sub) => match sub {
            ReparoCommands::Events { app, status } => {
                commands::reparo::events(app.as_deref(), status.as_deref())
            }
        },

        Commands::Feedback {
            message,
            category,
            app,
            context,
        } => commands::feedback::feedback(&message, &category, app.as_deref(), context.as_deref()),

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
            env,
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
                env,
            });
        }
        Commands::Version => commands::update::version(),
        Commands::Update { version } => commands::update::update(version.as_deref()),
    }

    // Post-command: apply any update that was downloaded during this run
    if let Some(handle) = version_handle {
        handle.apply_and_notify(VERSION, !cli.json);
    }
}
