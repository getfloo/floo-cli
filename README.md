# floo CLI

The command-line interface for [floo](https://getfloo.com) - manage and observe web apps. Deploys are git-driven: push to `main` and floo builds and deploys it. This CLI handles everything else.

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

Updates retain bundled public CAs and also use native certificate trust.
`SSL_CERT_FILE` selects a PEM CA bundle instead of native store discovery.
If an older CLI cannot update behind your proxy, rerun the curl installer above
with your organization’s trusted CA configured for curl. The installer verifies
both the release checksum and signature.

## Quick start

From a local git repository with a `floo.app.toml` (create one with `floo init <app-name>`):

```bash
floo deploy
```

floo asks the API where the code deploys from, then creates the app named
after `[app] name`, pushes the current commit, and waits until the dev URL is
live. A folder whose remote is on github.com deploys from that repository.
With no remote, pick `--github` (a private repo in your account, made with
`gh`) or `--hosted` (floo keeps a copy). Any branch other than the default
gets a preview URL. Every stop creates nothing and prints the fix. Commit your
changes first; `floo deploy --preflight` shows the plan. A hosted app moves to
your own GitHub repo, history included, with `floo connect owner/repo`.

For the HTTP-only path, read the [agent guide](https://getfloo.com/agents.md).

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

## Claude Code plugin

The floo plugin gives Claude Code the floo skill and the floo MCP server
(`https://api.getfloo.com/mcp`, signed in with OAuth) in one install. Run this
in Claude Code:

```
/plugin install floo --marketplace getfloo/floo-cli
```

On Claude Code older than 2.1.275, add the marketplace first:

```
/plugin marketplace add getfloo/floo-cli
/plugin install floo@floo
```

The plugin lives in [`plugin/`](plugin). [Privacy policy](https://getfloo.com/legal/privacy).

## Cursor and Grok Bot plugin

The same plugin works in Cursor and Grok Bot, which install plugins from the
Cursor Marketplace. Install **floo** from the
[Cursor Marketplace](https://cursor.com/marketplace), then connect the `floo`
MCP server and approve the sign-in in your browser. In Grok Bot, open
**Connect apps** in the sidebar.

To try it from a local checkout, copy `plugin/` to
`~/.cursor/plugins/local/floo` and run **Developer: Reload Window**. The
Cursor manifest is [`plugin/.cursor-plugin/plugin.json`](plugin/.cursor-plugin/plugin.json).

## Building from source

Use the current stable [Rust toolchain](https://rustup.rs/), matching CI.

```bash
git clone https://github.com/getfloo/floo-cli.git
cd floo-cli
cargo build --bin floo-local
# Binary at target/debug/floo-local
```

## Local development vs installed CLI

The package builds `floo`, `floo-local`, and `floo-dev`. Use `floo-local` for
local CLI development: it stores credentials in `~/.floo-local/`, separate from
installed `floo` (`~/.floo/`). Both default to the production API. `floo-dev`
uses the dev API and `~/.floo-dev/`.

```bash
cd floo-cli
cargo build --bin floo-local
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

### Guides

- [Host an internal tool or admin dashboard built with AI](https://getfloo.com/docs/deploy/internal-tool)
- [Deploy a customer-facing app built with AI](https://getfloo.com/docs/deploy/customer-app)
- [Deploy an app without a GitHub repository](https://getfloo.com/docs/deploy/without-github)
- [Build an app from Claude or ChatGPT](https://getfloo.com/docs/guides/build-in-chat)
- [A safe backend for an app built with AI](https://getfloo.com/docs/guides/backend)
- [App sign-in](https://getfloo.com/docs/guides/app-auth)
- [Move from Vercel to floo](https://getfloo.com/docs/migrate/from-vercel)
- [Move from Heroku to floo](https://getfloo.com/docs/migrate/from-heroku)
- [Leave floo anytime](https://getfloo.com/docs/migrate/leave-floo)

### From the blog

- [The best platforms to host an internal tool built with Claude Code](https://getfloo.com/blog/best-platform-to-host-internal-tools-built-with-ai)
- [How to build and deploy an internal tool with AI, step by step](https://getfloo.com/blog/how-to-build-and-deploy-an-internal-tool-with-ai)
- [Where should I host an app I built with Claude Code?](https://getfloo.com/blog/where-to-host-an-app-built-with-claude-code)
- [How to add a backend to an app your agent built](https://getfloo.com/blog/how-to-add-a-backend-to-an-app-your-agent-built)
- [Your app is a container: take it anywhere](https://getfloo.com/blog/your-app-is-a-container-take-it-anywhere)

## License

MIT. See [LICENSE](LICENSE) for details.
