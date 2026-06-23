use std::ffi::OsString;
use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

use crate::commands;
use crate::constants::VERSION;
use crate::output;

#[derive(Parser)]
#[command(
    name = "floo",
    about = "Manage and observe web apps. Deploys are git-driven.",
    version = VERSION,
)]
pub struct Cli {
    /// Output JSON to stdout (for agents).
    #[arg(long, global = true)]
    pub json: bool,

    /// Preview what a command would do without executing it.
    #[arg(long, global = true)]
    pub dry_run: bool,

    /// Emit secret-shaped values verbatim in `--json` output instead of
    /// `***REDACTED***`. The default redaction protects agents that pipe
    /// stdout into transcripts and logs. Use this only when you control
    /// where the JSON goes (e.g. a local script that pipes into a file
    /// outside any agent context). The top-level `contains_secrets`
    /// marker still fires either way so harnesses can refuse the
    /// payload.
    #[arg(long, global = true)]
    pub reveal_secrets: bool,

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
    ///
    /// `--json` redacts secret-shaped values (DATABASE_URL, REDIS_URL,
    /// SECRET_KEY_BASE, etc.) by default and stamps the payload with
    /// `contains_secrets: true` so agent harnesses can refuse it. Pass
    /// `--reveal-secrets` only when you control where the JSON goes
    /// (a local script piping into a file outside any agent context).
    #[command(after_help = "\
Examples:
  floo dev                                              Start all services defined in floo.app.toml
  floo dev --app my-app                                 Explicitly specify the app
  floo dev --fixture-user you@example.com               Front each accounts-mode service with a local
                                                        proxy that injects X-Floo-User-* identity
                                                        headers, mirroring what floo's gateway does
                                                        in production. Prints both the raw service
                                                        URL and the auth-proxied URL.")]
    Dev {
        /// App name or ID. Overrides the app name in floo.app.toml, but the
        /// file itself is still required for service definitions.
        #[arg(short, long)]
        app: Option<String>,

        /// Email of the fixture user to inject as X-Floo-User-Email on every
        /// request to an accounts-mode service. When set, `floo dev` starts a
        /// local proxy in front of each accounts-mode service that adds the
        /// four identity headers (Email/Id/Name/Role) so your app sees the
        /// same shape it sees in production behind the floo gateway.
        ///
        /// No effect on non-accounts-mode apps.
        #[arg(long, value_name = "EMAIL")]
        fixture_user: Option<String>,

        /// Fixture user id to inject as X-Floo-User-Id (default: dev-fixture-<email-localpart>).
        #[arg(long, value_name = "ID", requires = "fixture_user")]
        fixture_id: Option<String>,

        /// Fixture user display name to inject as X-Floo-User-Name (default: the email).
        #[arg(long, value_name = "NAME", requires = "fixture_user")]
        fixture_name: Option<String>,

        /// Fixture user role to inject as X-Floo-User-Role (default: "member").
        #[arg(long, value_name = "ROLE", requires = "fixture_user")]
        fixture_role: Option<String>,
    },

    /// Run a one-shot command with a service's managed env vars injected.
    ///
    /// Creates a scoped dev session to fetch the service's env vars (DATABASE_URL,
    /// REDIS_URL, and any custom vars set via `floo env`), authorizes Postgres for
    /// direct connections if provisioned, runs the command in the service's directory,
    /// then tears down the session when the command exits.
    ///
    /// Exit code propagates exactly — a failing test suite returns non-zero.
    #[command(after_help = "\
Examples:
  floo run --service api -- pytest tests/unit/        Run tests with api env vars
  floo run --service worker -- python seed.py         Run a script with worker env vars
  floo run --service api --json -- pytest             Machine-readable exit code")]
    Run {
        /// Service name to inject env vars for (from floo.app.toml).
        #[arg(long, required = true)]
        service: String,

        /// App name or ID. Overrides the app name in floo.app.toml, but the
        /// file itself is still required for service definitions.
        #[arg(short, long)]
        app: Option<String>,

        /// The command to run (everything after --).
        #[arg(required = true, trailing_var_arg = true, allow_hyphen_values = true)]
        command: Vec<String>,
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

        /// Hotfix: declare no migrations are pending and skip the MIGRATE step.
        ///
        /// The platform takes you at your word — if you're wrong, the new
        /// image boots against an unmigrated schema. The deploy records
        /// MIGRATE as SKIPPED (not SUCCESS) so the audit trail is
        /// unambiguous about whether migrations ran. Meaningful only on
        /// rebuild deploys; rollback and restart already skip MIGRATE
        /// because the image was previously migrated.
        #[arg(long)]
        skip_migrations: bool,
    },

    /// View and manage deploy history.
    #[command(
        name = "deploys",
        alias = "deploy",
        subcommand,
        after_help = "\
Examples:
  floo deploys list --app my-app            Show deploy history
  floo deploys logs <id> --follow           Stream build logs in real-time
  floo deploys watch --app my-app           Stream deploy progress
  floo deploys rollback my-app abc123       Rollback to a previous deploy

Note: To trigger a deploy, use `floo redeploy` or push to GitHub.
`floo deploy ...` is a backwards-compatible alias for `floo deploys ...`."
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
  floo apps show my-app --json             App details and service info
  floo apps delete my-app                  Delete (typed-name confirmation)
  floo apps delete my-app --yes-i-know-this-destroys-data  Skip prompt (CI-only)"
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

