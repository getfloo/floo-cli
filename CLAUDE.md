# CLAUDE.md — Agent Instructions for getfloo/floo-cli

## What This Repo Is

This is the **open-source Rust CLI** for [Floo](https://getfloo.com) — deploy, manage, and observe web apps from the terminal. The CLI is a thin HTTP client that calls the Floo API. Licensed under MIT.

## Development Commands

```bash
# Build
cargo build                      # Debug build
cargo build --release            # Release build (~2MB static binary)

# Test
cargo test                       # All tests (unit + integration)

# Lint & format
cargo clippy -- -D warnings      # Lint (deny all warnings)
cargo fmt --check                # Check formatting
cargo fmt                        # Auto-format

# Run locally
cargo run -- --help              # Show help
cargo run -- deploy              # Run deploy command
cargo run -- apps list --json    # JSON mode
```

## Architecture

### Output Contract (`output.rs`) — CRITICAL

Dual-mode output pattern:
- **Colored output** (spinners, tables, progress) → **stderr**
- **JSON output** → **stdout**

This makes `floo deploy --json 2>/dev/null | jq` work for agents.

`JSON_MODE` is an `AtomicBool` set once at startup. Every output function checks it.

**Pitfall:** `info(msg)` emits `{"success": true, "data": null}` to stdout in JSON mode. If a command later calls `success()` with real data, stdout has two JSON objects — breaking agents. **Fix:** Guard with `if !is_json_mode() { info("..."); }`.

### API Client (`api_client.rs`)

All HTTP calls go through `FlooClient`. Never use `reqwest` directly in commands. Auth header injected from config. Base URL from config, overridable via `FLOO_API_URL` env var.

### Config (`config.rs`)

Manages `~/.floo/config.json` (API key, email, API URL). File permissions set to `0o600`.

### Detection (`detection.rs`)

Auto-detects runtime/framework from project files. Priority: Dockerfile > package.json > pyproject.toml/requirements.txt > go.mod > index.html.

### Archive (`archive.rs`)

Packs source into `.tar.gz`, respects `.flooignore`. 500MB size limit.

### Errors (`errors.rs`)

`FlooError` and `FlooApiError` with thiserror derive. Use `?` operator, never `unwrap()` in production paths.

## Key Conventions

- Rust 2021 edition, **cargo** for build/deps, **clap** derive for CLI
- Lint: **clippy** (`-D warnings`). Format: **cargo fmt**. Test: **cargo test**
- No `println!` — use `output` module functions
- No `unwrap()` in production paths — use `?` operator
- No `unsafe` without documented justification
- All HTTP calls via `FlooClient`, never direct `reqwest`
- No hardcoded API URLs — use config or `FLOO_API_URL` env var
- Unit tests inline (`#[cfg(test)] mod tests`), integration tests in `tests/`
- Reset `output::set_json_mode(false)` and `output::set_dry_run_mode(false)` at the start of every test (global state leaks)
- Issue tracker: CLI issues live in `getfloo/floo-cli` (this repo). API/infra issues live in `getfloo/floo`.
- PR closure language is mandatory for issue-driven work:
  - CLI issues: `Closes #N` (same-repo reference)
  - Cross-repo issues: `Closes getfloo/floo#N`

## Agent Skill Maintenance

The skill file (`skills/floo.md`) is a tiny intro (~30 lines). Platform knowledge lives in
`floo docs` (`src/commands/docs.rs`). Command metadata lives in `floo commands`
(`src/commands/command_tree.rs`). When adding new commands, update `command_tree.rs` and add
`after_help` examples in `cli.rs`. Only update `skills/floo.md` if the getting-started flow changes.

## Release Flow

1. Tag `v*` on main branch
2. CI builds binaries for 5 targets (macOS x86/arm, Linux x86/arm, Windows x86)
3. GitHub Release created with binaries + SHA256 checksums
4. Install script downloads from these releases
