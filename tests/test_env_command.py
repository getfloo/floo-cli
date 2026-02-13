"""Tests for the env command group."""

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

_LIST_APPS_RESPONSE = {
    "apps": [_APP_RESPONSE],
    "total": 1,
    "page": 1,
    "per_page": 20,
}

_ENV_VAR_RESPONSE = {
    "id": str(uuid.uuid4()),
    "app_id": _APP_ID,
    "key": "DATABASE_URL",
    "masked_value": "post**************",
    "created_at": "2025-01-01T00:00:00Z",
    "updated_at": "2025-01-01T00:00:00Z",
}

_ENV_LIST_RESPONSE = {
    "env_vars": [_ENV_VAR_RESPONSE],
    "total": 1,
}


def _config_with_key() -> FlooConfig:
    return FlooConfig(api_key="floo_test_key_12345", api_url=_API_URL)


def _config_no_key() -> FlooConfig:
    return FlooConfig(api_key=None, api_url=_API_URL)


def _mock_resolve_by_name():
    """Mock app resolution by name (UUID lookup fails, list matches)."""
    respx.get(f"{_API_URL}/v1/apps/test-app").mock(
        return_value=Response(
            404, json={"detail": {"code": "APP_NOT_FOUND", "message": "Not found"}}
        )
    )
    respx.get(f"{_API_URL}/v1/apps").mock(
        return_value=Response(200, json=_LIST_APPS_RESPONSE)
    )


# --- env set ---


@respx.mock
def test_set_success(monkeypatch):
    output.set_json_mode(False)
    monkeypatch.setattr("floo.commands.env.load_config", lambda: _config_with_key())
    _mock_resolve_by_name()
    respx.post(f"{_API_URL}/v1/apps/{_APP_ID}/env").mock(
        return_value=Response(200, json=_ENV_VAR_RESPONSE)
    )

    result = runner.invoke(
        app, ["env", "set", "database_url=postgres://localhost", "--app", "test-app"]
    )
    assert result.exit_code == 0
    assert "DATABASE_URL" in result.output


@respx.mock
def test_set_json(monkeypatch):
    output.set_json_mode(False)
    monkeypatch.setattr("floo.commands.env.load_config", lambda: _config_with_key())
    _mock_resolve_by_name()
    respx.post(f"{_API_URL}/v1/apps/{_APP_ID}/env").mock(
        return_value=Response(200, json=_ENV_VAR_RESPONSE)
    )

    result = runner.invoke(
        app, ["--json", "env", "set", "DATABASE_URL=postgres://localhost", "--app", "test-app"]
    )
    assert result.exit_code == 0
    data = json.loads(result.output)
    assert data["success"] is True
    assert data["data"]["key"] == "DATABASE_URL"
    output.set_json_mode(False)


def test_set_invalid_format(monkeypatch):
    output.set_json_mode(False)
    monkeypatch.setattr("floo.commands.env.load_config", lambda: _config_with_key())

    result = runner.invoke(app, ["env", "set", "NO_EQUALS_SIGN", "--app", "test-app"])
    assert result.exit_code == 1
    assert "KEY=VALUE" in result.output


def test_set_not_authenticated(monkeypatch):
    output.set_json_mode(False)
    monkeypatch.setattr("floo.commands.env.load_config", lambda: _config_no_key())

    result = runner.invoke(app, ["env", "set", "FOO=bar", "--app", "test-app"])
    assert result.exit_code == 1
    assert "Not logged in" in result.output


# --- env list ---


@respx.mock
def test_list_success(monkeypatch):
    output.set_json_mode(False)
    monkeypatch.setattr("floo.commands.env.load_config", lambda: _config_with_key())
    _mock_resolve_by_name()
    respx.get(f"{_API_URL}/v1/apps/{_APP_ID}/env").mock(
        return_value=Response(200, json=_ENV_LIST_RESPONSE)
    )

    result = runner.invoke(app, ["env", "list", "--app", "test-app"])
    assert result.exit_code == 0
    assert "DATABASE_URL" in result.output


@respx.mock
def test_list_json(monkeypatch):
    output.set_json_mode(False)
    monkeypatch.setattr("floo.commands.env.load_config", lambda: _config_with_key())
    _mock_resolve_by_name()
    respx.get(f"{_API_URL}/v1/apps/{_APP_ID}/env").mock(
        return_value=Response(200, json=_ENV_LIST_RESPONSE)
    )

    result = runner.invoke(app, ["--json", "env", "list", "--app", "test-app"])
    assert result.exit_code == 0
    data = json.loads(result.output)
    assert data["success"] is True
    assert len(data["data"]["env_vars"]) == 1
    output.set_json_mode(False)


@respx.mock
def test_list_empty(monkeypatch):
    output.set_json_mode(False)
    monkeypatch.setattr("floo.commands.env.load_config", lambda: _config_with_key())
    _mock_resolve_by_name()
    respx.get(f"{_API_URL}/v1/apps/{_APP_ID}/env").mock(
        return_value=Response(200, json={"env_vars": [], "total": 0})
    )

    result = runner.invoke(app, ["env", "list", "--app", "test-app"])
    assert result.exit_code == 0
    assert "No environment variables" in result.output


# --- env remove ---


@respx.mock
def test_remove_success(monkeypatch):
    output.set_json_mode(False)
    monkeypatch.setattr("floo.commands.env.load_config", lambda: _config_with_key())
    _mock_resolve_by_name()
    respx.delete(f"{_API_URL}/v1/apps/{_APP_ID}/env/DATABASE_URL").mock(
        return_value=Response(204)
    )

    result = runner.invoke(app, ["env", "remove", "database_url", "--app", "test-app"])
    assert result.exit_code == 0
    assert "Removed" in result.output


@respx.mock
def test_remove_not_found(monkeypatch):
    output.set_json_mode(False)
    monkeypatch.setattr("floo.commands.env.load_config", lambda: _config_with_key())
    _mock_resolve_by_name()
    respx.delete(f"{_API_URL}/v1/apps/{_APP_ID}/env/MISSING").mock(
        return_value=Response(
            404, json={"detail": {"code": "ENV_VAR_NOT_FOUND", "message": "Not found"}}
        )
    )

    result = runner.invoke(app, ["env", "remove", "MISSING", "--app", "test-app"])
    assert result.exit_code == 1


def test_remove_not_authenticated(monkeypatch):
    output.set_json_mode(False)
    monkeypatch.setattr("floo.commands.env.load_config", lambda: _config_no_key())

    result = runner.invoke(app, ["env", "remove", "FOO", "--app", "test-app"])
    assert result.exit_code == 1
    assert "Not logged in" in result.output
