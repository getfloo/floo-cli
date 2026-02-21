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
    #[arg(long)]
    pub json: bool,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Deploy a project to Floo.
    Deploy {
        /// Project directory to deploy.
        #[arg(default_value = ".")]
        path: PathBuf,

        /// App name (generated if omitted).
        #[arg(short, long)]
        name: Option<String>,

        /// Existing app ID or name to deploy to.
        #[arg(short, long)]
        app: Option<String>,
    },

    /// Authenticate and manage your account.
    #[command(subcommand)]
    Auth(AuthCommands),

    /// Manage your apps.
    #[command(subcommand)]
    Apps(AppsCommands),

    /// Manage environment variables.
    #[command(subcommand)]
    Env(EnvCommands),

    /// Show database details for an app.
    #[command(subcommand)]
    Db(DbCommands),

    /// Manage custom domains.
    #[command(subcommand)]
    Domains(DomainsCommands),

    /// View deploy history for rollback.
    #[command(subcommand)]
    Rollbacks(RollbacksCommands),

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

    /// View runtime logs for an app.
    Logs {
        /// App name or ID.
        app: String,

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

        /// Filter logs to a specific service (e.g., "api", "web").
        #[arg(long)]
        service: Option<String>,

        /// Write logs to a file (JSON or plain text based on --json flag).
        #[arg(short, long)]
        output: Option<PathBuf>,
    },

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
    Login,
    /// Clear stored credentials.
    Logout,
    /// Show the currently authenticated user.
    Whoami,
    /// Create a new Floo account.
    Register {
        /// Account email address.
        email: String,
    },
}

#[derive(Subcommand)]
pub enum AppsCommands {
    /// List all your apps.
    List,

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
}

#[derive(Subcommand)]
pub enum EnvCommands {
    /// Set an environment variable on an app.
    Set {
        /// KEY=VALUE pair to set.
        key_value: String,

        /// App name or ID.
        #[arg(short, long)]
        app: String,
    },

    /// List environment variables for an app.
    List {
        /// App name or ID.
        #[arg(short, long)]
        app: String,
    },

    /// Remove an environment variable from an app.
    Remove {
        /// Environment variable key to remove.
        key: String,

        /// App name or ID.
        #[arg(short, long)]
        app: String,
    },
}

#[derive(Subcommand)]
pub enum DomainsCommands {
    /// Add a custom domain to an app.
    Add {
        /// Domain hostname (e.g. app.example.com).
        hostname: String,

        /// App name or ID.
        #[arg(short, long)]
        app: String,
    },

    /// List custom domains for an app.
    List {
        /// App name or ID.
        #[arg(short, long)]
        app: String,
    },

    /// Remove a custom domain from an app.
    Remove {
        /// Domain hostname to remove.
        hostname: String,

        /// App name or ID.
        #[arg(short, long)]
        app: String,
    },
}

#[derive(Subcommand)]
pub enum DbCommands {
    /// Show database connection details for an app.
    Info {
        /// App name or ID.
        app: String,
    },
}

#[derive(Subcommand)]
pub enum RollbacksCommands {
    /// List deploys available for rollback.
    List {
        /// App name or ID.
        app: String,
    },
}

pub fn run() {
    let cli = Cli::parse();

    if cli.json {
        output::set_json_mode(true);
    }

    match cli.command {
        Commands::Deploy { path, name, app } => commands::deploy::deploy(path, name, app),
        Commands::Auth(sub) => match sub {
            AuthCommands::Login => commands::auth::login(),
            AuthCommands::Logout => commands::auth::logout(),
            AuthCommands::Whoami => commands::auth::whoami(),
            AuthCommands::Register { email } => commands::auth::register(&email),
        },

        Commands::Apps(sub) => match sub {
            AppsCommands::List => commands::apps::list(),
            AppsCommands::Status { app_name } => commands::apps::status(&app_name),
            AppsCommands::Delete { app_name, force } => commands::apps::delete(&app_name, force),
        },

        Commands::Env(sub) => match sub {
            EnvCommands::Set { key_value, app } => commands::env::set(&key_value, &app),
            EnvCommands::List { app } => commands::env::list(&app),
            EnvCommands::Remove { key, app } => commands::env::remove(&key, &app),
        },

        Commands::Db(sub) => match sub {
            DbCommands::Info { app } => commands::db::info(&app),
        },

        Commands::Domains(sub) => match sub {
            DomainsCommands::Add { hostname, app } => commands::domains::add(&hostname, &app),
            DomainsCommands::List { app } => commands::domains::list(&app),
            DomainsCommands::Remove { hostname, app } => commands::domains::remove(&hostname, &app),
        },

        Commands::Rollbacks(sub) => match sub {
            RollbacksCommands::List { app } => commands::rollbacks::list(&app),
        },

        Commands::Rollback {
            app,
            deploy_id,
            force,
        } => commands::rollbacks::rollback(&app, &deploy_id, force),

        Commands::Logs {
            app,
            tail,
            since,
            error,
            severity,
            service,
            output,
        } => {
            let sev = if error {
                Some("ERROR")
            } else {
                severity.as_deref()
            };
            commands::logs::logs(
                &app,
                tail,
                since.as_deref(),
                sev,
                service.as_deref(),
                output.as_deref(),
            );
        }
        Commands::Version => commands::update::version(),
        Commands::Update { version } => commands::update::update(version.as_deref()),
    }
}
