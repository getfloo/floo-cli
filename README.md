# Floo CLI

The command-line interface for [Floo](https://getfloo.com) — deploy, manage, and observe web apps.

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
# Show installed version
floo version

# Update to latest release
floo update

# Update to a specific release tag
floo update --version v0.1.0
```

## Quick start

```bash
# Authenticate
floo login

# Deploy your project
cd my-project
floo deploy

# Manage apps
floo apps list
floo apps status my-app
floo apps delete my-app

# Environment variables
floo env set DATABASE_URL=postgres://... --app my-app
floo env list --app my-app

# Custom domains
floo domains add app.example.com --app my-app
floo domains list --app my-app
```

All commands are invoked with the production alias: `floo`.

## Agent / programmatic use

Every command supports `--json` for structured output:

```bash
# JSON to stdout, human output to stderr
floo deploy --json 2>/dev/null | jq '.data.deploy.url'

# Success: {"success": true, "data": {...}}
# Error:   {"success": false, "error": {"code": "...", "message": "...", "suggestion": "..."}}
```

## Building from source

Requires [Rust](https://rustup.rs/) (1.70+).

```bash
git clone https://github.com/getfloo/floo-cli.git
cd floo-cli
cargo build --release
# Binary at target/release/floo
```

## Local development vs installed CLI

Keep your installed production CLI on `floo`, and use the dev wrapper for local builds:

```bash
cd floo-cli
cargo build
./scripts/floo-dev --help
```

`scripts/floo-dev` runs `target/debug/floo` (or `FLOO_DEV_BIN` if set) so local development does not replace your installed `floo` binary.

## Contributing

Contributions are welcome! Please:

1. Fork the repository
2. Create a feature branch (`git checkout -b feat/my-feature`)
3. Run tests and lint before committing:
   ```bash
   cargo test
   cargo clippy -- -D warnings
   cargo fmt --check
   ```
4. Use [Conventional Commits](https://www.conventionalcommits.org/) (`feat:`, `fix:`, `docs:`, etc.)
5. Open a pull request

## Documentation

- [Getting Started](https://docs.getfloo.com)
- [CLI Reference](https://docs.getfloo.com/cli)
- [API Reference](https://docs.getfloo.com/api)

## License

MIT. See [LICENSE](LICENSE) for details.
