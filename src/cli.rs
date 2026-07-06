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
        /// App name or ID (positional). Omit for org-level overview.
        app: Option<String>,

        /// App name or ID — `--app` form, consistent with `floo logs`. Takes
        /// precedence over the positional argument if both are given.
        #[arg(short = 'a', long = "app")]
        app_flag: Option<String>,

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
        #[arg(long = "service", visible_alias = "services", required = true)]
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

        /// Validate only these services (repeatable, or comma-separated).
        #[arg(
            short,
            long = "service",
            visible_alias = "services",
            value_delimiter = ','
        )]
        services: Vec<String>,
    },

    /// Force a redeploy (after env var changes, config updates, or to rebuild).
    #[command(after_help = "\
Examples:
  floo redeploy --app my-app               Redeploy with fresh env vars (no rebuild)
  floo redeploy --app my-app --rebuild     Force a full rebuild from latest commit
  floo redeploy                            Redeploy from current project directory
  floo redeploy --service api              Redeploy specific services only

Note: The primary way to deploy is `git push`. Use `floo redeploy` when you
need to apply env var changes or force a rebuild without a code change.")]
    Redeploy {
        /// Project directory (only needed when --app is not provided).
        #[arg(default_value = ".")]
        path: PathBuf,

        /// Existing app ID or name to deploy to.
        #[arg(short, long)]
        app: Option<String>,

        /// Deploy only these services (--service api --service web, or --service api,web).
        #[arg(
            short,
            long = "service",
            visible_alias = "services",
            value_delimiter = ','
        )]
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

    /// Create and manage preview sandboxes for remote GitHub branches.
    #[command(
        name = "previews",
        alias = "preview",
        subcommand,
        after_help = "\
Examples:
  floo previews up --app my-app --branch feat/foo --wait
  floo previews list --app my-app --json
  floo previews status --app my-app feat-foo-abcde
  floo previews logs --app my-app feat-foo-abcde --follow
  floo previews delete --app my-app feat-foo-abcde --yes

Preview sandboxes deploy remote GitHub source only. Push your branch first;
dirty local files are not uploaded."
    )]
    Previews(PreviewsSubcommands),

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

    /// Inspect edge routing and access policy for an app.
    #[command(
        subcommand,
        after_help = "\
Examples:
  floo edge routes list --app my-app
  floo edge routes list --app my-app --env prod --json"
    )]
    Edge(EdgeCommands),

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

    /// Built-in platform documentation. Run `floo docs` (no topic) for the
    /// full list of topics with one-line descriptions.
    #[command(after_help = "\
Topics: golden-path, quickstart, build, nextjs, rails, fastapi, django,
express, templates, services, edge, previews, config, cron, deploy, auth,
notifications, feedback.
Aliases: storage -> services, app-toml -> config.
Run `floo docs` (no topic) for a one-line description of each topic.")]
    Docs {
        /// Documentation topic to display. Omit for the overview.
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

    /// Number of log lines to show (max 500; higher values are capped).
    #[arg(short, long, visible_alias = "limit", default_value = "100")]
    tail: u32,

    /// Show logs since a time (e.g., 1h, 30m, 2d, or ISO timestamp).
    #[arg(short, long)]
    since: Option<String>,

    /// Filter to errors only (shorthand for --severity ERROR).
    #[arg(short, long)]
    error: bool,

    /// Exact severity level (DEFAULT, DEBUG, INFO, WARNING, ERROR, CRITICAL).
    #[arg(long)]
    severity: Option<String>,

    /// Filter logs to specific services (repeatable, or comma-separated).
    #[arg(long = "service", visible_alias = "services", value_delimiter = ',')]
    services: Vec<String>,

    /// Filter logs to a specific cron job by name.
    #[arg(long, conflicts_with = "services")]
    cron: Option<String>,

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

    /// Invite a user to the current org with a role, in one step.
    ///
    /// Sends an invite email and returns a one-time invite link. The role is
    /// assigned on acceptance, with no separate `orgs members set-role` call.
    Invite {
        /// Email address to invite.
        email: String,

        /// Role to grant: admin, member, or viewer.
        #[arg(long, default_value = "member", value_parser = ["admin", "member", "viewer"])]
        role: String,
    },

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

        /// Target specific services (repeatable, or comma-separated).
        #[arg(long = "service", visible_alias = "services", value_delimiter = ',')]
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

        /// Mark the variable write-only: floo never returns its value.
        /// Deploys still receive it. To change it, set a new value or unset it.
        /// Omitting the flag on a later set keeps an existing write-only marker.
        #[arg(long)]
        secret: bool,
    },

    /// List environment variables for an app.
    ///
    /// Values are shown masked (`********`); the value itself is never echoed
    /// here. With no `--service`, lists every service's vars plus app-level in
    /// one pass — with a `Service` column labelling each row's scope whenever the
    /// app has services (a service-less app keeps the plain Key/Value table).
    List {
        /// App name or ID (reads from config if omitted).
        #[arg(short, long)]
        app: Option<String>,

        /// Target specific services (repeatable, or comma-separated).
        #[arg(long = "service", visible_alias = "services", value_delimiter = ',')]
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

        /// Target specific services (repeatable, or comma-separated).
        #[arg(long = "service", visible_alias = "services", value_delimiter = ',')]
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
        #[arg(long = "service", visible_alias = "services")]
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

        /// Target specific services (repeatable, or comma-separated).
        #[arg(
            long = "service",
            visible_alias = "services",
            value_delimiter = ',',
            conflicts_with = "all"
        )]
        services: Vec<String>,

        /// Import env vars for all services using their configured env_file paths.
        #[arg(long)]
        all: bool,

        /// Mark every imported variable write-only: floo never returns their
        /// values. Deploys still receive them.
        #[arg(long)]
        secret: bool,

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
pub enum EdgeCommands {
    /// Inspect effective edge routes.
    #[command(subcommand)]
    Routes(EdgeRoutesCommands),

