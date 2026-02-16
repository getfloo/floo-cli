# Floo CLI

The command-line interface for [Floo](https://getfloo.com) — deploy, manage, and observe web apps.

## Install

```bash
curl -fsSL https://getfloo.com/install.sh | sh
```

Or download a binary directly from [Releases](https://github.com/getfloo/floo-cli/releases).

### Supported platforms

| Platform | Architecture | Binary |
|----------|-------------|--------|
| macOS | Intel (x86_64) | `floo-x86_64-apple-darwin` |
| macOS | Apple Silicon (arm64) | `floo-aarch64-apple-darwin` |
| Linux | x86_64 | `floo-x86_64-unknown-linux-musl` |
| Linux | arm64 | `floo-aarch64-unknown-linux-musl` |
| Windows | x86_64 | `floo-x86_64-pc-windows-msvc.exe` |

**Windows:** Download `floo-x86_64-pc-windows-msvc.exe` from [Releases](https://github.com/getfloo/floo-cli/releases) and add it to your PATH.

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
