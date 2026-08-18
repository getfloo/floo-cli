floo Deploy Flow

## How Deploys Work

  All source comes from GitHub. The CLI never uploads code.

  1. **Validate** - run floo preflight against the config you will commit
  2. **Push** - a push or merge to the connected branch triggers dev
  3. **Pull source** - the API downloads that GitHub commit
  4. **Build** - Cloud Build builds the service images
  5. **Migrate** - migrate_command runs when the lane requires it
  6. **Route** - the platform moves traffic only after its deploy gates pass
  7. **Promote** - a GitHub release promotes the proved artifact to prod

## Deploy Flow

  1. Push to GitHub:     git push origin main
  2. Watch the deploy:   floo deploys watch --app <name>
  3. Done when you see:  ✓ Deployed to https://...

  The push triggers a deploy automatically via GitHub webhook.

  To follow one pushed commit, pass its full or abbreviated SHA:

    floo deploys watch --app <name> --commit <sha>

  If rapid pushes arrive while another deploy is active, floo may coalesce an
  intermediate commit into the later authoritative deploy. Watch follows that
  recorded effective deploy and emits `commit_coalesced` in JSON mode before
  the ordinary deploy lifecycle events. An exact deploy for the requested SHA
  always takes precedence.

## Force Redeploy

  Use `floo redeploy` only when you need to apply an operational change
  without a new commit, such as updated environment values. It resolves
  source from the connected GitHub repository and never uploads local files:

    floo env set API_KEY=new-value --app myapp --service api
    floo redeploy --app myapp

  A no-rebuild redeploy replays only the immutable contract captured by the
  current LIVE deploy. If that historical contract is unavailable or invalid,
  floo fails closed and returns one exact recovery command:

    floo redeploy --app myapp --rebuild

  `--rebuild` resolves the connected primary repository's current default-
  branch HEAD, downloads that exact commit from GitHub, reparses
  `floo.app.toml`, and persists fresh immutable topology, environment,
  resource, and cron contracts. It does not upload local files or require an
  artificial source commit. JSON errors preserve `recovery_action`,
  `recovery_command`, `recovery_app_id`, and `recovery_source`.

## First Deploy

  Use `floo apps github connect owner/repo`. This connects GitHub and
  triggers the first deploy in one step. The app is auto-created if
  it doesn't exist.

## Do I Need a Dockerfile?

  Yes - every service deploys from a Dockerfile. floo does not deploy
  without one.

  You usually do not have to write it yourself. `floo init` detects your
  runtime (Node.js, Python, Go, static) and generates a working Dockerfile
  that you commit alongside your code. Agents run `floo init --json` to
  see what was detected and generated.

  Write your own Dockerfile when you need a custom build (multi-stage,
  system packages, non-standard entrypoints). If a Dockerfile already
  exists, `floo init` leaves it alone.

## Runtime Detection (at `floo init`)

  `floo init` inspects the project directory to generate a Dockerfile:

  Dockerfile       - already present, init leaves it untouched
  package.json     - Node.js (detects Express, Next.js, etc.)
  pyproject.toml   - Python (detects Django, Flask, FastAPI)
  requirements.txt - Python (fallback)
  go.mod           - Go
  index.html       - Static site (lowest priority)

  If detection is low-confidence, init prompts you (or in `--json` mode,
  suggests adding a Dockerfile manually). At deploy time, the API requires
  a Dockerfile in the service path - missing-Dockerfile deploys fail fast.

## Preflight Validation

  floo preflight                   - validate config, detect runtimes, check readiness
  floo preflight --json            - structured output for agents
  floo preflight --env prod --json - resolve tier-aware production runtime defaults

  Preflight FAILS (exit 1, valid=false) on configs that can't build or run:
  a service path that doesn't exist, a [cron.*] with an invalid schedule, a
  cron job whose service doesn't exist (multi-service apps). It WARNS (exit 0,
  but not a clean green) on things it can't fully verify locally: a
  migrate_command with no reachable database, a required env var not injected
  or present in a local env file. Server-side `floo env set` vars and external
  databases are invisible to local preflight, which is why those warn.

  JSON shape:
    data.valid           - false iff any finding has severity "error"
    data.findings[]      - every advisory, typed: {severity (error|warning|
                           info), code, message, path?, hint?}. Filter by
                           severity/code instead of screen-scraping prose.
    data.env_injection_plan - per-service managed attachments, generated env
                           keys (DATABASE_URL + PG*, REDIS_URL, STORAGE_*),
                           required/optional keys, explicit vs implicit mode.
    data.cron[]          - declared [cron.*] entries (name, schedule, command,
                           service, timeout).
    data.runtime_plan[]  - local configured/default scaling intent. A production
                           omission remains unresolved without the app plan.
    data.plan.runtime_services[] - authenticated server resolution for --env,
                           including effective values, sources, and billing notes.
    contains_secrets     - top-level marker, true when a secret-shaped var is
                           found in a web service's env file (it may ship to
                           the browser). Harnesses can refuse the payload.

## Redeploy Options

  floo redeploy --app <name>       - redeploy with fresh env vars (no rebuild)
  floo redeploy --app <name> --rebuild  - rebuild exact current GitHub default-branch HEAD
  floo redeploy [path]             - resolve app/config from this directory; source stays on GitHub
  floo redeploy --service <name>  - redeploy specific services only
  floo redeploy --sync-env         - re-sync env vars from env_file before redeploying
  floo redeploy --rebuild --skip-migrations  - hotfix path: bypass MIGRATE step

## Deploy History

  floo deploys list --app <name>    - list past deploys without build logs
  floo deploys logs <id> --app <n>  - build logs for a specific deploy
  floo deploys watch --app <name>   - stream deploy progress in real-time
  floo deploys watch --app <name> --commit <sha>  - follow exact or coalesced commit lineage
  floo deploys rollback <app> <id>  - rollback to a previous deploy
  floo releases rollback --app <name> --to <id>  - same, alias under releases