    /// Manage the app's IP/CIDR firewall (edge policy).
    #[command(subcommand)]
    Policy(EdgePolicyCommands),
}

#[derive(Subcommand)]
pub enum EdgePolicyCommands {
    /// Show the edge policy for an environment.
    #[command(after_help = "Examples:
  floo edge policy get --env prod
  floo edge policy get --app my-app --env dev --json

Shows the ordered allow/deny rules, the default action, and whether the
policy is enabled. Previews inherit the dev policy automatically.")]
    Get {
        /// App name or ID (reads from config if omitted).
        #[arg(short, long)]
        app: Option<String>,

        /// Environment: dev or prod. Previews inherit dev.
        #[arg(long, value_parser = ["dev", "prod"])]
        env: String,
    },

    /// Create or replace the edge policy for an environment.
    #[command(after_help = "Examples:
  # Office-only allowlist (everything else is denied):
  floo edge policy set --env prod --rule allow:203.0.113.0/24 --default-action deny

  # Block one abusive network, allow everyone else:
  floo edge policy set --env prod --rule deny:198.51.100.0/24 --default-action allow

  # Multiple rules — first match wins, top to bottom:
  floo edge policy set --env prod \
    --rule allow:203.0.113.7/32 --rule deny:203.0.113.0/24 --default-action allow

Rules are evaluated in the order given; the default action applies when no
rule matches. IPv4 and IPv6 CIDRs are accepted; bare IPs mean /32 (or /128).
Replaces any existing policy for the environment. Requires the Team plan.
Also configurable in floo.app.toml under [edge] — config wins on deploy.")]
    Set {
        /// App name or ID (reads from config if omitted).
        #[arg(short, long)]
        app: Option<String>,

        /// Environment: dev or prod. Previews inherit dev.
        #[arg(long, value_parser = ["dev", "prod"])]
        env: String,

        /// Ordered rule as <allow|deny>:<CIDR>. Repeatable; first match wins.
        #[arg(long = "rule", value_name = "ACTION:CIDR")]
        rule: Vec<String>,

        /// Action when no rule matches.
        #[arg(long, default_value = "allow", value_parser = ["allow", "deny"])]
        default_action: String,

        /// Store the policy without enforcing it.
        #[arg(long)]
        disabled: bool,
    },

    /// Remove the edge policy for an environment (opens traffic back up).
    #[command(after_help = "Examples:
  floo edge policy clear --env prod --yes

Deletes the policy entirely — all IPs are admitted again (subject to the
app's auth). To keep the rules but stop enforcing them, use
'floo edge policy set ... --disabled' instead.")]
    Clear {
        /// App name or ID (reads from config if omitted).
        #[arg(short, long)]
        app: Option<String>,

        /// Environment: dev or prod.
        #[arg(long, value_parser = ["dev", "prod"])]
        env: String,

        /// Skip the confirmation prompt.
        #[arg(long)]
        yes: bool,
    },
}

#[derive(Subcommand)]
pub enum EdgeRoutesCommands {
    /// List the effective route table for an app.
    #[command(after_help = "\
Examples:
  floo edge routes list --app my-app
  floo edge routes list --app my-app --env prod --json

Shows the customer-safe route table served by floo's edge: host, path,
environment, target service, access policy, source, and freshness marker.")]
    List {
        /// App name or ID (reads from config if omitted).
        #[arg(short, long)]
        app: Option<String>,

        /// Environment filter: dev, prod, or preview. Omit to list all routes.
        #[arg(long, value_parser = ["dev", "prod", "preview"])]
        env: Option<String>,
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
        #[arg(
            long = "service",
            visible_alias = "services",
            conflicts_with = "service_name"
        )]
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

