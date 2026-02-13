"""Deploy command — detect, archive, and deploy a project."""

from __future__ import annotations

import time
from pathlib import Path
from typing import Annotated

import typer
from rich.console import Console

from floo import output
from floo.api_client import FlooClient
from floo.archive import create_archive
from floo.config import load_config
from floo.detection import detect
from floo.errors import FlooAPIError, FlooError
from floo.names import generate_name

_console = Console(stderr=True)

_STATUS_LABELS = {
    "pending": "Queued...",
    "building": "Building...",
    "deploying": "Deploying...",
}

_TERMINAL_STATUSES = {"live", "failed"}
_POLL_INTERVAL = 2


def deploy(
    path: Annotated[
        Path, typer.Argument(help="Project directory to deploy.", show_default=".")
    ] = Path("."),
    name: Annotated[
        str | None, typer.Option("--name", "-n", help="App name (generated if omitted).")
    ] = None,
    app: Annotated[
        str | None, typer.Option("--app", "-a", help="Existing app ID or name to deploy to.")
    ] = None,
) -> None:
    """Deploy a project to Floo."""
    config = load_config()
    if not config.api_key:
        output.error(
            "Not logged in.",
            code="NOT_AUTHENTICATED",
            suggestion="Run 'floo login' to authenticate.",
        )
        raise typer.Exit(1) from None

    project_path = path.resolve()
    if not project_path.is_dir():
        output.error(
            f"Path '{path}' is not a directory.",
            code="INVALID_PATH",
            suggestion="Provide a valid project directory.",
        )
        raise typer.Exit(1) from None

    # Detect runtime/framework
    detection = detect(project_path)
    if detection.runtime == "unknown":
        output.error(
            "No supported project files found.",
            code="NO_RUNTIME_DETECTED",
            suggestion="Add a package.json, requirements.txt, or Dockerfile to your project.",
        )
        raise typer.Exit(1) from None

    if not output.is_json_mode():
        framework_label = f" ({detection.framework})" if detection.framework else ""
        output.info(
            f"Detected [bold]{detection.runtime}[/bold]{framework_label}"
            f" — {detection.confidence} confidence"
        )

    if detection.confidence == "low" and not output.confirm("Continue with this detection?"):
        raise typer.Exit(0) from None

    # Create archive
    archive_path: Path | None = None
    client = FlooClient(config)
    try:
        with output.spinner("Packaging source..."):
            try:
                archive_path = create_archive(project_path)
            except FlooError as e:
                output.error(e.message, code=e.code, suggestion=e.suggestion)
                raise typer.Exit(1) from None

        # Resolve or create app
        app_data: dict
        if app is not None:
            with output.spinner("Looking up app..."):
                try:
                    app_data = client.get_app(app)
                except FlooAPIError:
                    # Try finding by name in the user's app list
                    try:
                        apps_resp = client.list_apps()
                        matched = [
                            a for a in apps_resp["apps"] if a["name"] == app
                        ]
                        if not matched:
                            output.error(
                                f"App '{app}' not found.",
                                code="APP_NOT_FOUND",
                                suggestion="Check the app ID or name and try again.",
                            )
                            raise typer.Exit(1) from None
                        app_data = matched[0]
                    except FlooAPIError as e:
                        output.error(e.message, code=e.code)
                        raise typer.Exit(1) from None
        else:
            app_name = name or generate_name()
            with output.spinner(f"Creating app [bold]{app_name}[/bold]..."):
                try:
                    app_data = client.create_app(app_name, runtime=detection.runtime)
                except FlooAPIError as e:
                    output.error(e.message, code=e.code)
                    raise typer.Exit(1) from None

        # Deploy
        with output.spinner("Uploading..."):
            try:
                deploy_data = client.create_deploy(
                    app_id=app_data["id"],
                    tarball_path=archive_path,
                    runtime=detection.runtime,
                    framework=detection.framework,
                )
            except FlooAPIError as e:
                output.error(e.message, code=e.code)
                raise typer.Exit(1) from None

        # Poll until terminal status if the API returned a non-terminal status
        last_log_len = 0
        while deploy_data["status"] not in _TERMINAL_STATUSES:
            if not output.is_json_mode():
                build_logs = deploy_data.get("build_logs") or ""
                new_logs = build_logs[last_log_len:]
                if new_logs:
                    for line in new_logs.strip().splitlines():
                        _console.print(f"  {line}", style="dim")
                    last_log_len = len(build_logs)

                label = _STATUS_LABELS.get(deploy_data["status"], "Deploying...")
                _console.print(f"  {label}", style="bold")

            time.sleep(_POLL_INTERVAL)
            deploy_data = client.get_deploy(
                app_id=app_data["id"], deploy_id=deploy_data["id"]
            )

        if deploy_data["status"] == "failed":
            if not output.is_json_mode() and deploy_data.get("build_logs"):
                build_logs = deploy_data["build_logs"]
                new_logs = build_logs[last_log_len:]
                if new_logs:
                    for line in new_logs.strip().splitlines():
                        _console.print(f"  {line}", style="dim")
            output.error(
                "Deploy failed.",
                code="DEPLOY_FAILED",
            )
            raise typer.Exit(1) from None

        output.success(
            f"Deployed to {deploy_data['url']}",
            data={
                "app": app_data,
                "deploy": deploy_data,
                "detection": detection.to_dict(),
            },
        )
    finally:
        if archive_path is not None and archive_path.exists():
            archive_path.unlink()
        client.close()
