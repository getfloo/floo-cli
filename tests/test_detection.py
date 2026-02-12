"""Tests for runtime/framework detection."""

from __future__ import annotations

import json

from floo.detection import detect


def test_detect_nodejs_nextjs(tmp_path):
    """Detect Node.js with Next.js framework."""
    pkg = {"dependencies": {"next": "14.0.0", "react": "18.0.0"}}
    (tmp_path / "package.json").write_text(json.dumps(pkg))

    result = detect(tmp_path)
    assert result.runtime == "nodejs"
    assert result.framework == "Next.js"
    assert result.confidence == "high"


def test_detect_nodejs_vite(tmp_path):
    """Detect Node.js with Vite."""
    pkg = {"devDependencies": {"vite": "5.0.0"}}
    (tmp_path / "package.json").write_text(json.dumps(pkg))

    result = detect(tmp_path)
    assert result.runtime == "nodejs"
    assert result.framework == "Vite"
    assert result.confidence == "high"


def test_detect_nodejs_express(tmp_path):
    """Detect Node.js with Express."""
    pkg = {"dependencies": {"express": "4.18.0"}}
    (tmp_path / "package.json").write_text(json.dumps(pkg))

    result = detect(tmp_path)
    assert result.runtime == "nodejs"
    assert result.framework == "Express"
    assert result.confidence == "high"


def test_detect_python_fastapi(tmp_path):
    """Detect Python with FastAPI from requirements.txt."""
    (tmp_path / "requirements.txt").write_text("fastapi>=0.115\nuvicorn\n")

    result = detect(tmp_path)
    assert result.runtime == "python"
    assert result.framework == "FastAPI"
    assert result.confidence == "high"


def test_detect_python_flask(tmp_path):
    """Detect Python with Flask from requirements.txt."""
    (tmp_path / "requirements.txt").write_text("flask>=3.0\n")

    result = detect(tmp_path)
    assert result.runtime == "python"
    assert result.framework == "Flask"
    assert result.confidence == "high"


def test_detect_python_django(tmp_path):
    """Detect Python with Django from pyproject.toml."""
    content = '[project]\nname = "myapp"\ndependencies = ["django>=5.0"]\n'
    (tmp_path / "pyproject.toml").write_text(content)

    result = detect(tmp_path)
    assert result.runtime == "python"
    assert result.framework == "Django"
    assert result.confidence == "high"


def test_detect_go(tmp_path):
    """Detect Go project from go.mod."""
    (tmp_path / "go.mod").write_text("module example.com/myapp\n\ngo 1.22\n")

    result = detect(tmp_path)
    assert result.runtime == "go"
    assert result.version == "1.22"
    assert result.confidence == "high"


def test_detect_dockerfile(tmp_path):
    """Detect Dockerfile takes highest priority."""
    (tmp_path / "Dockerfile").write_text("FROM node:20\n")
    pkg = {"dependencies": {"next": "14.0.0"}}
    (tmp_path / "package.json").write_text(json.dumps(pkg))

    result = detect(tmp_path)
    assert result.runtime == "docker"
    assert result.confidence == "high"


def test_detect_static_html(tmp_path):
    """Detect static HTML site."""
    (tmp_path / "index.html").write_text("<html><body>Hello</body></html>")

    result = detect(tmp_path)
    assert result.runtime == "static"
    assert result.confidence == "low"


def test_detect_unknown(tmp_path):
    """Return unknown for empty directories."""
    result = detect(tmp_path)
    assert result.runtime == "unknown"
    assert result.confidence == "low"


def test_detection_result_to_dict(tmp_path):
    """Verify to_dict serialization."""
    pkg = {"dependencies": {"next": "14.0.0"}}
    (tmp_path / "package.json").write_text(json.dumps(pkg))

    result = detect(tmp_path)
    d = result.to_dict()
    assert d["runtime"] == "nodejs"
    assert d["framework"] == "Next.js"
    assert "confidence" in d
    assert "reason" in d