        /// Route this domain to a specific service (required for multi-service apps).
        #[arg(long = "service", visible_alias = "services")]
        services: Option<String>,
    },

    /// List custom domains for an app.
    ///
    /// Custom domains are app/ingress-level, so this lists every domain on the
    /// app regardless of which service each routes to — no service target.
    List {
        /// App name or ID (uses config file if omitted).
        #[arg(short, long)]
        app: Option<String>,
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
    /// List compact deploy history for an app (no build logs).
    List {
        /// App name or ID (uses config file if omitted).
        #[arg(short, long)]
        app: Option<String>,

        /// Maximum deploy rows to return.
        #[arg(long, default_value_t = 20)]
        limit: u32,

        /// Continue from a previous next_cursor value.
        #[arg(long)]
        cursor: Option<String>,
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
pub enum PreviewsSubcommands {
    /// Create or refresh a preview sandbox from a remote GitHub branch.
    Up {
        /// App name or ID (uses config file if omitted).
        #[arg(short, long)]
        app: Option<String>,

        /// Remote GitHub branch to deploy into an isolated preview.
        #[arg(long)]
        branch: String,

        /// Wait for the returned deploy ID to reach a terminal state.
        #[arg(long)]
        wait: bool,

        /// Runtime hint for the deploy request.
        #[arg(long, default_value = "auto")]
        runtime: String,

        /// Exact remote commit SHA to deploy.
        #[arg(long)]
        commit_sha: Option<String>,

        /// Remote git ref to record on the deploy.
        #[arg(long = "ref")]
        ref_name: Option<String>,
    },

    /// List active preview sandboxes for an app.
    List {
        /// App name or ID (uses config file if omitted).
        #[arg(short, long)]
        app: Option<String>,
    },

    /// Show preview sandbox status.
    Status {
        /// Preview slug, source branch, preview URL, or unambiguous #PR.
        preview: String,

        /// App name or ID (uses config file if omitted).
        #[arg(short, long)]
        app: Option<String>,
    },

    /// Show logs for the preview's latest deploy.
    Logs {
        /// Preview slug, source branch, preview URL, or unambiguous #PR.
        preview: String,

        /// App name or ID (uses config file if omitted).
        #[arg(short, long)]
        app: Option<String>,

        /// Follow deploy logs while the preview deploy is active.
        #[arg(short, long)]
        follow: bool,

        /// Number of runtime log lines to fetch when not streaming deploy logs.
        #[arg(long, default_value_t = 100)]
        tail: u32,
    },

    /// Delete a preview sandbox and its preview-owned resources.
    Delete {
        /// Preview slug, source branch, preview URL, or unambiguous #PR.
        preview: String,

        /// App name or ID (uses config file if omitted).
        #[arg(short, long)]
        app: Option<String>,

        /// Skip the y/N prompt. Required in non-interactive contexts.
        #[arg(long, alias = "force")]
        yes: bool,
    },

    /// Inspect and reset preview managed-resource branches.
    #[command(subcommand)]
    Resources(PreviewResourcesSubcommands),
}

#[derive(Subcommand)]
pub enum PreviewResourcesSubcommands {
    /// List preview-owned Postgres, Redis, and Storage branches.
    #[command(after_help = "\
Examples:
  floo previews resources list feat-db-abcde --app my-app
  floo previews resources list feat-db-abcde --app my-app --json")]
    List {
        /// Preview slug, source branch, preview URL, or unambiguous #PR.
        preview: String,

        /// App name or ID (uses config file if omitted).
        #[arg(short, long)]
        app: Option<String>,
    },

    /// Show one preview managed-resource branch.
    #[command(after_help = "\
Examples:
  floo previews resources show feat-db-abcde --app my-app --resource redis:default
  floo previews resources show feat-db-abcde --app my-app --resource storage:uploads --json")]
    Show {
        /// Preview slug, source branch, preview URL, or unambiguous #PR.
        preview: String,

        /// App name or ID (uses config file if omitted).
        #[arg(short, long)]
        app: Option<String>,

        /// Resource key, shaped type:name, e.g. postgres:default or redis:cache.
        #[arg(long)]
        resource: String,
    },

    /// Reset one preview managed-resource branch.
    #[command(after_help = "\
Examples:
  floo previews resources reset feat-db-abcde --app my-app --resource postgres:default --yes
  floo previews resources reset feat-db-abcde --app my-app --resource redis:cache --dry-run --json

Reset is preview-scoped. Dev and prod resources are not touched. Providers that
do not yet support reset fail closed with the API's named reset blocker.")]
    Reset {
        /// Preview slug, source branch, preview URL, or unambiguous #PR.
        preview: String,

        /// App name or ID (uses config file if omitted).
        #[arg(short, long)]
        app: Option<String>,

        /// Resource key, shaped type:name, e.g. postgres:default or storage:uploads.
        #[arg(long)]
        resource: String,

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

    /// Inspect and reset preview database branches.
    #[command(subcommand)]
    Branches(DbBranchesCommands),
}

#[derive(Subcommand)]
pub enum DbBranchesCommands {
    /// List managed Postgres branches backing one preview.
    #[command(after_help = "\
Examples:
  floo db branches list feat-db-abcde --app my-app
  floo db branches list feat-db-abcde --app my-app --json

Preview database branches are preview-owned. Dev and prod databases are not
listed or reset by this surface.")]
    List {
        /// Preview slug from PR preview URLs or `floo previews` surfaces.
        preview: String,

        /// App name or ID (reads from config if omitted).
        #[arg(short, long)]
        app: Option<String>,
    },

    /// Show one preview database branch.
    #[command(after_help = "\
Examples:
  floo db branches show feat-db-abcde --app my-app
  floo db branches show feat-db-abcde --app my-app --name analytics")]
    Show {
        /// Preview slug from PR preview URLs or `floo previews` surfaces.
        preview: String,

        /// App name or ID (reads from config if omitted).
        #[arg(short, long)]
        app: Option<String>,

        /// Managed Postgres branch name.
        #[arg(long, default_value = "default")]
        name: String,
    },

    /// Reset one preview database branch.
    #[command(after_help = "\
Examples:
  floo db branches reset feat-db-abcde --app my-app --name default
  floo db branches reset feat-db-abcde --app my-app --yes --json

Reset drops and recreates preview-owned state only. It does not touch dev or
prod databases.")]
    Reset {
        /// Preview slug from PR preview URLs or `floo previews` surfaces.
        preview: String,

        /// App name or ID (reads from config if omitted).
        #[arg(short, long)]
        app: Option<String>,

        /// Managed Postgres branch name.
        #[arg(long, default_value = "default")]
        name: String,

        /// Skip the y/N prompt. Required in non-interactive contexts.
        #[arg(long)]
        yes: bool,
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
    "db branches reset",
    "previews resources reset",
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

/// Scan raw argv for the global `--json` flag, stopping at the `--`
/// end-of-options separator so passthrough args (`floo run -- cmd --json`)
/// don't count as an output-mode request. Runs before clap parsing so a parse
/// failure under `--json` can still honor the JSON output contract (#1156).
fn json_flag_in_argv(args: &[OsString]) -> bool {
    for arg in args.iter().skip(1) {
        match arg.to_str() {
            Some("--") => return false,
            Some("--json") => return true,
            _ => {}
        }
    }
    false
}

/// First actionable line of a clap parse error, minus clap's `error:` prefix.
/// clap renders multi-line output (`error: …\n\nUsage: …`); the usage block is
/// a human affordance dropped from the JSON message. Pure so it can be tested
/// without spawning a process.
fn clap_error_message(err: &clap::Error) -> String {
    let rendered = err.to_string();
    let first_line = rendered
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("invalid arguments");
    first_line
        .strip_prefix("error:")
        .unwrap_or(first_line)
        .trim()
        .to_string()
}

/// Render a clap parse failure as a JSON error and exit, so agents that always
/// pass `--json` get `{"success":false,"error":{...}}` instead of clap's
/// plain-text usage dump (#1156). Exit code matches clap's usage-error
/// convention (`err.exit_code()`, normally 2); only the rendering changes.
fn exit_with_clap_error_json(err: &clap::Error) -> ! {
    output::error(
        &clap_error_message(err),
        &crate::errors::ErrorCode::InvalidArguments,
        Some("Run the command with --help for usage."),
    );
    std::process::exit(err.exit_code());
}

pub fn run() {
    let args = rewrite_bare_version_flag(std::env::args_os().collect());

    // Detect `--json` from raw argv BEFORE clap parses. A parse failure makes
    // clap exit with plain-text usage on stderr (exit 2) *before* the parsed
    // `--json` flag is ever visible, so an agent that always passes `--json`
    // would get non-JSON on any malformed invocation — e.g. `floo doctor
    // --json` (missing subcommand) (#1156). This fixes the whole class, not
    // just `doctor`.
    let json_requested = json_flag_in_argv(&args);
    let cli = match Cli::try_parse_from(args) {
        Ok(cli) => cli,
        Err(err) => {
            // `use_stderr()` is false only for `--help`/`--version` displays —
            // explicit requests for human text that we leave to clap even
            // under `--json`. Every real parse error becomes a JSON error so
            // the `--json` contract holds.
            if json_requested && err.use_stderr() {
                // Parsing failed before the normal `set_json_mode` below ran,
                // so enable it here from the raw-argv detection — otherwise
                // `output::error` would fall back to human text on stderr.
                output::set_json_mode(true);
                exit_with_clap_error_json(&err);
            }
            err.exit();
        }
    };

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
        Commands::Analytics {
            app,
            app_flag,
            period,
        } => commands::analytics::analytics(app_flag.or(app), &period),

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
            DeploysSubcommands::List { app, limit, cursor } => {
                commands::deploys::list(app.as_deref(), limit, cursor.as_deref())
            }
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
        Commands::Previews(sub) => match sub {
            PreviewsSubcommands::Up {
                app,
                branch,
                wait,
                runtime,
                commit_sha,
                ref_name,
            } => commands::previews::up(
                app.as_deref(),
                &branch,
                wait,
                &runtime,
                commit_sha.as_deref(),
                ref_name.as_deref(),
            ),
            PreviewsSubcommands::List { app } => commands::previews::list(app.as_deref()),
            PreviewsSubcommands::Status { preview, app } => {
                commands::previews::status(app.as_deref(), &preview)
            }
            PreviewsSubcommands::Logs {
                preview,
                app,
                follow,
                tail,
            } => commands::previews::logs(app.as_deref(), &preview, follow, tail),
            PreviewsSubcommands::Delete { preview, app, yes } => {
                commands::previews::delete(app.as_deref(), &preview, yes)
            }
            PreviewsSubcommands::Resources(sub) => match sub {
                PreviewResourcesSubcommands::List { preview, app } => {
                    commands::previews::resources_list(app.as_deref(), &preview)
                }
                PreviewResourcesSubcommands::Show {
                    preview,
                    app,
                    resource,
                } => commands::previews::resources_show(app.as_deref(), &preview, &resource),
                PreviewResourcesSubcommands::Reset {
                    preview,
                    app,
                    resource,
                    yes,
                } => commands::previews::resources_reset(app.as_deref(), &preview, &resource, yes),
            },
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
            OrgsCommands::Invite { email, role } => commands::orgs::invite(&email, &role),
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
                secret,
            } => {
                let value_source = if stdin {
                    commands::env::ValueSource::Stdin
                } else if let Some(ref path) = value_file {
                    commands::env::ValueSource::File(path)
                } else {
                    commands::env::ValueSource::Inline
                };
                commands::env::set(
                    &key_value,
                    app.as_deref(),
                    &services,
                    restart,
                    &env,
                    &value_source,
                    secret,
                )
            }
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
                secret,
                env,
            } => {
                if all {
                    commands::env::import_all_services(app.as_deref(), &env, secret);
                } else {
                    commands::env::import_vars(
                        file.as_deref(),
                        app.as_deref(),
                        &services,
                        &env,
                        secret,
                    );
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

        Commands::Edge(sub) => match sub {
            EdgeCommands::Routes(routes_sub) => match routes_sub {
                EdgeRoutesCommands::List { app, env } => {
                    commands::edge::list_routes(app.as_deref(), env.as_deref())
                }
            },
            EdgeCommands::Policy(policy_sub) => match policy_sub {
                EdgePolicyCommands::Get { app, env } => {
                    commands::edge::policy_get(app.as_deref(), &env)
                }
                EdgePolicyCommands::Set {
                    app,
                    env,
                    rule,
                    default_action,
                    disabled,
                } => commands::edge::policy_set(
                    app.as_deref(),
                    &env,
                    &rule,
                    &default_action,
                    disabled,
                ),
                EdgePolicyCommands::Clear { app, env, yes } => {
                    commands::edge::policy_clear(app.as_deref(), &env, yes)
                }
            },
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
            DomainsCommands::List { app } => commands::domains::list(app.as_deref()),
            DomainsCommands::Verify { hostname, app } => {
                commands::domains::verify(&hostname, app.as_deref())
            }
            DomainsCommands::Remove { hostname, app, yes } => {
                commands::domains::remove(&hostname, app.as_deref(), yes)
            }
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
            DbCommands::Branches(sub) => match sub {
                DbBranchesCommands::List { preview, app } => {
                    commands::db::branches_list(app.as_deref(), &preview)
                }
                DbBranchesCommands::Show { preview, app, name } => {
                    commands::db::branches_show(app.as_deref(), &preview, &name)
                }
                DbBranchesCommands::Reset {
                    preview,
                    app,
                    name,
                    yes,
                } => commands::db::branches_reset(app.as_deref(), &preview, &name, yes),
            },
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
                    cron: options.cron,
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

    /// `floo docs --help` must list every topic and alias. Pinned to the
    /// `TOPICS`/`ALIASES` tables so the help block can't silently drift —
    /// before #1159 it advertised only "services, config, deploy".
    #[test]
    fn docs_help_lists_every_topic_and_alias() {
        use clap::CommandFactory;
        let mut cmd = Cli::command();
        let docs = cmd
            .find_subcommand_mut("docs")
            .expect("docs subcommand exists");
        let help = docs.render_long_help().to_string();
        for (name, _) in crate::commands::docs::TOPICS {
            assert!(
                help.contains(name),
                "`floo docs --help` is missing topic '{name}'",
            );
        }
        for (alias, _) in crate::commands::docs::ALIASES {
            assert!(
                help.contains(alias),
                "`floo docs --help` is missing alias '{alias}'",
            );
        }
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
            (
                "db branches reset",
                &[
                    "floo",
                    "db",
                    "branches",
                    "reset",
                    "feat-db-abcde",
                    "--app",
                    "myapp",
                    "--dry-run",
                ],
            ),
            (
                "previews resources reset",
                &[
                    "floo",
                    "previews",
                    "resources",
                    "reset",
                    "feat-db-abcde",
                    "--app",
                    "myapp",
                    "--resource",
                    "postgres:default",
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

    #[test]
    fn analytics_accepts_app_flag_and_positional() {
        // --app flag form, consistent with `floo logs`
        let cli = Cli::try_parse_from(["floo", "analytics", "--app", "myapp"]).unwrap();
        let Commands::Analytics { app, app_flag, .. } = cli.command else {
            panic!("expected Analytics");
        };
        assert_eq!(app_flag.or(app), Some("myapp".to_string()));

        // positional form still works (back-compat)
        let cli = Cli::try_parse_from(["floo", "analytics", "myapp"]).unwrap();
        let Commands::Analytics { app, app_flag, .. } = cli.command else {
            panic!("expected Analytics");
        };
        assert_eq!(app_flag.or(app), Some("myapp".to_string()));
    }

    #[test]
    fn analytics_app_flag_takes_precedence_over_positional() {
        let cli = Cli::try_parse_from(["floo", "analytics", "pos", "--app", "flag"]).unwrap();
        let Commands::Analytics { app, app_flag, .. } = cli.command else {
            panic!("expected Analytics");
        };
        assert_eq!(app_flag.or(app), Some("flag".to_string()));
    }

    #[test]
    fn analytics_omitted_app_is_org_level() {
        let cli = Cli::try_parse_from(["floo", "analytics"]).unwrap();
        let Commands::Analytics { app, app_flag, .. } = cli.command else {
            panic!("expected Analytics");
        };
        assert_eq!(app_flag.or(app), None);
    }

    // --- #1161: `--service` canonical, `--services` alias, CLI-wide ---
    //
    // Every service-targeting flag accepts both spellings so agents never have
    // to special-case singular vs plural per subcommand. Canonical is
    // `--service`; `--services` is a visible alias (same arg id), so existing
    // scripts keep working and `--service` is the one true name in help.

    #[test]
    fn run_accepts_both_service_spellings() {
        for flag in ["--service", "--services"] {
            let cli = Cli::try_parse_from(["floo", "run", flag, "api", "--", "echo", "hi"])
                .unwrap_or_else(|e| panic!("clap rejected `run {flag}`: {e}"));
            let Commands::Run { service, .. } = cli.command else {
                panic!("expected Run");
            };
            assert_eq!(service, "api", "run {flag}");
        }
    }

    #[test]
    fn env_get_accepts_both_service_spellings() {
        for flag in ["--service", "--services"] {
            let cli = Cli::try_parse_from(["floo", "env", "get", "KEY", flag, "api"])
                .unwrap_or_else(|e| panic!("clap rejected `env get {flag}`: {e}"));
            let Commands::Env(EnvCommands::Get { service, .. }) = cli.command else {
                panic!("expected Env::Get");
            };
            assert_eq!(service.as_deref(), Some("api"), "env get {flag}");
        }
    }

    #[test]
    fn domains_add_accepts_both_service_spellings() {
        for flag in ["--service", "--services"] {
            let cli = Cli::try_parse_from(["floo", "domains", "add", "x.com", flag, "api"])
                .unwrap_or_else(|e| panic!("clap rejected `domains add {flag}`: {e}"));
            let Commands::Domains(DomainsCommands::Add { services, .. }) = cli.command else {
                panic!("expected Domains::Add");
            };
            assert_eq!(services.as_deref(), Some("api"), "domains add {flag}");
        }
    }

    #[test]
    fn logs_accepts_both_service_spellings_and_repeats() {
        for flag in ["--service", "--services"] {
            let cli = Cli::try_parse_from(["floo", "logs", flag, "api", flag, "web"])
                .unwrap_or_else(|e| panic!("clap rejected `logs {flag}`: {e}"));
            let Commands::Logs { options, .. } = cli.command else {
                panic!("expected Logs");
            };
            assert_eq!(options.services, vec!["api", "web"], "logs {flag}");
        }
    }

    #[test]
    fn env_set_accepts_both_service_spellings_and_repeats() {
        for flag in ["--service", "--services"] {
            let cli = Cli::try_parse_from(["floo", "env", "set", "K=V", flag, "api", flag, "web"])
                .unwrap_or_else(|e| panic!("clap rejected `env set {flag}`: {e}"));
            let Commands::Env(EnvCommands::Set { services, .. }) = cli.command else {
                panic!("expected Env::Set");
            };
            assert_eq!(services, vec!["api", "web"], "env set {flag}");
        }
    }

    #[test]
    fn redeploy_accepts_both_service_spellings_and_short() {
        // `redeploy` keeps its `-s` short form alongside the renamed long flag.
        for flag in ["--service", "--services", "-s"] {
            let cli = Cli::try_parse_from(["floo", "redeploy", flag, "api"])
                .unwrap_or_else(|e| panic!("clap rejected `redeploy {flag}`: {e}"));
            let Commands::Redeploy { services, .. } = cli.command else {
                panic!("expected Redeploy");
            };
            assert_eq!(services, vec!["api"], "redeploy {flag}");
        }
    }

    #[test]
    fn mixed_service_spellings_address_one_arg() {
        // Both spellings target the same arg id, so on a repeatable flag they
        // accumulate in order rather than colliding as two distinct args.
        let cli =
            Cli::try_parse_from(["floo", "logs", "--service", "api", "--services", "web"]).unwrap();
        let Commands::Logs { options, .. } = cli.command else {
            panic!("expected Logs");
        };
        assert_eq!(options.services, vec!["api", "web"]);
    }

    // --- #192: comma-separated lists on multi-value service flags ---

    #[test]
    fn multi_value_service_flag_splits_on_comma() {
        // The repeatable Vec service flags accept comma-separated lists, and
        // the comma form composes with repetition.
        let cli = Cli::try_parse_from(["floo", "logs", "--service", "api,web"]).unwrap();
        let Commands::Logs { options, .. } = cli.command else {
            panic!("expected Logs");
        };
        assert_eq!(options.services, vec!["api", "web"]);

        // Comma and repetition accumulate together.
        let cli = Cli::try_parse_from([
            "floo",
            "logs",
            "--service",
            "api,web",
            "--services",
            "worker",
        ])
        .unwrap();
        let Commands::Logs { options, .. } = cli.command else {
            panic!("expected Logs");
        };
        assert_eq!(options.services, vec!["api", "web", "worker"]);

        // `env set` is the same shape.
        let cli =
            Cli::try_parse_from(["floo", "env", "set", "K=V", "--service", "api,worker"]).unwrap();
        let Commands::Env(EnvCommands::Set { services, .. }) = cli.command else {
            panic!("expected Env::Set");
        };
        assert_eq!(services, vec!["api", "worker"]);
    }

    #[test]
    fn single_value_service_flag_does_not_split_on_comma() {
        // The delimiter is scoped to the repeatable Vec flags; single-target
        // flags keep the value intact (a bogus name here, but unsplit).
        let cli =
            Cli::try_parse_from(["floo", "env", "get", "KEY", "--service", "api,web"]).unwrap();
        let Commands::Env(EnvCommands::Get { service, .. }) = cli.command else {
            panic!("expected Env::Get");
        };
        assert_eq!(service.as_deref(), Some("api,web"));
    }

    #[test]
    fn domains_list_and_remove_reject_service_flag() {
        // #1161: custom domains are app/ingress-level, so `list`/`remove` no
        // longer accept a service target (it was required-but-ignored before).
        // clap must reject the removed flag in either spelling.
        let invocations: &[&[&str]] = &[
            &["floo", "domains", "list", "--service", "api"],
            &["floo", "domains", "list", "--services", "api"],
            &[
                "floo",
                "domains",
                "remove",
                "x.com",
                "--service",
                "api",
                "--yes",
            ],
        ];
        for args in invocations {
            let err = parse_err(args);
            assert_eq!(
                err.kind(),
                clap::error::ErrorKind::UnknownArgument,
                "expected UnknownArgument for {args:?}",
            );
        }
    }

    #[test]
    fn orgs_invite_role_flag_parses_defaults_and_validates() {
        // #1161: `orgs invite --role` captures the role in one step; the
        // default is member; clap rejects anything outside admin/member/viewer.
        let cli =
            Cli::try_parse_from(["floo", "orgs", "invite", "a@x.com", "--role", "admin"]).unwrap();
        let Commands::Orgs(OrgsCommands::Invite { email, role }) = cli.command else {
            panic!("expected Orgs::Invite");
        };
        assert_eq!(email, "a@x.com");
        assert_eq!(role, "admin");

        let cli = Cli::try_parse_from(["floo", "orgs", "invite", "a@x.com"]).unwrap();
        let Commands::Orgs(OrgsCommands::Invite { role, .. }) = cli.command else {
            panic!("expected Orgs::Invite");
        };
        assert_eq!(role, "member", "default role");

        let err = parse_err(&["floo", "orgs", "invite", "a@x.com", "--role", "owner"]);
        assert_eq!(err.kind(), clap::error::ErrorKind::InvalidValue);
    }

    // --- `--json` arg-error contract (#1156) ---

    /// Parse `args` expecting failure, returning the clap error. Avoids
    /// `unwrap_err()`, which would require `Cli: Debug` (not derived).
    fn parse_err(args: &[&str]) -> clap::Error {
        match Cli::try_parse_from(args.iter().copied()) {
            Ok(_) => panic!("expected a clap parse error for {args:?}"),
            Err(e) => e,
        }
    }

    #[test]
    fn json_flag_detected_anywhere_before_separator() {
        assert!(json_flag_in_argv(&argv(&["floo", "--json", "doctor"])));
        assert!(json_flag_in_argv(&argv(&["floo", "doctor", "--json"])));
        assert!(!json_flag_in_argv(&argv(&["floo", "doctor"])));
        assert!(!json_flag_in_argv(&argv(&["floo"])));
    }

    #[test]
    fn json_flag_after_double_dash_does_not_count() {
        // `--json` in passthrough args is the wrapped command's flag, not a
        // request for the CLI's JSON output mode.
        assert!(!json_flag_in_argv(&argv(&[
            "floo",
            "run",
            "--service",
            "api",
            "--",
            "mycmd",
            "--json",
        ])));
        // …but a real `--json` before the separator still counts.
        assert!(json_flag_in_argv(&argv(&[
            "floo",
            "run",
            "--json",
            "--service",
            "api",
            "--",
            "mycmd",
            "--json",
        ])));
    }

    #[test]
    fn clap_error_message_strips_prefix_and_usage() {
        // The #1156 case: `floo doctor --json` (missing subcommand) yields a
        // `MissingSubcommand` error rendered as
        //   error: 'floo doctor' requires a subcommand but one was not provided
        //     [subcommands: ...]
        //   Usage: ...
        // The JSON message keeps only the first actionable line, with clap's
        // `error:` prefix and the usage/hint block dropped.
        let err = parse_err(&["floo", "doctor", "--json"]);
        let msg = clap_error_message(&err);
        assert!(
            msg.contains("requires a subcommand but one was not provided"),
            "unexpected message: {msg}",
        );
        assert!(!msg.starts_with("error:"));
        assert!(!msg.contains("Usage:"));
        assert!(!msg.contains("[subcommands:"));
    }

    #[test]
    fn clap_missing_subcommand_is_a_real_error_not_a_help_display() {
        // The discriminator the run() error path relies on: a missing
        // subcommand under `--json` is `use_stderr() == true`, so it converts
        // to JSON, whereas `--help`/`--version` are stdout displays that don't.
        assert!(parse_err(&["floo", "doctor", "--json"]).use_stderr());
        assert!(!parse_err(&["floo", "--help"]).use_stderr());
    }
}
