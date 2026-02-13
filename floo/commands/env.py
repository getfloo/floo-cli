"""Environment variable commands: set, list, remove."""

from __future__ import annotations

from typing import Annotated

import typer

from floo import output
from floo.api_client import FlooClient
from floo.config import load_config
from floo.errors import FlooAPIError
from floo.resolve import resolve_app

env_app = typer.Typer(name="env", help="Manage environment variables.")


@env_app.command("set")
def set_env(
    key_value: Annotated[str, typer.Argument(help="KEY=VALUE pair to set.")],
    app_name: Annotated[str, typer.Option("--app", "-a", help="App name or ID.")] = ...,
) -> None:
    """Set an environment variable on an app."""
    config = load_config()
    if not config.api_key:
        output.error(
            "Not logged in.",
            code="NOT_AUTHENTICATED",
            suggestion="Run 'floo login' to authenticate.",
        )
        raise typer.Exit(1) from None

    if "=" not in key_value:
        output.error(
            "Invalid format. Use KEY=VALUE.",
            code="INVALID_FORMAT",
            suggestion="Example: floo env set DATABASE_URL=postgres://...",
        )
        raise typer.Exit(1) from None

    key, value = key_value.split("=", 1)
    key = key.upper()

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
            result = client.set_env_var(app_data["id"], key, value)
        except FlooAPIError as e:
            output.error(e.message, code=e.code)
            raise typer.Exit(1) from None

        output.success(f"Set {key} on {app_data['name']}.", data=result)
    finally:
        client.close()


@env_app.command("list")
def list_env(
    app_name: Annotated[str, typer.Option("--app", "-a", help="App name or ID.")] = ...,
) -> None:
    """List environment variables for an app."""
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
            result = client.list_env_vars(app_data["id"])
        except FlooAPIError as e:
            output.error(e.message, code=e.code)
            raise typer.Exit(1) from None
    finally:
        client.close()

    env_vars = result["env_vars"]
    if not env_vars:
        if not output.is_json_mode():
            output.info(
                f"No environment variables set on {app_data['name']}. "
                "Set one with [bold]floo env set KEY=VALUE --app "
                f"{app_data['name']}[/bold]."
            )
        else:
            output.success("No env vars.", data={"env_vars": []})
        return

    rows = [[ev["key"], ev["masked_value"]] for ev in env_vars]
    output.table(
        headers=["Key", "Value"],
        rows=rows,
        data={"env_vars": env_vars},
    )


@env_app.command("remove")
def remove_env(
    key: Annotated[str, typer.Argument(help="Environment variable key to remove.")],
    app_name: Annotated[str, typer.Option("--app", "-a", help="App name or ID.")] = ...,
) -> None:
    """Remove an environment variable from an app."""
    key = key.upper()

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
            client.delete_env_var(app_data["id"], key)
        except FlooAPIError as e:
            output.error(e.message, code=e.code)
            raise typer.Exit(1) from None

        output.success(f"Removed {key} from {app_data['name']}.", data={"key": key})
    finally:
        client.close()
