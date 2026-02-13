"""Tests for the deploy command."""

from __future__ import annotations

import io
import json
import tarfile
import uuid
from pathlib import Path
from unittest.mock import patch

import respx
from httpx import Response
from typer.testing import CliRunner

from floo import output
from floo.cli import app
from floo.config import FlooConfig

runner = CliRunner()

_APP_ID = str(uuid.uuid4())
_DEPLOY_ID = str(uuid.uuid4())
_API_URL = "https://api.test.local"

_APP_RESPONSE = {
    "id": _APP_ID,
    "name": "test-app",
    "user_id": str(uuid.uuid4()),
    "status": "created",
    "url": None,
    "runtime": "nodejs",
    "created_at": "2025-01-01T00:00:00Z",
    "updated_at": "2025-01-01T00:00:00Z",
}

_DEPLOY_RESPONSE = {
    "id": _DEPLOY_ID,
    "app_id": _APP_ID,
    "status": "live",
    "runtime": "nodejs",
    "framework": "Express",
    "build_logs": None,
    "url": "https://test-app.on.getfloo.com",
    "created_at": "2025-01-01T00:00:00Z",
    "updated_at": "2025-01-01T00:00:00Z",
}


def _config_with_key() -> FlooConfig:
    return FlooConfig(api_key="floo_test_key_12345", api_url=_API_URL)


def _config_no_key() -> FlooConfig:
    return FlooConfig(api_key=None, api_url=_API_URL)


def _make_tarball(tmp_path: Path) -> Path:
    """Create a minimal valid .tar.gz for testing."""
    tarball_path = tmp_path / "source.tar.gz"
    buf = io.BytesIO()
    with tarfile.open(fileobj=buf, mode="w:gz") as tar:
        data = b"console.log('hello')"
        info = tarfile.TarInfo(name="index.js")
        info.size = len(data)
        tar.addfile(info, io.BytesIO(data))
    tarball_path.write_bytes(buf.getvalue())
    return tarball_path


def _setup_project(tmp_path: Path) -> None:
    """Create a minimal Node.js project for detection."""
    pkg = {"dependencies": {"express": "4.18.0"}}
    (tmp_path / "package.json").write_text(json.dumps(pkg))
    (tmp_path / "index.js").write_text("console.log('hello')")


@respx.mock
@patch("floo.commands.deploy.load_config")
@patch("floo.commands.deploy.create_archive")
def test_deploy_success(mock_archive, mock_config, tmp_path):
    output.set_json_mode(False)
    mock_config.return_value = _config_with_key()
    mock_archive.return_value = _make_tarball(tmp_path)
    _setup_project(tmp_path)

    respx.post(f"{_API_URL}/v1/apps").mock(
        return_value=Response(201, json=_APP_RESPONSE)
    )
    respx.post(f"{_API_URL}/v1/apps/{_APP_ID}/deploys").mock(
        return_value=Response(201, json=_DEPLOY_RESPONSE)
    )

    result = runner.invoke(app, ["deploy", str(tmp_path)])
    assert result.exit_code == 0
    assert "https://test-app.on.getfloo.com" in result.output


@respx.mock
@patch("floo.commands.deploy.load_config")
@patch("floo.commands.deploy.create_archive")
def test_deploy_success_json(mock_archive, mock_config, tmp_path):
    output.set_json_mode(False)
    mock_config.return_value = _config_with_key()
    mock_archive.return_value = _make_tarball(tmp_path)
    _setup_project(tmp_path)

    respx.post(f"{_API_URL}/v1/apps").mock(
        return_value=Response(201, json=_APP_RESPONSE)
    )
    respx.post(f"{_API_URL}/v1/apps/{_APP_ID}/deploys").mock(
        return_value=Response(201, json=_DEPLOY_RESPONSE)
    )

    result = runner.invoke(app, ["--json", "deploy", str(tmp_path)])
    assert result.exit_code == 0
    data = json.loads(result.output)
    assert data["success"] is True
    assert data["data"]["deploy"]["url"] == "https://test-app.on.getfloo.com"
    assert data["data"]["detection"]["runtime"] == "nodejs"
    output.set_json_mode(False)


@patch("floo.commands.deploy.load_config")
def test_deploy_not_authenticated(mock_config, tmp_path):
    output.set_json_mode(False)
    mock_config.return_value = _config_no_key()
    _setup_project(tmp_path)

    result = runner.invoke(app, ["deploy", str(tmp_path)])
    assert result.exit_code == 1
    assert "Not logged in" in result.output


@patch("floo.commands.deploy.load_config")
def test_deploy_no_project_files(mock_config, tmp_path):
    output.set_json_mode(False)
    mock_config.return_value = _config_with_key()

    result = runner.invoke(app, ["deploy", str(tmp_path)])
    assert result.exit_code == 1
    assert "No supported project files" in result.output


@respx.mock
@patch("floo.commands.deploy.load_config")
@patch("floo.commands.deploy.create_archive")
def test_deploy_existing_app(mock_archive, mock_config, tmp_path):
    output.set_json_mode(False)
    mock_config.return_value = _config_with_key()
    mock_archive.return_value = _make_tarball(tmp_path)
    _setup_project(tmp_path)

    respx.get(f"{_API_URL}/v1/apps/{_APP_ID}").mock(
        return_value=Response(200, json=_APP_RESPONSE)
    )
    respx.post(f"{_API_URL}/v1/apps/{_APP_ID}/deploys").mock(
        return_value=Response(201, json=_DEPLOY_RESPONSE)
    )

    result = runner.invoke(app, ["deploy", str(tmp_path), "--app", _APP_ID])
    assert result.exit_code == 0
    assert "https://test-app.on.getfloo.com" in result.output


@respx.mock
@patch("floo.commands.deploy.load_config")
@patch("floo.commands.deploy.create_archive")
def test_deploy_with_name(mock_archive, mock_config, tmp_path):
    output.set_json_mode(False)
    mock_config.return_value = _config_with_key()
    mock_archive.return_value = _make_tarball(tmp_path)
    _setup_project(tmp_path)

    respx.post(f"{_API_URL}/v1/apps").mock(
        return_value=Response(201, json={**_APP_RESPONSE, "name": "my-custom-app"})
    )
    respx.post(f"{_API_URL}/v1/apps/{_APP_ID}/deploys").mock(
        return_value=Response(201, json=_DEPLOY_RESPONSE)
    )

    result = runner.invoke(app, ["deploy", str(tmp_path), "--name", "my-custom-app"])
    assert result.exit_code == 0
    assert "https://test-app.on.getfloo.com" in result.output
