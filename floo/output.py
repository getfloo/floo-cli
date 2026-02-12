"""Dual-mode output: Rich to stderr (humans), JSON to stdout (agents)."""

from __future__ import annotations

import json
import sys
from contextlib import contextmanager
from typing import Any

from rich.console import Console
from rich.table import Table

_json_mode = False
_console = Console(stderr=True)


def set_json_mode(enabled: bool = True) -> None:
    """Enable or disable JSON output mode."""
    global _json_mode  # noqa: PLW0603
    _json_mode = enabled


def is_json_mode() -> bool:
    """Check if JSON output mode is active."""
    return _json_mode


def _print_json(data: dict[str, Any]) -> None:
    """Print JSON to stdout."""
    print(json.dumps(data, default=str))


def success(message: str, data: Any = None) -> None:
    """Output a success result."""
    if _json_mode:
        _print_json({"success": True, "data": data})
    else:
        _console.print(f"[green]✓[/green] {message}")


def error(message: str, code: str = "ERROR", suggestion: str | None = None) -> None:
    """Output an error result."""
    if _json_mode:
        err: dict[str, Any] = {"code": code, "message": message}
        if suggestion:
            err["suggestion"] = suggestion
        _print_json({"success": False, "error": err})
    else:
        _console.print(f"[red]Error:[/red] {message}")
        if suggestion:
            _console.print(f"  → {suggestion}")


def info(message: str, data: Any = None) -> None:
    """Output an informational message."""
    if _json_mode:
        _print_json({"success": True, "data": data})
    else:
        _console.print(message)


def table(headers: list[str], rows: list[list[str]], data: Any = None) -> None:
    """Output tabular data."""
    if _json_mode:
        _print_json({"success": True, "data": data})
    else:
        t = Table()
        for header in headers:
            t.add_column(header)
        for row in rows:
            t.add_row(*row)
        _console.print(t)


@contextmanager
def spinner(message: str):
    """Show a spinner while work is in progress. No-op in JSON mode."""
    if _json_mode:
        yield
    else:
        with _console.status(message):
            yield


def confirm(message: str) -> bool:
    """Prompt for confirmation. Returns True in JSON mode (non-interactive)."""
    if _json_mode:
        return True
    try:
        response = _console.input(f"{message} [y/N] ")
        return response.lower() in ("y", "yes")
    except (EOFError, KeyboardInterrupt):
        print(file=sys.stderr)
        return False
