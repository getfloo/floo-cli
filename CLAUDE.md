# CLAUDE.md — Agent Instructions for getfloo/floo-cli

## What This Repo Is

This is the **open-source Rust CLI** for [floo](https://getfloo.com). It deploys, manages, and observes web apps from the terminal. The CLI is a thin HTTP client that calls the floo API. Licensed under MIT.

## Development Commands

```bash
# Build
cargo build                      # Debug build
cargo build --release            # Optimized release binaries

# Test
./scripts/test                   # Canonical: fmt --check + clippy + all tests,
                                 # with the corrupt-harness guard (#204)
cargo test                       # All tests only (unit + integration)

# Lint & format
cargo clippy -- -D warnings      # Lint (deny all warnings)
cargo fmt --check                # Check formatting
cargo fmt                        # Auto-format

# Run locally
cargo run --bin floo-local -- --help                 # Show help
cargo run --bin floo-local -- docs quickstart --json # Web quickstart URL
cargo run --bin floo-local -- apps list --json       # List apps (authenticated)
```

## Architecture

### Output Contract (`output.rs`) — CRITICAL

Dual-mode output pattern:
- **Colored output** (spinners, tables, progress) → **stderr**
- **JSON output** → **stdout**

This makes `floo deploys list --json 2>/dev/null | jq` work for agents.

`JSON_MODE` is an `AtomicBool` set once at startup. Every output function checks it.

**Pitfall:** `info(msg)` emits `{"success": true, "data": null}` to stdout in JSON mode. If a command later calls `success()` with real data, stdout has two JSON objects — breaking agents. **Fix:** Guard with `if !is_json_mode() { info("..."); }`.

### Secret redaction (`redact.rs`) — load-bearing for agent safety

Agents pipe `--json` stdout into transcripts and logs by default. Every payload that hits `print_json` runs through `redact::process_in_place` first; secret-shaped values are replaced with `***REDACTED***` and the top-level object is stamped with `"contains_secrets": true` so harnesses can refuse the payload before it lands anywhere persistent. The contract is enforced **at the boundary**, not at each call site — callers do not need to remember to redact.

Detection has three layers, applied in order:

1. **Field name** — lowercase JSON keys matching `SECRET_FIELD_NAMES` (`password`, `api_key`, `token`, `database_url`, `generated_password`, …).
2. **Env-var-shaped key** — UPPER_SNAKE_CASE keys containing a token from `ENV_VAR_SECRET_TOKENS` (`PASSWORD`, `SECRET`, `KEY`, `TOKEN`, …) and not on `ENV_VAR_ALLOWLIST` (`PORT`, `PUBLIC_KEY`, `AWS_REGION`, …). Catches `services.web.DATABASE_URL` shapes inside arbitrary maps.
3. **Value content** — strings whose body matches a credential pattern (URI userinfo, `floo_…` keys, AWS access keys, bearer tokens, JWTs).

The global `--reveal-secrets` flag opts back in to plaintext. The `contains_secrets` marker still fires under reveal so harnesses retain detect-and-refuse capability even when the user explicitly opted in.

When designing new commands:

- **Don't** hand-roll scrubbing in command modules — the redactor is the single source of truth.
- **Don't** invent a new envelope shape for env-var data; reuse the `EnvVar { key, value }` pair so the env-var-pair detector catches it.
- **Do** add a snapshot test in `redact::snapshots::*` for any new command that surfaces credential-shaped data. Tests embed forbidden substrings and assert they don't survive — `kitchen_sink_no_forbidden_substring_survives` is the pattern to follow.
- **Mirror the API redactor.** When adding patterns, also update `api/app/services/logs.py` (`_SECRET_KEY_PATTERN` + the URI/floo/AWS regex set) so server-side log scrubbing stays aligned.

### API Client (`api_client.rs`)

All HTTP calls go through `FlooClient`. Never use `reqwest` directly in commands. Auth header injected from config. Base URL from config, overridable via `FLOO_API_URL` env var.

### Config (`config.rs`)

The binary basename selects credentials and the default API: `floo-local` uses
`~/.floo-local/` and the production API, `floo` uses `~/.floo/` and the production
API, and `floo-dev` uses `~/.floo-dev/` and the dev API. A local build does not
select a local API automatically. Config file permissions are `0o600`.

### Detection (`detection.rs`)

Auto-detects runtime/framework from project files. Priority: Dockerfile > package.json > pyproject.toml/requirements.txt > go.mod > index.html.

### Errors (`errors.rs`)

`FlooError` and `FlooApiError` with thiserror derive. Use `?` operator, never `unwrap()` in production paths.

## Key Conventions

- Rust 2021 edition, **cargo** for build/deps, **clap** derive for CLI
- Always write the product name as `floo` in user-visible text.
- Lint: **clippy** (`-D warnings`). Format: **cargo fmt**. Test: **`./scripts/test`** (canonical entry point; wraps fmt + clippy + `cargo test` and self-heals the #204 corrupt-harness artifact class)
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

## Documentation ownership

The installed binary owns command syntax and documentation discovery:

- Clap definitions and `after_help` examples in `src/cli.rs` own exact syntax and immediate command recovery.
- `src/commands/command_tree.rs` owns the machine-readable command catalog.
- The typed registry in `src/commands/docs.rs` owns topic names, aliases, summaries, URLs, and JSON discovery.
- https://getfloo.com/docs owns platform guidance, onboarding, examples, and troubleshooting. `floo docs` returns links to those current pages; articles are neither bundled nor pinned to the installed CLI version.
- `plugin/skills/**/SKILL.md` route agents to command help and web docs while preserving durable safety policy. The CLI embeds these same files for installation and refresh.
- README owns installation and contributor instructions and links to the web quickstart.

When behavior changes, update its canonical web documentation and any affected
Clap help or topic routing. Do not copy long-form guidance into README, skills,
or Rust strings. The guidance validator checks first-party command examples
against the live Clap tree.

## Release Flow

1. Tag `v*` on main branch
2. CI builds binaries for 5 targets (macOS x86/arm, Linux x86/arm, Windows x86)
3. GitHub Release created with binaries + SHA256 checksums + RSA signatures
4. Slack `#releases` ping fires (mirrors the platform's deploy.yml notification — gated on `secrets.SLACK_RELEASES_WEBHOOK`; skips with a warning if unset, fails the release if Slack rejects the post)
5. Install script downloads from these releases and verifies checksum + signature before install

### Required secrets

- `FLOO_RELEASE_SIGNING_KEY` — RSA private key whose public key is pinned into the CLI updater. Required to publish a signed release; the workflow fails fast if missing or mismatched.
- `SLACK_RELEASES_WEBHOOK` — Slack incoming-webhook URL for the `#releases` channel. Optional; the notify step warns and skips when empty so a fork that cuts its own tag still gets a successful release.

Both must be set at the org or repo level. **This is a public repo — never reference secret values in workflow files except via `secrets.NAME`, never echo them in `run:` blocks, and never expose them to fork PRs (the workflow's `push: tags` trigger already gates that — fork PRs cannot push tags to upstream).**
