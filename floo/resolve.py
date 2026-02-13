"""App resolution helpers for CLI commands."""

from __future__ import annotations

from typing import Any

from floo.api_client import FlooClient
from floo.errors import FlooAPIError


def resolve_app(client: FlooClient, identifier: str) -> dict[str, Any] | None:
    """Resolve an app by UUID or name.

    Tries UUID lookup first via get_app(), then falls back to name match
    via list_apps().

    Returns the app dict, or None if not found.
    """
    try:
        return client.get_app(identifier)
    except FlooAPIError:
        pass

    try:
        apps_resp = client.list_apps(per_page=100)
        matched = [a for a in apps_resp["apps"] if a["name"] == identifier]
        if matched:
            return matched[0]
    except FlooAPIError:
        pass

    return None