    /// View and tune which emails floo sends you.
    #[command(
        subcommand,
        after_help = "\
Examples:
  floo notifications list                     Show your email settings
  floo notifications list --json              Machine-readable for agents
  floo notifications set deploy_success on    Email me on every successful deploy
  floo notifications set billing off          Stop spend-cap warning emails

Run `floo notifications list` to see the available categories."
    )]
    Notifications(NotificationsCommands),

    /// Manage services for an app.
    #[command(subcommand)]
    Services(ServicesCommands),

    /// Recover objects from floo-managed storage.
    #[command(subcommand)]
    Storage(StorageCommands),

    /// Manage custom domains.
    #[command(subcommand)]
    Domains(DomainsCommands),

    /// Manage releases and promote to prod.
    #[command(subcommand)]
    Releases(ReleasesCommands),

    /// View runtime logs for an app.
    #[command(after_help = "\
Examples:
  floo logs query --app my-app --since 1h         Query stored runtime logs
  floo logs query --app my-app --deployment latest --json
  floo logs tail --app my-app --env prod          Stream runtime logs
  floo logs --app my-app --live                   Backwards-compatible tail")]
    Logs {
        #[command(subcommand)]
        command: Option<Box<LogsSubcommands>>,

        #[command(flatten)]
        options: LogsOptions,
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
        long_about = "\
List and trigger scheduled cron jobs for an app.

Cron jobs are declared in floo.app.toml under [cron.<name>] sections — they
are not created with the CLI. This command surface is read-only (`list`, `show`)
plus manual trigger (`run`); jobs are reconciled from config on every deploy.

See `floo docs app-toml` (look for 'Cron Jobs') for the full [cron.<name>]
schema, or https://getfloo.com/docs/guides/cron-jobs for the long-form guide.",
        after_help = "\
Examples:
  floo cron list --app my-app              List all cron jobs and their last run status
  floo cron show daily-report --app my-app Show details for a single cron job
  floo cron run daily-report --app my-app  Manually trigger a cron job

Schedules are declared in floo.app.toml under [cron.<name>]. This CLI is
read-only + manual trigger; new jobs are added by editing config and deploying."
    )]
    Cron(CronCommands),

    /// Manage Reparo auto-recovery events.
    #[command(subcommand)]
    Reparo(ReparoCommands),

    /// Diagnose an app's posture in one round trip.
    #[command(
        subcommand,
        long_about = "\
Diagnose an app's posture in one round trip.

Read-only diagnostic surface — answers questions like 'why isn't accounts
mode active?' without requiring agents to curl the gateway, parse request
logs, and join four database tables by hand.

The endpoint is intentionally narrow: no env-var values, no secret
material, no Cloud Run audit payloads. Those live on dedicated surfaces."
    )]
    Doctor(DoctorCommands),

    /// List all commands (structured for agents in --json mode).
    #[command(name = "commands")]
    Discover,

    /// Print installed CLI version.
    Version,

    /// Update the CLI binary in-place.
    ///
    /// Downloads the target release and overwrites the file at `floo`'s current path
    /// on disk. There is no automatic rollback — to revert, reinstall the desired
    /// version with the installer or download the binary directly. Use `--dry-run` to
    /// preview the release without touching the binary.
    Update {
        /// Specific release tag to install (e.g. v0.2.0).
        #[arg(long)]
        version: Option<String>,
    },
}

#[derive(Args, Clone)]
pub struct LogsOptions {
    /// App name or ID (overrides config file).
    #[arg(short, long)]
    app: Option<String>,

    /// Number of log lines to show.
    #[arg(short, long, visible_alias = "limit", default_value = "100")]
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

    /// Filter by deployment ID, or 'latest' for the app's latest deploy.
    #[arg(long)]
    deployment: Option<String>,

    /// Continue a paginated log query from a previous next_cursor value.
    #[arg(long)]
    cursor: Option<String>,

    /// Stream logs in real-time (poll every 2s). Works with --requests too.
    #[arg(short = 'f', long, alias = "follow", conflicts_with = "output")]
    live: bool,

    /// Write logs to a file (JSON or plain text based on --json flag).
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Environment to query: dev or prod.
    #[arg(long, value_parser = ["dev", "prod"])]
    env: Option<String>,

    /// Show HTTP requests captured by floo's gateway instead of app-level
    /// log output. Each line is one proxied request with the public URL,
    /// status, and latency.
    #[arg(long)]
    requests: bool,
}

#[derive(Subcommand, Clone)]
pub enum LogsSubcommands {
    /// Query stored runtime logs once.
    Query {
        #[command(flatten)]
        options: LogsOptions,
    },

