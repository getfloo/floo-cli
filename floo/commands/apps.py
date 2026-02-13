"""App management commands: list, status, delete."""

from __future__ import annotations

from typing import Annotated

import typer

from floo import output
from floo.api_client import FlooClient
from floo.config import load_config
from floo.errors import FlooAPIError
from floo.resolve import resolve_app

apps_app = typer.Typer(name="apps", help="Manage your apps.")


@apps_app.command("list")
def list_apps() -> None:
    """List all your apps."""
    config = load_config()
    if not config.api_key:
        output.error(
            "Not logged in.",
            code="NOT_AUTHENTICATED",
            suggestion="Run 'floo login' to authenticate.",
        )
        raise typer.Exit(1) from None

    client = FlooClient(config)
    try:
        result = client.list_apps()
    except FlooAPIError as e:
        output.error(e.message, code=e.code)
        raise typer.Exit(1) from None
    finally:
        client.close()

    apps = result["apps"]
    if not apps:
        if not output.is_json_mode():
            output.info("No apps yet. Deploy one with [bold]floo deploy[/bold].")
        else:
            output.success("No apps.", data={"apps": []})
        return

    rows = []
    for a in apps:
        rows.append([
            a["name"],
            a["status"],
            a.get("url") or "—",
            a.get("runtime") or "—",
            a["created_at"],
        ])

    output.table(
        headers=["Name", "Status", "URL", "Runtime", "Created"],
        rows=rows,
        data={"apps": apps},
    )


@apps_app.command("status")
def app_status(
    app_name: Annotated[str, typer.Argument(help="App name or ID.")],
) -> None:
    """Show details for an app."""
    config = load_config()
    if not config.api_key:
        output.error(
            "Not logged in.",
            code="NOT_AUTHENTICATED",
            suggestion="Run 'floo login' to authenticate.",
        )
        raise typer.Exit(1) from None

    client = FlooClient(config)
    try:
        app_data = resolve_app(client, app_name)
    finally:
        client.close()

    if app_data is None:
        output.error(
            f"App '{app_name}' not found.",
            code="APP_NOT_FOUND",
            suggestion="Check the app name or ID and try again.",
        )
        raise typer.Exit(1) from None

    if output.is_json_mode():
        output.success(f"App {app_data['name']}", data=app_data)
    else:
        output.info(f"[bold]{app_data['name']}[/bold]")
        output.info(f"  Status:   {app_data['status']}")
        output.info(f"  URL:      {app_data.get('url') or '—'}")
        output.info(f"  Runtime:  {app_data.get('runtime') or '—'}")
        output.info(f"  ID:       {app_data['id']}")
        output.info(f"  Created:  {app_data['created_at']}")


@apps_app.command("delete")
def delete_app(
    app_name: Annotated[str, typer.Argument(help="App name or ID.")],
    force: Annotated[bool, typer.Option("--force", "-f", help="Skip confirmation.")] = False,
) -> None:
    """Delete an app."""
    config = load_config()
    if not config.api_key:
        output.error(
            "Not logged in.",
            code="NOT_AUTHENTICATED",
            suggestion="Run 'floo login' to authenticate.",
        )
        raise typer.Exit(1) from None

    client = FlooClient(config)
    try:
        app_data = resolve_app(client, app_name)
        if app_data is None:
            output.error(
                f"App '{app_name}' not found.",
                code="APP_NOT_FOUND",
                suggestion="Check the app name or ID and try again.",
            )
            raise typer.Exit(1) from None

        if not force and not output.confirm(
            f"Delete app '{app_data['name']}'? This cannot be undone."
        ):
            if not output.is_json_mode():
                output.info("Cancelled.")
            raise typer.Exit(0) from None

        try:
            client.delete_app(app_data["id"])
        except FlooAPIError as e:
            output.error(e.message, code=e.code)
            raise typer.Exit(1) from None

        output.success(f"Deleted app '{app_data['name']}'.", data={"id": app_data["id"]})
    finally:
        client.close()
