"""Tests for the domains command group."""

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
    "url": "https://test-app.fly.dev",
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

_DOMAIN_RESPONSE = {
    "id": str(uuid.uuid4()),
    "app_id": _APP_ID,
    "hostname": "app.example.com",
    "status": "pending",
    "created_at": "2025-01-01T00:00:00Z",
    "updated_at": "2025-01-01T00:00:00Z",
    "dns_instructions": "Add a CNAME record: app.example.com -> test-app.fly.dev",
}

_DOMAIN_LIST_RESPONSE = {
    "domains": [_DOMAIN_RESPONSE],
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


# --- domains add ---


@respx.mock
def test_add_success(monkeypatch):
    output.set_json_mode(False)
    monkeypatch.setattr("floo.commands.domains.load_config", lambda: _config_with_key())
    _mock_resolve_by_name()
    respx.post(f"{_API_URL}/v1/apps/{_APP_ID}/domains").mock(
        return_value=Response(201, json=_DOMAIN_RESPONSE)
    )

    result = runner.invoke(app, ["domains", "add", "app.example.com", "--app", "test-app"])
    assert result.exit_code == 0
    assert "app.example.com" in result.output
    assert "CNAME" in result.output


@respx.mock
def test_add_json(monkeypatch):
    output.set_json_mode(False)
    monkeypatch.setattr("floo.commands.domains.load_config", lambda: _config_with_key())
    _mock_resolve_by_name()
    respx.post(f"{_API_URL}/v1/apps/{_APP_ID}/domains").mock(
        return_value=Response(201, json=_DOMAIN_RESPONSE)
    )

    result = runner.invoke(
        app, ["--json", "domains", "add", "app.example.com", "--app", "test-app"]
    )
    assert result.exit_code == 0
    data = json.loads(result.output)
    assert data["success"] is True
    assert data["data"]["hostname"] == "app.example.com"
    output.set_json_mode(False)


@respx.mock
def test_add_duplicate(monkeypatch):
    output.set_json_mode(False)
    monkeypatch.setattr("floo.commands.domains.load_config", lambda: _config_with_key())
    _mock_resolve_by_name()
    respx.post(f"{_API_URL}/v1/apps/{_APP_ID}/domains").mock(
        return_value=Response(
            409, json={"detail": {"code": "DOMAIN_TAKEN", "message": "Domain taken"}}
        )
    )

    result = runner.invoke(app, ["domains", "add", "dup.example.com", "--app", "test-app"])
    assert result.exit_code == 1


def test_add_not_authenticated(monkeypatch):
    output.set_json_mode(False)
    monkeypatch.setattr("floo.commands.domains.load_config", lambda: _config_no_key())

    result = runner.invoke(app, ["domains", "add", "app.example.com", "--app", "test-app"])
    assert result.exit_code == 1
    assert "Not logged in" in result.output


# --- domains list ---


@respx.mock
def test_list_success(monkeypatch):
    output.set_json_mode(False)
    monkeypatch.setattr("floo.commands.domains.load_config", lambda: _config_with_key())
    _mock_resolve_by_name()
    respx.get(f"{_API_URL}/v1/apps/{_APP_ID}/domains").mock(
        return_value=Response(200, json=_DOMAIN_LIST_RESPONSE)
    )

    result = runner.invoke(app, ["domains", "list", "--app", "test-app"])
    assert result.exit_code == 0
    assert "app.example.com" in result.output


@respx.mock
def test_list_json(monkeypatch):
    output.set_json_mode(False)
    monkeypatch.setattr("floo.commands.domains.load_config", lambda: _config_with_key())
    _mock_resolve_by_name()
    respx.get(f"{_API_URL}/v1/apps/{_APP_ID}/domains").mock(
        return_value=Response(200, json=_DOMAIN_LIST_RESPONSE)
    )

    result = runner.invoke(app, ["--json", "domains", "list", "--app", "test-app"])
    assert result.exit_code == 0
    data = json.loads(result.output)
    assert data["success"] is True
    assert len(data["data"]["domains"]) == 1
    output.set_json_mode(False)


@respx.mock
def test_list_empty(monkeypatch):
    output.set_json_mode(False)
    monkeypatch.setattr("floo.commands.domains.load_config", lambda: _config_with_key())
    _mock_resolve_by_name()
    respx.get(f"{_API_URL}/v1/apps/{_APP_ID}/domains").mock(
        return_value=Response(200, json={"domains": [], "total": 0})
    )

    result = runner.invoke(app, ["domains", "list", "--app", "test-app"])
    assert result.exit_code == 0
    assert "No custom domains" in result.output


# --- domains remove ---


@respx.mock
def test_remove_success(monkeypatch):
    output.set_json_mode(False)
    monkeypatch.setattr("floo.commands.domains.load_config", lambda: _config_with_key())
    _mock_resolve_by_name()
    respx.delete(f"{_API_URL}/v1/apps/{_APP_ID}/domains/app.example.com").mock(
        return_value=Response(204)
    )

    result = runner.invoke(app, ["domains", "remove", "app.example.com", "--app", "test-app"])
    assert result.exit_code == 0
    assert "Removed" in result.output


@respx.mock
def test_remove_not_found(monkeypatch):
    output.set_json_mode(False)
    monkeypatch.setattr("floo.commands.domains.load_config", lambda: _config_with_key())
    _mock_resolve_by_name()
    respx.delete(f"{_API_URL}/v1/apps/{_APP_ID}/domains/missing.example.com").mock(
        return_value=Response(
            404, json={"detail": {"code": "DOMAIN_NOT_FOUND", "message": "Not found"}}
        )
    )

    result = runner.invoke(
        app, ["domains", "remove", "missing.example.com", "--app", "test-app"]
    )
    assert result.exit_code == 1


def test_remove_not_authenticated(monkeypatch):
    output.set_json_mode(False)
    monkeypatch.setattr("floo.commands.domains.load_config", lambda: _config_no_key())

    result = runner.invoke(app, ["domains", "remove", "app.example.com", "--app", "test-app"])
    assert result.exit_code == 1
    assert "Not logged in" in result.output