    /// Tail runtime logs continuously.
    Tail {
        #[command(flatten)]
        options: LogsOptions,
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

    /// Show current usage, spend cap, and per-app compute breakdown.
    Usage {
        /// Time period to show compute costs for.
        #[arg(short, long, default_value = "current_month", value_parser = ["current_month", "last_month", "last_7d"])]
        period: String,
    },

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
    ///
    /// Accepts either a positional name or `--app` for parity with the rest
    /// of the CLI's flag-based API. Pass exactly one.
    Show {
        /// App name or ID (positional form). Mutually exclusive with `--app`.
        app_name: Option<String>,

        /// App name or ID. Mutually exclusive with the positional form.
        #[arg(short, long, conflicts_with = "app_name")]
        app: Option<String>,
    },

    /// Permanently delete an app and all its data.
    ///
    /// Tier-3 destructive: interactive mode requires typing the app name
    /// to confirm; non-interactive requires --yes-i-know-this-destroys-data.
    /// Never a plain --yes or --force — destroying user data must be an
    /// explicit, acknowledged decision.
    Delete {
        /// App name or ID.
        app_name: String,

        /// Skip interactive confirmation. Required in non-interactive contexts
        /// (JSON mode, CI, pipes). This flag is deliberately verbose; a script
        /// using it must have user authorization for this specific app.
        #[arg(long = "yes-i-know-this-destroys-data", alias = "force")]
        confirmed: bool,
    },

    /// Manage GitHub integration.
    #[command(subcommand)]
    Github(GitHubCommands),

    /// Show the shared password for a password-protected app.
    ///
    /// `--json` redacts the password by default. Pass
    /// `--reveal-secrets` to print the plaintext value (the response
    /// is still stamped with `contains_secrets: true` either way).
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
    ///
    /// Pass `KEY=VALUE` inline, or `KEY` alone with `--stdin` / `--value-file`
    /// to keep a secret value out of argv, shell history, and `ps`.
    Set {
        /// KEY=VALUE pair to set. With --stdin or --value-file, pass the KEY only.
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

        /// Read the value from stdin instead of the command line, keeping the
        /// secret out of argv / shell history / `ps`. A single trailing newline
        /// is stripped (so `echo "$SECRET" | floo env set KEY --stdin` works).
        #[arg(long, conflicts_with = "value_file")]
        stdin: bool,

        /// Read the value from a file. A single trailing newline is stripped.
        #[arg(long, value_name = "PATH", conflicts_with = "stdin")]
        value_file: Option<PathBuf>,
    },

    /// List environment variables for an app.
    ///
    /// Values are shown masked (`********`); the value itself is never echoed
    /// here. With no `--services`, lists every service's vars plus app-level in
    /// one pass — with a `Service` column labelling each row's scope whenever the
    /// app has services (a service-less app keeps the plain Key/Value table).
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

    /// Remove (unset) an environment variable from an app.
    Unset {
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
    ///
    /// Secret-shaped values (by key name or value content) are refused unless
    /// you pass `--reveal-secrets`, in both human and `--json` output, so a
    /// secret is never echoed to a terminal or transcript by accident. Plain,
    /// non-secret values print without the flag.
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
pub enum NotificationsCommands {
    /// Show which emails floo sends you, and whether each is on or off.
    List,

    /// Turn one category of email on or off.
    Set {
        /// Category to change (run `floo notifications list` to see them).
        category: String,

        /// Whether to receive this category.
        #[arg(value_parser = ["on", "off"])]
        value: String,
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
    ///
    /// Accepts the service name as a positional or via `--service` for parity
    /// with the rest of the CLI's flag-based API. Pass exactly one.
    /// `--services` is accepted as a plural alias to match the multi-service
    /// flag used by `floo logs` / `floo redeploy`.
    Show {
        /// Service name (positional form). Mutually exclusive with `--service`.
        service_name: Option<String>,

        /// App name or ID (uses config file if omitted).
        #[arg(short, long)]
        app: Option<String>,

        /// Service name. Mutually exclusive with the positional form.
        #[arg(long = "service", alias = "services", conflicts_with = "service_name")]
        service: Option<String>,
    },

    /// Provision a managed service (postgres, redis, or storage).
    Add {
        /// Service type: postgres, redis, or storage.
        #[arg(value_parser = ["postgres", "redis", "storage"])]
        service_type: String,

        /// App name or ID (uses config file if omitted).
        #[arg(short, long)]
        app: Option<String>,

        /// Deprecated. Every managed Postgres service now ships with the
        /// same defaults (25 connections, 60s statement timeout); the tier
        /// value is recorded but ignored. For sustained higher concurrency,
        /// email team@getfloo.com for a dedicated instance.
        #[arg(long, default_value = "basic", value_parser = ["basic", "standard", "performance"])]
        tier: String,

        /// Service row name (lowercase, alphanumeric + underscores).
        #[arg(long, default_value = "default")]
        name: String,
    },

    /// Permanently destroy a managed service and its data.
    ///
    /// Tier-3 destructive: requires typing the resource name to confirm,
    /// or --yes-i-know-this-destroys-data in automation. Never a plain --yes.
    Remove {
        /// Service type: postgres, redis, or storage.
        #[arg(value_parser = ["postgres", "redis", "storage"])]
        service_type: String,

        /// App name or ID (uses config file if omitted).
        #[arg(short, long)]
        app: Option<String>,

        /// Service row name (defaults to the one matching the type).
        #[arg(long, default_value = "default")]
        name: String,

        /// Skip interactive confirmation. Required in non-interactive contexts.
        /// This is deliberately verbose — destroying user data must be
        /// an explicit, acknowledged decision.
        #[arg(long = "yes-i-know-this-destroys-data")]
        confirmed: bool,
    },

    /// Migrate legacy [postgres]/[redis]/[storage] TOML sections to CLI-managed state.
    ///
    /// Reads floo.app.toml, ensures each declared managed service is provisioned
    /// (idempotent — existing services are recorded, not re-created), and writes
    /// .floo/services.lock. Zero data impact: the underlying managed services
    /// are not touched. Prints instructions to delete the TOML sections afterward.
    Migrate {
        /// App name or ID (uses config file if omitted).
        #[arg(short, long)]
        app: Option<String>,

        /// Project directory containing floo.app.toml.
        #[arg(default_value = ".")]
        path: PathBuf,
    },
}

#[derive(Subcommand)]
pub enum StorageCommands {
    /// List restorable versions for a managed storage object.
    Versions {
        /// Object path inside the bucket.
        object_path: String,

        /// App name or ID (uses config file if omitted).
        #[arg(short, long)]
        app: Option<String>,

        /// Managed storage service name.
        #[arg(long, default_value = "default")]
        name: String,

        /// Environment to inspect: dev or prod.
        #[arg(long, default_value = "dev", value_parser = ["dev", "prod"])]
        env: String,
    },

    /// Restore a managed storage object generation over the live object.
    Restore {
        /// Object path inside the bucket.
        object_path: String,

        /// GCS generation id to restore.
        #[arg(long)]
        generation: String,

        /// App name or ID (uses config file if omitted).
        #[arg(short, long)]
        app: Option<String>,

        /// Managed storage service name.
        #[arg(long, default_value = "default")]
        name: String,

        /// Environment to restore into: dev or prod.
        #[arg(long, default_value = "dev", value_parser = ["dev", "prod"])]
        env: String,
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
    ///
    /// Tier-2 destructive: interactive prompts `y/N`; non-interactive
    /// requires `--yes` to confirm.
    Remove {
        /// Domain hostname to remove.
        hostname: String,

        /// App name or ID (uses config file if omitted).
        #[arg(short, long)]
        app: Option<String>,

        /// Target service name (required for multi-service apps).
        #[arg(long)]
        services: Option<String>,

        /// Skip the y/N prompt. Required in non-interactive contexts.
        #[arg(long)]
        yes: bool,
    },

    /// Show detailed status for a single custom domain.
    Show {
        /// Domain hostname.
        hostname: String,

        /// App name or ID (uses config file if omitted).
        #[arg(short, long)]
        app: Option<String>,
    },

    /// Poll until a domain is active, failed, or the timeout expires.
    Watch {
        /// Domain hostname to watch.
        hostname: String,

        /// App name or ID (uses config file if omitted).
        #[arg(short, long)]
        app: Option<String>,

        /// Maximum seconds to wait (default: 300).
        #[arg(long, default_value_t = 300)]
        timeout: u64,
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

    /// Roll back to a previous live deploy by re-pointing gateway routes
    /// at its image (no rebuild).
    ///
    /// Tier-2 destructive: interactive prompts `y/N`; non-interactive
    /// requires `--yes` to confirm. Equivalent to
    /// `floo deploys rollback <app> <id>`.
    Rollback {
        /// App name or ID.
        #[arg(short, long)]
        app: String,

        /// Deploy ID to roll back to (the previous live deploy). Aliased
        /// as `--deploy` for parity with the rest of the CLI's flag
        /// vocabulary, which already speaks "deploy" everywhere else.
        #[arg(long, alias = "deploy")]
        to: String,

        /// Skip the y/N prompt. Required in non-interactive contexts.
        #[arg(long, alias = "force")]
        yes: bool,
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

    /// Print a compact, agent-safe deploy status summary.
    ///
    /// Emits the latest deploy's id, commit, derived phase booleans
    /// (image_built / service_ready / host_bound), the gateway URL, and a
    /// next recommended command — without dumping build logs, Cloud Run
    /// audit payloads, or env-var values. Use this from agents and
    /// scripts that need to know "what state is the deploy in?" without
    /// risking secret exfiltration through verbose log output.
    Status {
        /// App name or ID (uses config file if omitted).
        #[arg(short, long)]
        app: Option<String>,

        /// Deploy ID. Defaults to the latest deploy for the app.
        #[arg(long)]
        id: Option<String>,
    },

    /// Rollback to a previous deploy.
    ///
    /// Tier-2 destructive: interactive prompts `y/N`; non-interactive
    /// requires `--yes` to confirm.
    Rollback {
        /// App name or ID.
        app: String,

        /// Deploy ID to rollback to.
        deploy_id: String,

        /// Skip the y/N prompt. Required in non-interactive contexts.
        #[arg(long, alias = "force")]
        yes: bool,
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
    ///
    /// Tables live in a per-app namespaced Postgres schema (e.g.
    /// `app_<unique_id>_<env>`), not `public`. The API auto-sets
    /// `search_path` to that schema, so unqualified references like
    /// `SELECT * FROM users` work. Introspection queries that hard-code
    /// `WHERE table_schema = 'public'` will return empty. Use
    /// `current_schema()` or run `floo db schema` to see the actual schema
    /// name.
    #[command(after_help = "\
Examples:
  floo db query --app my-app \"SELECT id, email FROM users LIMIT 5\"
  floo db query --app my-app \"SELECT COUNT(*) FROM orders\" --env prod
  floo db query --app my-app \"SELECT * FROM logs\" --limit 50
  floo db query --app my-app \"SELECT current_schema()\"   Show the namespaced schema")]
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
    #[command(after_help = "\
Examples:
  floo db migrate --app my-app              Run migrations against dev (default)
  floo db migrate --app my-app --env prod   Run migrations against prod")]
    Migrate {
        /// App name or ID (reads from config if omitted).
        #[arg(short, long)]
        app: Option<String>,

        /// Environment to migrate: dev or prod.
        #[arg(long, default_value = "dev", value_parser = ["dev", "prod"])]
        env: String,
    },

    /// Show current Postgres connection usage versus the role's limit.
    ///
    /// Useful for diagnosing "too many connections" errors and for
    /// deciding whether the app needs more capacity. Prints a percentage
    /// and surfaces team@getfloo.com when the role is near-saturated so
    /// you can request a dedicated instance without context-switching.
    #[command(after_help = "\
Examples:
  floo db connections --app my-app                Show dev connection usage
  floo db connections --app my-app --env prod     Show prod connection usage
  floo db connections --app my-app --json         Machine-readable output")]
    Connections {
        /// App name or ID (reads from config if omitted).
        #[arg(short, long)]
        app: Option<String>,

        /// Environment to query: dev or prod.
        #[arg(long, default_value = "dev", value_parser = ["dev", "prod"])]
        env: String,
    },

    /// Create a restorable backup of the app's managed Postgres schema.
    #[command(after_help = "\
Examples:
  floo db backup --app my-app               Back up dev (default)
  floo db backup --app my-app --env prod    Back up prod")]
    Backup {
        /// App name or ID (reads from config if omitted).
        #[arg(short, long)]
        app: Option<String>,

        /// Managed Postgres service name.
        #[arg(long, default_value = "default")]
        name: String,

        /// Environment to back up: dev or prod.
        #[arg(long, default_value = "dev", value_parser = ["dev", "prod"])]
        env: String,
    },

    /// List restorable managed Postgres backups.
    Backups {
        /// App name or ID (reads from config if omitted).
        #[arg(short, long)]
        app: Option<String>,

        /// Managed Postgres service name.
        #[arg(long, default_value = "default")]
        name: String,

        /// Environment to filter by: dev or prod.
        #[arg(long, value_parser = ["dev", "prod"])]
        env: Option<String>,
    },

    /// Restore a managed Postgres backup into its original env schema.
    #[command(after_help = "\
Examples:
  floo db restore 018f... --app my-app --env dev
  floo db restore 018f... --app my-app --env prod")]
    Restore {
        /// Backup ID returned by `floo db backups`.
        backup_id: String,

        /// App name or ID (reads from config if omitted).
        #[arg(short, long)]
        app: Option<String>,

        /// Managed Postgres service name.
        #[arg(long, default_value = "default")]
        name: String,

        /// Environment to restore: dev or prod.
        #[arg(long, default_value = "dev", value_parser = ["dev", "prod"])]
        env: String,
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
pub enum DoctorCommands {
    /// Diagnose an app's accounts-mode posture (feedback 64268e05).
    #[command(after_help = "\
Examples:
  floo doctor accounts                Use the app from floo.app.toml
  floo doctor accounts --app foo      Diagnose a specific app
  floo doctor accounts --json         Machine-readable output for agents

Exit code is non-zero when drift is detected, so scripts can branch on
`floo doctor accounts --json && deploy_things` without parsing the body.")]
    Accounts {
        /// App name or ID (reads from config if omitted).
        #[arg(short, long)]
        app: Option<String>,
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

    /// Show details for a single cron job (schedule, status, last run).
    ///
    /// Accepts the job name positionally or as `--name` for parity with
    /// the rest of the CLI's flag-based API. Pass exactly one.
    Show {
        /// Cron job name (positional form). Mutually exclusive with `--name`.
        name: Option<String>,

        /// App name or ID (reads from config if omitted).
        #[arg(short, long)]
        app: Option<String>,

        /// Cron job name. Mutually exclusive with the positional form.
        #[arg(long = "name", conflicts_with = "name")]
        name_flag: Option<String>,
    },

    /// Manually trigger a cron job by name.
    ///
    /// Accepts the job name positionally (legacy form) or as `--name` for
    /// parity with the rest of the CLI's flag-based API. Pass exactly one.
    Run {
        /// Cron job name as defined in floo.app.toml [cron] section
        /// (positional form). Mutually exclusive with `--name`.
        name: Option<String>,

        /// App name or ID (reads from config if omitted).
        #[arg(short, long)]
        app: Option<String>,

        /// Cron job name. Mutually exclusive with the positional form.
        #[arg(long = "name", conflicts_with = "name")]
        name_flag: Option<String>,
    },
}

fn should_check_version(cli: &Cli) -> bool {
    if crate::config::is_local_binary() {
        return false;
    }
    if std::env::var("FLOO_NO_UPDATE_CHECK").is_ok() {
        return false;
    }
    // Commands that drive their own update flow synchronously — skip the
    // background check to avoid a redundant download racing the foreground one.
    !matches!(cli.command, Commands::Update { .. } | Commands::Version)
}

/// Rewrites `floo --version` / `floo -V` to `floo version`.
///
/// Clap's built-in `version = VERSION` handler short-circuits inside `parse()`
/// before `run()` executes, so `--version` would otherwise never trigger the
/// updater path. We intercept argv before clap sees it so the subcommand runs.
///
/// The rewrite **appends** `version` to the end of the non-version args. This
/// is safe today because `Commands::Version` takes no positional arguments;
/// do NOT add positionals to `Commands::Version` without revisiting this.
///
/// Non-UTF-8 `OsString` args are conservatively treated as flag-like for
/// subcommand detection — all floo subcommand names are ASCII, so a non-UTF-8
/// positional can't be a valid subcommand and clap will reject it downstream
/// with its own error.
fn rewrite_bare_version_flag(args: Vec<OsString>) -> Vec<OsString> {
    if args.len() < 2 {
        return args;
    }

    // If any UTF-8 non-flag token follows the binary name, a subcommand
    // is present. See doc comment above re: non-UTF-8 handling.
    let has_subcommand = args.iter().skip(1).any(|arg| {
        arg.to_str()
            .is_some_and(|s| !s.starts_with('-') && !s.is_empty())
    });
    if has_subcommand {
        return args;
    }

    // No subcommand — look for --version/-V and rewrite.
    let mut has_version = false;
    let mut rewritten: Vec<OsString> = Vec::with_capacity(args.len() + 1);
    for (idx, arg) in args.into_iter().enumerate() {
        if idx == 0 {
            rewritten.push(arg);
            continue;
        }
        match arg.to_str() {
            Some("--version" | "-V") => has_version = true,
            _ => rewritten.push(arg),
        }
    }
    if has_version {
        rewritten.push(OsString::from("version"));
    }
    rewritten
}

/// Mutating commands that have not implemented `--dry-run`.
///
/// Read-only commands silently ignore the flag (they don't mutate, so a
/// preview of "do nothing" is a no-op). Mutating commands either implement
/// `--dry-run` themselves (handled in their own command module) or are
/// listed here so we error early instead of partially executing.
///
/// Tested via `dry_run_supported_names_are_real_subcommands` (which pins
/// the listed-supported set against parseable subcommands) and
/// `dry_run_unsupported_for_known_mutator` (a spot-check that a known
/// mutator stays in the unsupported set). A stale entry here fails fast.
fn dry_run_is_unsupported(command: &Commands) -> bool {
    matches!(
        command,
        Commands::Apps(AppsCommands::Github(
            GitHubCommands::Connect { .. } | GitHubCommands::Disconnect { .. },
        )) | Commands::Releases(ReleasesCommands::Promote { .. })
            | Commands::Orgs(
                OrgsCommands::Members(MembersCommands::SetRole { .. })
                    | OrgsCommands::Switch { .. },
            )
            | Commands::Apps(AppsCommands::Invite { .. })
            | Commands::Billing(BillingCommands::SpendCap(SpendCapCommands::Set { .. }))
            | Commands::Run { .. }
            | Commands::Feedback { .. }
            | Commands::Skills(SkillsCommands::Install { .. })
            | Commands::Auth(
                AuthCommands::Login { .. }
                    | AuthCommands::Logout
                    | AuthCommands::Register { .. }
                    | AuthCommands::UpdateProfile { .. }
            )
    )
}

/// Comma-joinable names of mutating commands that DO support `--dry-run`.
///
/// Single source of truth used in the error suggestion. Each entry must be
/// a real subcommand path (parseable by clap) — guarded by
/// `dry_run_supported_names_are_real_subcommands`.
///
/// **When you add a new `--dry-run` handler in a `commands/*.rs` module,
/// add an entry here AND a matching invocation row in the test.** Otherwise
/// the suggestion text in the error will silently lie about what's supported
/// — that drift class is exactly what this PR was opened to fix.
const DRY_RUN_SUPPORTED_COMMANDS: &[&str] = &[
    "init",
    "redeploy",
    "preflight",
    "dev",
    "update",
    "env set",
    "env unset",
    "env import",
    "apps delete",
    "domains add",
    "domains remove",
    "cron run",
    "deploys rollback",
    "db migrate",
    "db query",
];

fn reject_unsupported_dry_run(command: &Commands) {
    if !dry_run_is_unsupported(command) {
        return;
    }
    let suggestion = format!("Supported: {}.", DRY_RUN_SUPPORTED_COMMANDS.join(", "));
    output::error(
        "--dry-run is not supported for this command.",
        &crate::errors::ErrorCode::InvalidFormat,
        Some(&suggestion),
    );
    std::process::exit(1);
}

pub fn run() {
    let args = rewrite_bare_version_flag(std::env::args_os().collect());
    let cli = Cli::parse_from(args);

    if cli.json {
        output::set_json_mode(true);
    }
    if cli.dry_run {
        output::set_dry_run_mode(true);
        reject_unsupported_dry_run(&cli.command);
    }
    if cli.reveal_secrets {
        crate::redact::set_reveal_secrets(true);
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

        Commands::Dev {
            app,
            fixture_user,
            fixture_id,
            fixture_name,
            fixture_role,
        } => commands::dev::dev(commands::dev::DevArgs {
            app,
            fixture_user,
            fixture_id,
            fixture_name,
            fixture_role,
        }),

        Commands::Run {
            service,
            app,
            command,
        } => commands::run::run(&service, app, command),

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
            skip_migrations,
        } => commands::deploy::deploy(path, app, services, rebuild, sync_env, skip_migrations),

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
            DeploysSubcommands::Status { app, id } => {
                commands::deploys::status(app.as_deref(), id.as_deref())
            }
            DeploysSubcommands::Rollback {
                app,
                deploy_id,
                yes,
            } => commands::rollbacks::rollback(&app, &deploy_id, yes),
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
            BillingCommands::Usage { period } => commands::billing::usage(&period),
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
            AppsCommands::Show { app_name, app } => {
                commands::apps::status(app_name.as_deref().or(app.as_deref()))
            }
            AppsCommands::Delete {
                app_name,
                confirmed,
            } => commands::apps::delete(&app_name, confirmed),
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
            AppsCommands::Invite { email, app } => commands::apps::invite(&email, app.as_deref()),
        },

        Commands::Env(sub) => match sub {
            EnvCommands::Set {
                key_value,
                app,
                services,
                restart,
                env,
                stdin,
                value_file,
            } => commands::env::set(
                &key_value,
                app.as_deref(),
                &services,
                restart,
                &env,
                stdin,
                value_file.as_deref(),
            ),
            EnvCommands::List { app, services, env } => {
                commands::env::list(app.as_deref(), &services, &env)
            }
            EnvCommands::Unset {
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

        Commands::Notifications(sub) => match sub {
            NotificationsCommands::List => commands::notifications::list(),
            NotificationsCommands::Set { category, value } => {
                commands::notifications::set(&category, &value)
            }
        },

        Commands::Services(sub) => match sub {
            ServicesCommands::List { app, env } => commands::services::list(app.as_deref(), &env),
            ServicesCommands::Show {
                service_name,
                app,
                service,
            } => {
                let resolved = service_name.or(service).unwrap_or_else(|| {
                    output::error(
                        "Service name required.",
                        &crate::errors::ErrorCode::InvalidFormat,
                        Some("Pass --service <name> or supply the name positionally."),
                    );
                    std::process::exit(1);
                });
                commands::services::info(&resolved, app.as_deref())
            }
            ServicesCommands::Add {
                service_type,
                app,
                tier,
                name,
            } => commands::services::add(&service_type, app.as_deref(), &tier, &name),
            ServicesCommands::Remove {
                service_type,
                app,
                name,
                confirmed,
            } => commands::services::remove(&service_type, app.as_deref(), &name, confirmed),
            ServicesCommands::Migrate { app, path } => {
                commands::services::migrate(app.as_deref(), &path)
            }
        },

        Commands::Storage(sub) => match sub {
            StorageCommands::Versions {
                object_path,
                app,
                name,
                env,
            } => commands::storage::versions(app.as_deref(), &name, &env, &object_path),
            StorageCommands::Restore {
                object_path,
                generation,
                app,
                name,
                env,
            } => commands::storage::restore(app.as_deref(), &name, &env, &object_path, &generation),
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
                yes,
            } => commands::domains::remove(&hostname, app.as_deref(), services.as_deref(), yes),
            DomainsCommands::Show { hostname, app } => {
                commands::domains::status(&hostname, app.as_deref())
            }
            DomainsCommands::Watch {
                hostname,
                app,
                timeout,
            } => commands::domains::watch(&hostname, app.as_deref(), timeout),
        },

        Commands::Releases(sub) => match sub {
            ReleasesCommands::List { app } => commands::releases::list(app.as_deref()),
            ReleasesCommands::Show { release_id, app } => {
                commands::releases::show(&release_id, app.as_deref())
            }
            ReleasesCommands::Promote { app, tag } => {
                commands::releases::promote(app.as_deref(), tag.as_deref())
            }
            // `floo releases rollback --app X --to <id>` is the discoverable
            // alias for `floo deploys rollback <app> <id>`. Routes to the
            // same backend (`POST /v1/apps/{id}/rollback`) which re-points
            // gateway routes at the previous deploy's image without
            // rebuilding source.
            ReleasesCommands::Rollback { app, to, yes } => {
                commands::rollbacks::rollback(&app, &to, yes)
            }
        },

        Commands::Skills(sub) => match sub {
            SkillsCommands::Install { path, print } => commands::skills::install(path, print),
        },

        Commands::Db(sub) => match sub {
            DbCommands::Query {
                sql,
                app,
                env,
                limit,
            } => commands::db::query(app.as_deref(), &sql, &env, limit),
            DbCommands::Schema { app } => commands::db::schema(app.as_deref()),
            DbCommands::Migrate { app, env } => commands::db::migrate(app.as_deref(), &env),
            DbCommands::Connections { app, env } => commands::db::connections(app.as_deref(), &env),
            DbCommands::Backup { app, name, env } => {
                commands::db::backup(app.as_deref(), &name, &env)
            }
            DbCommands::Backups { app, name, env } => {
                commands::db::backups(app.as_deref(), &name, env.as_deref())
            }
            DbCommands::Restore {
                backup_id,
                app,
                name,
                env,
            } => commands::db::restore(app.as_deref(), &name, &env, &backup_id),
        },

        Commands::Cron(sub) => match sub {
            CronCommands::List { app } => commands::cron::list(app.as_deref()),
            CronCommands::Show {
                name,
                app,
                name_flag,
            } => {
                let resolved = name.or(name_flag).unwrap_or_else(|| {
                    output::error(
                        "Cron job name required.",
                        &crate::errors::ErrorCode::InvalidFormat,
                        Some("Pass --name <job> or supply the name positionally."),
                    );
                    std::process::exit(1);
                });
                commands::cron::show(app.as_deref(), &resolved)
            }
            CronCommands::Run {
                name,
                app,
                name_flag,
            } => {
                let resolved = name.or(name_flag).unwrap_or_else(|| {
                    output::error(
                        "Cron job name required.",
                        &crate::errors::ErrorCode::InvalidFormat,
                        Some("Pass --name <job> or supply the name positionally."),
                    );
                    std::process::exit(1);
                });
                commands::cron::run(app.as_deref(), &resolved)
            }
        },

        Commands::Reparo(sub) => match sub {
            ReparoCommands::Events { app, status } => {
                commands::reparo::events(app.as_deref(), status.as_deref())
            }
        },

        Commands::Doctor(sub) => match sub {
            DoctorCommands::Accounts { app } => commands::doctor::accounts(app.as_deref()),
        },

        Commands::Feedback {
            message,
            category,
            app,
            context,
        } => commands::feedback::feedback(&message, &category, app.as_deref(), context.as_deref()),

        Commands::Docs { topic } => commands::docs::docs(topic.as_deref()),

        Commands::Discover => commands::command_tree::commands(),

        Commands::Logs { command, options } => {
            let options = match command.map(|boxed| *boxed) {
                Some(LogsSubcommands::Query { options }) => options,
                Some(LogsSubcommands::Tail { options }) => {
                    if options.output.is_some() {
                        output::error(
                            "`floo logs tail` cannot write to --output.",
                            &crate::errors::ErrorCode::InvalidFormat,
                            Some(
                                "Use `floo logs query --output <path>` for a finite log snapshot.",
                            ),
                        );
                        std::process::exit(1);
                    }
                    let mut options = options;
                    options.live = true;
                    options
                }
                None => options,
            };

            if options.requests {
                commands::logs::request_logs(commands::logs::RequestLogsArgs {
                    app_flag: options.app,
                    tail: options.tail,
                    since: options.since,
                    live: options.live,
                });
            } else {
                let severity = if options.error {
                    Some("ERROR".to_string())
                } else {
                    options.severity
                };
                commands::logs::logs(commands::logs::LogsArgs {
                    app_flag: options.app,
                    tail: options.tail,
                    since: options.since,
                    severity,
                    services: options.services,
                    search: options.search,
                    deployment: options.deployment,
                    cursor: options.cursor,
                    live: options.live,
                    output_path: options.output,
                    env: options.env,
                });
            }
        }
        Commands::Version => commands::update::version(),
        Commands::Update { version } => commands::update::update(version.as_deref()),
    }

    // Post-command: apply any update that was downloaded during this run
    if let Some(handle) = version_handle {
        handle.apply_and_notify(VERSION, !cli.json);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(parts: &[&str]) -> Vec<OsString> {
        parts.iter().map(OsString::from).collect()
    }

    fn strs(args: &[OsString]) -> Vec<&str> {
        args.iter().map(|a| a.to_str().unwrap()).collect()
    }

    #[test]
    fn rewrite_version_long_flag() {
        let out = rewrite_bare_version_flag(argv(&["floo", "--version"]));
        assert_eq!(strs(&out), vec!["floo", "version"]);
    }

    #[test]
    fn rewrite_version_short_flag() {
        let out = rewrite_bare_version_flag(argv(&["floo", "-V"]));
        assert_eq!(strs(&out), vec!["floo", "version"]);
    }

    #[test]
    fn rewrite_version_with_json_flag() {
        // Non-version flags keep their relative order; `version` is appended
        // at the end regardless of where --version was in the original args.
        let out = rewrite_bare_version_flag(argv(&["floo", "--json", "--version"]));
        assert_eq!(strs(&out), vec!["floo", "--json", "version"]);

        let out = rewrite_bare_version_flag(argv(&["floo", "--version", "--json"]));
        assert_eq!(strs(&out), vec!["floo", "--json", "version"]);
    }

    #[test]
    fn rewrite_leaves_subcommand_unchanged() {
        // If a subcommand is already present, don't touch --version — let clap
        // handle any flag validation at the subcommand level.
        let out = rewrite_bare_version_flag(argv(&["floo", "redeploy", "--app", "my-app"]));
        assert_eq!(strs(&out), vec!["floo", "redeploy", "--app", "my-app"]);
    }

    #[test]
    fn rewrite_leaves_version_subcommand_unchanged() {
        let out = rewrite_bare_version_flag(argv(&["floo", "version"]));
        assert_eq!(strs(&out), vec!["floo", "version"]);
    }

    #[test]
    fn rewrite_leaves_bare_invocation_unchanged() {
        // `floo` alone should still hit clap's "missing subcommand" help path.
        let out = rewrite_bare_version_flag(argv(&["floo"]));
        assert_eq!(strs(&out), vec!["floo"]);
    }

    #[test]
    fn rewrite_leaves_help_flag_alone() {
        // --help has its own clap short-circuit and we don't need to network for it.
        let out = rewrite_bare_version_flag(argv(&["floo", "--help"]));
        assert_eq!(strs(&out), vec!["floo", "--help"]);
    }

    /// Each entry in DRY_RUN_SUPPORTED_COMMANDS must be a real subcommand path
    /// (parseable by clap) AND must NOT be in the unsupported set. This catches
    /// drift between the suggestion text and the matches! list — the original
    /// 2026-04-30 bug where the error string listed `cron add` (no such
    /// subcommand) and the matches! list rejected `cron run` (which actually
    /// implements --dry-run).
    #[test]
    fn dry_run_supported_names_are_real_subcommands() {
        // Minimal required positionals so each invocation parses.
        let invocations: &[(&str, &[&str])] = &[
            ("init", &["floo", "init", "--dry-run"]),
            ("redeploy", &["floo", "redeploy", "--dry-run"]),
            ("preflight", &["floo", "preflight", "--dry-run"]),
            ("dev", &["floo", "dev", "--dry-run"]),
            ("update", &["floo", "update", "--dry-run"]),
            ("env set", &["floo", "env", "set", "K=V", "--dry-run"]),
            ("env unset", &["floo", "env", "unset", "K", "--dry-run"]),
            (
                "env import",
                &["floo", "env", "import", ".env", "--dry-run"],
            ),
            (
                "apps delete",
                &["floo", "apps", "delete", "myapp", "--dry-run"],
            ),
            (
                "domains add",
                &["floo", "domains", "add", "x.com", "--dry-run"],
            ),
            (
                "domains remove",
                &["floo", "domains", "remove", "x.com", "--dry-run"],
            ),
            ("cron run", &["floo", "cron", "run", "myjob", "--dry-run"]),
            (
                "deploys rollback",
                &["floo", "deploys", "rollback", "myapp", "abc", "--dry-run"],
            ),
            (
                "db migrate",
                &["floo", "db", "migrate", "--app", "myapp", "--dry-run"],
            ),
            (
                "db query",
                &[
                    "floo",
                    "db",
                    "query",
                    "SELECT 1",
                    "--app",
                    "myapp",
                    "--dry-run",
                ],
            ),
        ];

        let listed: std::collections::HashSet<&str> =
            DRY_RUN_SUPPORTED_COMMANDS.iter().copied().collect();
        let invoked: std::collections::HashSet<&str> =
            invocations.iter().map(|(name, _)| *name).collect();
        assert_eq!(
            listed, invoked,
            "DRY_RUN_SUPPORTED_COMMANDS and the test invocation table must list the same names",
        );

        for (label, args) in invocations {
            let cli = Cli::try_parse_from(args.iter().copied())
                .unwrap_or_else(|e| panic!("clap rejected '{label}' invocation {args:?}: {e}"));
            assert!(cli.dry_run, "expected --dry-run set for '{label}'");
            assert!(
                !dry_run_is_unsupported(&cli.command),
                "'{label}' is listed as supported but dry_run_is_unsupported() returns true",
            );
        }
    }

    #[test]
    fn dry_run_unsupported_for_known_mutator() {
        // `floo apps github connect` does not implement --dry-run; the
        // gate must catch it. (Replaces the prior `floo init` spot-check
        // since `init` now implements --dry-run.)
        let cli = Cli::try_parse_from([
            "floo",
            "apps",
            "github",
            "connect",
            "owner/repo",
            "--dry-run",
        ])
        .unwrap();
        assert!(dry_run_is_unsupported(&cli.command));
    }
}
