# floo CLI

The command-line interface for [floo](https://getfloo.com) - manage and observe web apps. Deploys are git-driven (push to `main` for dev, cut a GitHub release for prod); this CLI handles everything else.

## Install

```bash
curl -fsSL https://getfloo.com/install.sh | bash
```

Or download a binary directly from [Releases](https://github.com/getfloo/floo-cli/releases).

### Installer options

```bash
# Install a specific release tag
curl -fsSL https://getfloo.com/install.sh | FLOO_INSTALL_VERSION=v0.1.0 bash

# Install to a custom directory
curl -fsSL https://getfloo.com/install.sh | FLOO_INSTALL_DIR="$HOME/.local/bin" bash
```

### Supported platforms

| Platform | Architecture | Binary |
|----------|-------------|--------|
| macOS | Intel (x86_64) | `floo-x86_64-apple-darwin` |
| macOS | Apple Silicon (arm64) | `floo-aarch64-apple-darwin` |
| Linux | x86_64 | `floo-x86_64-unknown-linux-musl` |
| Linux | arm64 | `floo-aarch64-unknown-linux-musl` |
| Windows | x86_64 | `floo-x86_64-pc-windows-msvc.exe` |

**Windows:** Download `floo-x86_64-pc-windows-msvc.exe` from [Releases](https://github.com/getfloo/floo-cli/releases) and add it to your PATH.

## Updating

```bash
# Check for updates and show the installed version
floo version

# Update to latest release
floo update

# Update to a specific release tag
floo update --version v0.1.0
```

## Quick start

```bash
# Authenticate
floo auth login

# Initialize, validate, and push the config GitHub will deploy
cd my-project
floo init my-app
floo preflight
git add floo.app.toml Dockerfile AGENTS.md
git commit -m "chore: configure floo"
git push origin main

# Create the floo app, connect GitHub, and deploy the pushed commit
floo apps github connect owner/repo

# Manage apps
floo apps list
floo apps show my-app
floo apps delete my-app

# Environment variables
floo env set DATABASE_URL --stdin --secret --app my-app
floo env list --app my-app

# Custom domains
floo domains add app.example.com --app my-app
floo domains list --app my-app

# Edge routes
floo edge routes list --app my-app --json

# Edge policy (IP/CIDR firewall, Team plan) - configured in floo.app.toml [edge], read via CLI
floo edge policy get --env prod
floo edge policy check 203.0.113.7 --env prod
```

All commands are invoked with the production alias: `floo`.

### Managed projects without a GitHub account

With a floo API key, create a managed app with hosted, invite-only login and
postgres, then clone its source into your workspace:

```bash
floo projects create client-portal
floo projects list
floo projects clone client-portal
cd client-portal
# Read AGENTS.md before editing, then commit and push with git.
floo deploys watch --app client-portal
```

Create starts the first deploy and prints the watch command without waiting.
Clone accepts a project name or app ID and an optional destination directory.
It configures git only in the cloned repository, using the absolute path of the
current floo binary. Keep that binary in place for later fetches and pushes.
The helper fetches one-hour tokens per git operation through floo; no GitHub
account or stored GitHub credential is needed. Only the floo API key is stored.
Inherited credential helpers are reset for the repository to prevent caching.

## Agent / programmatic use

User-facing commands support `--json` for structured output:

```bash
# JSON to stdout, human output to stderr
floo redeploy --json 2>/dev/null | jq '.data.deploy.url'

# Success: {"success": true, "data": {...}}
# Error:   {"success": false, "error": {"code": "...", "message": "...", "suggestion": "..."}}
```

The internal `floo projects git-credential get` command is used by git and emits
only git's credential protocol, even with `--json`. Its `store` and `erase`
operations are no-ops. Never capture its password output in logs or files.

## Building from source

Requires [Rust](https://rustup.rs/) (1.70+).

```bash
git clone https://github.com/getfloo/floo-cli.git
cd floo-cli
cargo build
# Binary at target/debug/floo-local
```

## Local development vs installed CLI

The binary compiles as `floo-local` and uses `~/.floo-local/` for config, keeping it completely isolated from the installed `floo` binary (`~/.floo/`):

```bash
cd floo-cli
cargo build
./target/debug/floo-local --help
# Or symlink: ln -sf $(pwd)/target/debug/floo-local /usr/local/bin/floo-local
```

## Contributing

Contributions are welcome. Please:

1. Fork the repository
2. Install the git hooks once (a pre-push gate runs `./scripts/test` on Rust changes):
   ```bash
   ./scripts/install-hooks.sh
   ```
3. Create a feature branch (`git checkout -b feat/my-feature`)
4. Run tests and lint before committing:
   ```bash
   ./scripts/test
   ```
5. Use [Conventional Commits](https://www.conventionalcommits.org/) (`feat:`, `fix:`, `docs:`, etc.)
6. Open a pull request

The pre-push hook blocks a push whose Rust changes fail `./scripts/test`; bypass an individual push with `git push --no-verify` when you're deferring to CI.

## Documentation

- Run `floo docs` for the version-matched offline topic index (`floo docs --json` for agents).
- [Getting Started](https://getfloo.com/docs/introduction)
- [CLI Reference](https://getfloo.com/docs/cli/overview)
- [Configuration Reference](https://getfloo.com/docs/reference/config-spec)

## License

MIT. See [LICENSE](LICENSE) for details.
