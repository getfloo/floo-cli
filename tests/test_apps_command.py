"""Tests for the apps command group."""

from __future__ import annotations

import json
import uuid

import respx
from httpx import Response
from typer.testing import CliRunner

from floo import output
from floo.cli import app
from floo.config import FlooConfig

runner = CliRunner()

_API_URL = "https://api.test.local"
_APP_ID = str(uuid.uuid4())

_APP_RESPONSE = {
    "id": _APP_ID,
    "name": "test-app",
    "user_id": str(uuid.uuid4()),
    "status": "live",
    "url": "https://test-app.on.getfloo.com",
    "runtime": "nodejs",
    "created_at": "2025-01-01T00:00:00Z",
    "updated_at": "2025-01-01T00:00:00Z",
}

_LIST_RESPONSE = {
    "apps": [_APP_RESPONSE],
    "total": 1,
    "page": 1,
    "per_page": 20,
}

_EMPTY_LIST_RESPONSE = {
    "apps": [],
    "total": 0,
    "page": 1,
    "per_page": 20,
}


def _config_with_key() -> FlooConfig:
    return FlooConfig(api_key="floo_test_key_12345", api_url=_API_URL)


def _config_no_key() -> FlooConfig:
    return FlooConfig(api_key=None, api_url=_API_URL)


# --- apps list ---


@respx.mock
def test_list_success(monkeypatch):
    output.set_json_mode(False)
    monkeypatch.setattr("floo.commands.apps.load_config", lambda: _config_with_key())
    respx.get(f"{_API_URL}/v1/apps").mock(return_value=Response(200, json=_LIST_RESPONSE))

    result = runner.invoke(app, ["apps", "list"])
    assert result.exit_code == 0
    assert "test-app" in result.output


@respx.mock
def test_list_json(monkeypatch):
    output.set_json_mode(False)
    monkeypatch.setattr("floo.commands.apps.load_config", lambda: _config_with_key())
    respx.get(f"{_API_URL}/v1/apps").mock(return_value=Response(200, json=_LIST_RESPONSE))

    result = runner.invoke(app, ["--json", "apps", "list"])
    assert result.exit_code == 0
    data = json.loads(result.output)
    assert data["success"] is True
    assert len(data["data"]["apps"]) == 1
    output.set_json_mode(False)


@respx.mock
def test_list_empty(monkeypatch):
    output.set_json_mode(False)
    monkeypatch.setattr("floo.commands.apps.load_config", lambda: _config_with_key())
    respx.get(f"{_API_URL}/v1/apps").mock(
        return_value=Response(200, json=_EMPTY_LIST_RESPONSE)
    )

    result = runner.invoke(app, ["apps", "list"])
    assert result.exit_code == 0
    assert "floo deploy" in result.output


def test_list_not_authenticated(monkeypatch):
    output.set_json_mode(False)
    monkeypatch.setattr("floo.commands.apps.load_config", lambda: _config_no_key())

    result = runner.invoke(app, ["apps", "list"])
    assert result.exit_code == 1
    assert "Not logged in" in result.output


# --- apps status ---


@respx.mock
def test_status_by_name(monkeypatch):
    output.set_json_mode(False)
    monkeypatch.setattr("floo.commands.apps.load_config", lambda: _config_with_key())
    _not_found = {"detail": {"code": "APP_NOT_FOUND", "message": "Not found"}}
    respx.get(f"{_API_URL}/v1/apps/test-app").mock(
        return_value=Response(404, json=_not_found)
    )
    respx.get(f"{_API_URL}/v1/apps").mock(return_value=Response(200, json=_LIST_RESPONSE))

    result = runner.invoke(app, ["apps", "status", "test-app"])
    assert result.exit_code == 0
    assert "test-app" in result.output
    assert "live" in result.output


@respx.mock
def test_status_by_id(monkeypatch):
    output.set_json_mode(False)
    monkeypatch.setattr("floo.commands.apps.load_config", lambda: _config_with_key())
    respx.get(f"{_API_URL}/v1/apps/{_APP_ID}").mock(
        return_value=Response(200, json=_APP_RESPONSE)
    )

    result = runner.invoke(app, ["apps", "status", _APP_ID])
    assert result.exit_code == 0
    assert "test-app" in result.output


