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

        /// Existing app ID or name to deploy to.
        #[arg(short, long)]
        app: Option<String>,

        /// Deploy only these services (repeatable: --services api --services web).
        #[arg(short, long = "services")]
        services: Vec<String>,
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

    /// Manage services for an app.
    #[command(subcommand)]
    Services(ServicesCommands),

    /// Manage custom domains.
    #[command(subcommand)]
    Domains(DomainsCommands),

    /// Manage releases.
    #[command(subcommand)]
    Releases(ReleasesCommands),

    /// Promote an app to prod by creating a GitHub release.
    Promote {
        /// App name or ID (uses config file if omitted).
        #[arg(short, long)]
        app: Option<String>,

        /// Release tag (auto-generated if omitted).
        #[arg(short, long)]
        tag: Option<String>,
    },

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

    /// Connect a GitHub repo to an app for auto-deploy.
    Connect {
        /// GitHub repo (owner/repo).
        #[arg(long)]
        repo: String,

        /// GitHub App installation ID.
        #[arg(long)]
        installation_id: u64,

        /// App name or ID.
        #[arg(short, long)]
        app: String,

        /// Default branch (defaults to "main").
        #[arg(short, long)]
        branch: Option<String>,
    },

    /// Disconnect a GitHub repo from an app.
    Disconnect {
        /// App name or ID.
        #[arg(short, long)]
        app: String,
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
        file: Option<PathBuf>,

        /// App name or ID (reads from config if omitted).
        #[arg(short, long)]
        app: Option<String>,

        /// Target specific services (repeatable).
        #[arg(long)]
        services: Vec<String>,
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
}

#[derive(Subcommand)]
pub enum RollbacksCommands {
    /// List deploys available for rollback.
    List {
        /// App name or ID (uses config file if omitted).
        #[arg(short, long)]
        app: Option<String>,
    },
}

pub fn run() {
    let cli = Cli::parse();

    if cli.json {
        output::set_json_mode(true);
    }

    match cli.command {
        Commands::Deploy {
            path,
            app,
            services,
        } => commands::deploy::deploy(path, app, services),
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
            AppsCommands::Connect {
                repo,
                installation_id,
                app,
                branch,
            } => commands::apps::connect(&repo, installation_id, &app, branch.as_deref()),
            AppsCommands::Disconnect { app } => commands::apps::disconnect(&app),
        },

        Commands::Env(sub) => match sub {
            EnvCommands::Set {
                key_value,
                app,
                services,
            } => commands::env::set(&key_value, app.as_deref(), &services),
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
            } => commands::env::import_vars(file.as_deref(), app.as_deref(), &services),
        },

        Commands::Services(sub) => match sub {
            ServicesCommands::List { app } => commands::services::list(app.as_deref()),
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
        },

        Commands::Promote { app, tag } => {
            commands::releases::promote(app.as_deref(), tag.as_deref())
        }

        Commands::Rollbacks(sub) => match sub {
            RollbacksCommands::List { app } => commands::rollbacks::list(app.as_deref()),
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
            services,
            search,
            live,
            output,
        } => {
            let sev = if error {
                Some("ERROR")
            } else {
                severity.as_deref()
            };
            commands::logs::logs(
                app.as_deref(),
                tail,
                since.as_deref(),
                sev,
                &services,
                search.as_deref(),
                live,
                output.as_deref(),
            );
        }
        Commands::Version => commands::update::version(),
        Commands::Update { version } => commands::update::update(version.as_deref()),
    }
}
