"""Manages ~/.floo/config.json for CLI credentials and settings."""

from __future__ import annotations

import json
import os
from pathlib import Path

from pydantic import BaseModel

from floo.constants import CONFIG_DIR_NAME, CONFIG_FILE_NAME, DEFAULT_API_URL


class FlooConfig(BaseModel):
    """CLI configuration stored in ~/.floo/config.json."""

    api_key: str | None = None
    api_url: str = DEFAULT_API_URL
    user_email: str | None = None


def _config_path() -> Path:
    """Return the path to the config file."""
    return Path.home() / CONFIG_DIR_NAME / CONFIG_FILE_NAME


def load_config() -> FlooConfig:
    """Load config from disk, returning defaults if file doesn't exist."""
    # Allow environment variable override for API URL
    env_api_url = os.environ.get("FLOO_API_URL")

    path = _config_path()
    if not path.exists():
        config = FlooConfig()
    else:
        data = json.loads(path.read_text())
        config = FlooConfig(**data)

    if env_api_url:
        config.api_url = env_api_url

    return config


def save_config(config: FlooConfig) -> None:
    """Save config to disk with restrictive permissions."""
    path = _config_path()
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(config.model_dump_json(indent=2))
    os.chmod(path, 0o600)


def clear_config() -> None:
    """Remove the config file."""
    path = _config_path()
    if path.exists():
        path.unlink()
