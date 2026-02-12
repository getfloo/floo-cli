"""HTTP client for communicating with the Floo API."""

from __future__ import annotations

from pathlib import Path
from typing import Any

import httpx

from floo.config import FlooConfig, load_config
from floo.errors import FlooAPIError


class FlooClient:
    """Wrapper around httpx for Floo API calls."""

    def __init__(self, config: FlooConfig | None = None) -> None:
        self._config = config or load_config()
        headers: dict[str, str] = {}
        if self._config.api_key:
            headers["Authorization"] = f"Bearer {self._config.api_key}"
        self._client = httpx.Client(
            base_url=self._config.api_url,
            headers=headers,
            timeout=30.0,
        )

    def _handle_response(self, response: httpx.Response) -> dict[str, Any]:
        """Parse response, raising FlooAPIError on 4xx/5xx."""
        if response.status_code >= 400:
            try:
                body = response.json()
                detail = body.get("detail", body)
                if isinstance(detail, dict):
                    code = detail.get("code", "API_ERROR")
                    message = detail.get("message", response.text)
                else:
                    code = "API_ERROR"
                    message = str(detail)
            except Exception:
                code = "API_ERROR"
                message = response.text
            raise FlooAPIError(
                status_code=response.status_code,
                code=code,
                message=message,
            )
        return response.json()

    def register(self, email: str, password: str) -> dict[str, Any]:
        """Register a new user account."""
        resp = self._client.post(
            "/v1/auth/register",
            json={"email": email, "password": password},
        )
        return self._handle_response(resp)

    def login(self, email: str, password: str) -> dict[str, Any]:
        """Authenticate and get an API key."""
        resp = self._client.post(
            "/v1/auth/login",
            json={"email": email, "password": password},
        )
        return self._handle_response(resp)

    def create_app(self, name: str, runtime: str | None = None) -> dict[str, Any]:
        """Create a new app."""
        body: dict[str, str] = {"name": name}
        if runtime is not None:
            body["runtime"] = runtime
        resp = self._client.post("/v1/apps", json=body)
        return self._handle_response(resp)

    def list_apps(self) -> dict[str, Any]:
        """List all apps for the current user."""
        resp = self._client.get("/v1/apps")
        return self._handle_response(resp)

    def get_app(self, app_id: str) -> dict[str, Any]:
        """Get details of a specific app."""
        resp = self._client.get(f"/v1/apps/{app_id}")
        return self._handle_response(resp)

    def delete_app(self, app_id: str) -> dict[str, Any]:
        """Delete an app (soft delete)."""
        resp = self._client.delete(f"/v1/apps/{app_id}")
        return self._handle_response(resp)

    def create_deploy(
        self,
        app_id: str,
        tarball_path: Path,
        runtime: str,
        framework: str | None = None,
    ) -> dict[str, Any]:
        """Upload a tarball and create a deploy."""
        with open(tarball_path, "rb") as f:
            resp = self._client.post(
                f"/v1/apps/{app_id}/deploys",
                files={"file": (tarball_path.name, f, "application/gzip")},
                data={"runtime": runtime, "framework": framework or ""},
            )
        return self._handle_response(resp)

    def list_deploys(self, app_id: str) -> dict[str, Any]:
        """List all deploys for an app."""
        resp = self._client.get(f"/v1/apps/{app_id}/deploys")
        return self._handle_response(resp)

    def get_deploy(self, app_id: str, deploy_id: str) -> dict[str, Any]:
        """Get deploy status and details."""
        resp = self._client.get(f"/v1/apps/{app_id}/deploys/{deploy_id}")
        return self._handle_response(resp)

    def close(self) -> None:
        """Close the underlying HTTP client."""
        self._client.close()
