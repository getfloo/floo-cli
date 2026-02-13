"""Custom domain commands: add, list, remove."""

from __future__ import annotations

from typing import Annotated

import typer

from floo import output
from floo.api_client import FlooClient
from floo.config import load_config
from floo.errors import FlooAPIError
from floo.resolve import resolve_app

domains_app = typer.Typer(name="domains", help="Manage custom domains.")


@domains_app.command("add")
def add_domain(
    hostname: Annotated[str, typer.Argument(help="Domain hostname (e.g. app.example.com).")],
    app_name: Annotated[str, typer.Option("--app", "-a", help="App name or ID.")] = ...,
) -> None:
    """Add a custom domain to an app."""
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

        try:
            result = client.add_domain(app_data["id"], hostname)
        except FlooAPIError as e:
            output.error(e.message, code=e.code)
            raise typer.Exit(1) from None

        if output.is_json_mode():
            output.success(f"Added {hostname}", data=result)
        else:
            output.success(f"Added domain {hostname} to {app_data['name']}.")
            output.info(f"  Status: {result['status']}")
            if result.get("dns_instructions"):
                output.info(f"  DNS:    {result['dns_instructions']}")
    finally:
        client.close()


@domains_app.command("list")
def list_domains(
    app_name: Annotated[str, typer.Option("--app", "-a", help="App name or ID.")] = ...,
) -> None:
    """List custom domains for an app."""
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

        try:
            result = client.list_domains(app_data["id"])
        except FlooAPIError as e:
            output.error(e.message, code=e.code)
            raise typer.Exit(1) from None
    finally:
        client.close()

    domains = result["domains"]
    if not domains:
        if not output.is_json_mode():
            output.info(
                f"No custom domains on {app_data['name']}. "
                f"Add one with [bold]floo domains add example.com --app {app_data['name']}[/bold]."
            )
        else:
            output.success("No domains.", data={"domains": []})
        return

    rows = [
        [d["hostname"], d["status"], d.get("dns_instructions") or "—"]
        for d in domains
    ]
    output.table(
        headers=["Domain", "Status", "DNS"],
        rows=rows,
        data={"domains": domains},
    )


@domains_app.command("remove")
def remove_domain(
    hostname: Annotated[str, typer.Argument(help="Domain hostname to remove.")],
    app_name: Annotated[str, typer.Option("--app", "-a", help="App name or ID.")] = ...,
) -> None:
    """Remove a custom domain from an app."""
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

        try:
            client.delete_domain(app_data["id"], hostname)
        except FlooAPIError as e:
            output.error(e.message, code=e.code)
            raise typer.Exit(1) from None

        output.success(
            f"Removed domain {hostname} from {app_data['name']}.",
            data={"hostname": hostname},
        )
    finally:
        client.close()
