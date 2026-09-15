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

Follow the [web quickstart](https://getfloo.com/docs/introduction) to connect your
GitHub repository and deploy your first app. For a floo-hosted repository without
a GitHub account, follow the [managed-project guide](https://getfloo.com/agents.md).

## Find commands and documentation

```bash
floo commands --json          # command tree
floo <command> --help         # syntax for the installed CLI
floo docs --json              # documentation topics and URLs
floo docs golden-path --json  # quickstart URL
```

`floo docs` returns links to current web documentation. It does not bundle
articles or pin them to your CLI version. Command help is version-matched to
the installed binary.

For agent setup and the JSON output contract, read the
[agent guide](https://getfloo.com/docs/guides/agent-setup).

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

- Run `floo docs` for the web documentation index (`floo docs --json` for agents).
- [Getting Started](https://getfloo.com/docs/introduction)
- [CLI Reference](https://getfloo.com/docs/cli/overview)
- [Configuration Reference](https://getfloo.com/docs/reference/config-spec)

## License

MIT. See [LICENSE](LICENSE) for details.
