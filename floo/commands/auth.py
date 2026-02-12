"""Authentication commands: login, logout, whoami."""

from __future__ import annotations

import typer
from rich.prompt import Prompt

from floo import output
from floo.api_client import FlooClient
from floo.config import clear_config, load_config, save_config
from floo.errors import FlooAPIError

auth_app = typer.Typer(name="auth", hidden=True)


@auth_app.callback(invoke_without_command=True)
def auth_callback(ctx: typer.Context) -> None:
    """Authentication commands (use login/logout/whoami directly)."""
    if ctx.invoked_subcommand is None:
        raise typer.Exit()


def login(
    email: str | None = typer.Option(None, "--email", "-e", help="Account email"),
    password: str | None = typer.Option(None, "--password", "-p", help="Account password"),
) -> None:
    """Authenticate with the Floo API and store credentials."""
    if not email:
        email = Prompt.ask("[bold]Email[/bold]")
    if not password:
        password = Prompt.ask("[bold]Password[/bold]", password=True)

    with output.spinner("Logging in..."):
        try:
            client = FlooClient()
            result = client.login(email, password)
            client.close()
        except FlooAPIError as e:
            output.error(e.message, code=e.code, suggestion="Check your email and password.")
            raise typer.Exit(1) from None

    config = load_config()
    config.api_key = result["api_key"]
    config.user_email = result["email"]
    save_config(config)

    output.success(f"Logged in as {result['email']}", data={"email": result["email"]})


def logout() -> None:
    """Clear stored credentials."""
    clear_config()
    output.success("Logged out.", data=None)


def whoami() -> None:
    """Show the currently authenticated user."""
    config = load_config()
    if not config.api_key:
        output.error(
            "Not logged in.",
            code="NOT_AUTHENTICATED",
            suggestion="Run 'floo login' to authenticate.",
        )
        raise typer.Exit(1)

    masked_key = config.api_key[:9] + "..." + config.api_key[-4:]
    data = {"email": config.user_email, "api_key": masked_key}
    output.success(
        f"Logged in as {config.user_email} (key: {masked_key})",
        data=data,
    )