@respx.mock
def test_status_not_found(monkeypatch):
    output.set_json_mode(False)
    monkeypatch.setattr("floo.commands.apps.load_config", lambda: _config_with_key())
    _not_found = {"detail": {"code": "APP_NOT_FOUND", "message": "Not found"}}
    respx.get(f"{_API_URL}/v1/apps/ghost").mock(
        return_value=Response(404, json=_not_found)
    )
    respx.get(f"{_API_URL}/v1/apps").mock(return_value=Response(200, json=_EMPTY_LIST_RESPONSE))

    result = runner.invoke(app, ["apps", "status", "ghost"])
    assert result.exit_code == 1
    assert "not found" in result.output.lower()


@respx.mock
def test_status_json(monkeypatch):
    output.set_json_mode(False)
    monkeypatch.setattr("floo.commands.apps.load_config", lambda: _config_with_key())
    respx.get(f"{_API_URL}/v1/apps/{_APP_ID}").mock(
        return_value=Response(200, json=_APP_RESPONSE)
    )

    result = runner.invoke(app, ["--json", "apps", "status", _APP_ID])
    assert result.exit_code == 0
    data = json.loads(result.output)
    assert data["success"] is True
    assert data["data"]["name"] == "test-app"
    output.set_json_mode(False)


# --- apps delete ---


@respx.mock
def test_delete_success(monkeypatch):
    output.set_json_mode(False)
    monkeypatch.setattr("floo.commands.apps.load_config", lambda: _config_with_key())
    respx.get(f"{_API_URL}/v1/apps/{_APP_ID}").mock(
        return_value=Response(200, json=_APP_RESPONSE)
    )
    respx.delete(f"{_API_URL}/v1/apps/{_APP_ID}").mock(return_value=Response(204))

    result = runner.invoke(app, ["apps", "delete", _APP_ID, "--force"])
    assert result.exit_code == 0
    assert "Deleted" in result.output


@respx.mock
def test_delete_force(monkeypatch):
    output.set_json_mode(False)
    monkeypatch.setattr("floo.commands.apps.load_config", lambda: _config_with_key())
    respx.get(f"{_API_URL}/v1/apps/{_APP_ID}").mock(
        return_value=Response(200, json=_APP_RESPONSE)
    )
    respx.delete(f"{_API_URL}/v1/apps/{_APP_ID}").mock(return_value=Response(204))

    result = runner.invoke(app, ["--json", "apps", "delete", _APP_ID, "--force"])
    assert result.exit_code == 0
    data = json.loads(result.output)
    assert data["success"] is True
    output.set_json_mode(False)


@respx.mock
def test_delete_cancelled(monkeypatch):
    output.set_json_mode(False)
    monkeypatch.setattr("floo.commands.apps.load_config", lambda: _config_with_key())
    respx.get(f"{_API_URL}/v1/apps/{_APP_ID}").mock(
        return_value=Response(200, json=_APP_RESPONSE)
    )

    result = runner.invoke(app, ["apps", "delete", _APP_ID], input="n\n")
    assert result.exit_code == 0
    assert "Cancelled" in result.output


@respx.mock
def test_delete_not_found(monkeypatch):
    output.set_json_mode(False)
    monkeypatch.setattr("floo.commands.apps.load_config", lambda: _config_with_key())
    _not_found = {"detail": {"code": "APP_NOT_FOUND", "message": "Not found"}}
    respx.get(f"{_API_URL}/v1/apps/ghost").mock(
        return_value=Response(404, json=_not_found)
    )
    respx.get(f"{_API_URL}/v1/apps").mock(return_value=Response(200, json=_EMPTY_LIST_RESPONSE))

    result = runner.invoke(app, ["apps", "delete", "ghost", "--force"])
    assert result.exit_code == 1
    assert "not found" in result.output.lower()


def test_delete_not_authenticated(monkeypatch):
    output.set_json_mode(False)
    monkeypatch.setattr("floo.commands.apps.load_config", lambda: _config_no_key())

    result = runner.invoke(app, ["apps", "delete", "test-app", "--force"])
    assert result.exit_code == 1
    assert "Not logged in" in result.output
