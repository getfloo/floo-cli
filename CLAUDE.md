# CLAUDE.md — Agent Instructions for getfloo/floo-cli

## Project Context

This is the open-source CLI for Floo, the AI-native infrastructure platform.
Users install this via `curl -fsSL https://getfloo.com/install.sh | sh` or `pip install floo-cli`.
The CLI is the primary interface — it must work perfectly for both humans and AI agents.

## Development Commands

```bash
uv sync                   # Install dependencies
uv run floo --help        # Run CLI locally
uv run pytest             # Run all tests
uv run pytest tests/test_detection.py::test_detect_nodejs_nextjs  # Single test
uv run ruff check .       # Lint
uv run ty check           # Type check
```

## Architecture

- `floo/cli.py` — Main Typer app, registers all command groups, handles `--json` global flag
- `floo/commands/` — One file per command group (auth, deploy, logs, env, etc.)
- `floo/api_client.py` — All HTTP communication with the Floo API. Synchronous httpx. Never make HTTP calls directly in commands
- `floo/config.py` — Manages `~/.floo/config.json` (API key, email, API URL). File permissions 0o600. `FLOO_API_URL` env var overrides stored URL
- `floo/detection.py` — Auto-detect project runtime and framework. Priority: Dockerfile > package.json > pyproject.toml/requirements.txt > go.mod > index.html. Returns `DetectionResult` with confidence level
- `floo/archive.py` — Pack source into `.tar.gz`, respects `.flooignore` (gitignore syntax). Built-in ignores: `.git`, `node_modules`, `__pycache__`, `.venv`, `.env`, etc. 500MB size limit
- `floo/output.py` — Dual-mode output: Rich to stderr (humans), JSON to stdout (agents)
- `floo/errors.py` — `FlooError(code, message, suggestion)` base class. `FlooAPIError` adds `status_code`
- `floo/names.py` — Random app name generator (adjective-noun pairs like `swift-brook`)
- `floo/constants.py` — API URLs, defaults

## Critical Design Principles

1. **Every command MUST support `--json` flag.** This is non-negotiable. AI agents depend on it.
2. **Every command MUST have clear error messages.** Not stack traces — actionable messages.
3. **Output is part of the product.** Use Rich for beautiful terminal output. Spinners during waits, tables for lists, colors for status.
4. **Zero config by default.** Detect everything automatically. Only ask the user if truly ambiguous.
5. **Fail fast and loud.** If something is wrong, tell the user immediately with the fix.

## Output Architecture

The `output.py` module enforces a critical design pattern:
- **Rich output (spinners, colors, tables) goes to stderr**
- **JSON output goes to stdout**

This means `floo deploy --json 2>/dev/null | jq` works perfectly for agents.

### --json Output Contract

When `--json` is passed, commands MUST:
- Output ONLY valid JSON to stdout (no spinners, no colors, no extra text)
- Use consistent schema: `{ "success": true, "data": ... }` or `{ "success": false, "error": { "code": "...", "message": "...", "suggestion": "..." } }`
- Exit code 0 on success, non-zero on failure
- Never mix human-readable output with JSON

## Error Handling

All errors use structured types:

```python
class FlooError(Exception):
    def __init__(self, code: str, message: str, suggestion: str | None = None):
        self.code = code
        self.message = message
        self.suggestion = suggestion
```

Display format:
```
Error: Build failed — no package.json or requirements.txt found
  → Add a package.json, requirements.txt, or Dockerfile to your project
```

JSON format:
```json
{
  "success": false,
  "error": {
    "code": "NO_RUNTIME_DETECTED",
    "message": "Build failed — no package.json or requirements.txt found",
    "suggestion": "Add a package.json, requirements.txt, or Dockerfile to your project"
  }
}
```

## Coding Standards

- Python 3.12+, **uv** for package management, **hatchling** build backend
- Type hints on ALL function signatures
- Google-style docstrings on all public functions
- Lint: **ruff** (line-length 100). Type check: **ty**. Test: **pytest**
- No `print()` — use `output` module for all user-facing output
- All API calls via `api_client.py` — never make HTTP calls directly in commands
- Conventional commits: `feat:`, `fix:`, `docs:`, `chore:`, `test:`, `refactor:`
- Branches: `feat/short-description`, `fix/short-description`
- No commented-out code, no TODOs in code (create GitHub issues instead)
- Mock external services in tests, never hit real APIs

## Testing

- Test file naming: `test_*.py` in `tests/`
- Use `typer.testing.CliRunner` for CLI integration tests
- Test both human output and `--json` output for every command
- Use test fixtures in `tests/fixtures/` for runtime detection tests
- Mock API calls — never hit real API in tests

## Security Reminders

- NEVER log or display API keys (mask in `floo whoami` output)
- Store API key in `~/.floo/config.json` with 600 permissions
- Validate SSL certificates on all API calls
- Don't include `.env` files in source tarballs by default
