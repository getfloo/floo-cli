"""Floo CLI — deploy, manage, and observe web apps."""

from __future__ import annotations

from typing import Annotated

import typer

from floo import __version__, output
from floo.commands.apps import apps_app
from floo.commands.auth import login, logout, whoami
from floo.commands.deploy import deploy
from floo.commands.domains import domains_app
from floo.commands.env import env_app

app = typer.Typer(
    name="floo",
    no_args_is_help=True,
    rich_markup_mode="rich",
    help="Deploy, manage, and observe web apps.",
)


def _version_callback(value: bool) -> None:
    if value:
        output.info(f"floo {__version__}", data={"version": __version__})
        raise typer.Exit()


@app.callback()
def main(
    json_mode: Annotated[
        bool, typer.Option("--json", help="Output JSON to stdout (for agents).")
    ] = False,
    version: Annotated[
        bool | None,
        typer.Option("--version", "-v", callback=_version_callback, is_eager=True),
    ] = None,
) -> None:
    """Floo CLI — deploy, manage, and observe web apps."""
    if json_mode:
        output.set_json_mode()


# Register top-level commands
app.command()(deploy)
app.command()(login)
app.command()(logout)
app.command()(whoami)

# Register subcommand groups
app.add_typer(apps_app)
app.add_typer(env_app)
app.add_typer(domains_app)
