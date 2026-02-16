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

    /// Authenticate with the Floo API and store credentials.
    Login {
        /// Account email.
        #[arg(short, long)]
        email: Option<String>,

        /// Account password.
        #[arg(short, long)]
        password: Option<String>,
    },

    /// Clear stored credentials.
    Logout,

    /// Show the currently authenticated user.
    Whoami,

    /// Manage your apps.
    #[command(subcommand)]
    Apps(AppsCommands),

    /// Manage environment variables.
    #[command(subcommand)]
    Env(EnvCommands),

    /// Manage custom domains.
    #[command(subcommand)]
    Domains(DomainsCommands),
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

pub fn run() {
    let cli = Cli::parse();

    if cli.json {
        output::set_json_mode(true);
    }

    match cli.command {
        Commands::Deploy { path, name, app } => commands::deploy::deploy(path, name, app),
        Commands::Login { email, password } => commands::auth::login(email, password),
        Commands::Logout => commands::auth::logout(),
        Commands::Whoami => commands::auth::whoami(),

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

        Commands::Domains(sub) => match sub {
            DomainsCommands::Add { hostname, app } => commands::domains::add(&hostname, &app),
            DomainsCommands::List { app } => commands::domains::list(&app),
            DomainsCommands::Remove { hostname, app } => commands::domains::remove(&hostname, &app),
        },
    }
}
