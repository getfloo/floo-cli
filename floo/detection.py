"""Auto-detect project runtime and framework from source files."""

from __future__ import annotations

import json
from dataclasses import dataclass
from pathlib import Path
from typing import Any


@dataclass
class DetectionResult:
    """Result of project runtime/framework detection."""

    runtime: str
    framework: str | None
    version: str | None
    confidence: str  # "high", "medium", "low"
    reason: str

    def to_dict(self) -> dict[str, Any]:
        """Serialize to a dictionary."""
        return {
            "runtime": self.runtime,
            "framework": self.framework,
            "version": self.version,
            "confidence": self.confidence,
            "reason": self.reason,
        }


def _detect_dockerfile(path: Path) -> DetectionResult | None:
    """Check for a Dockerfile."""
    if (path / "Dockerfile").exists():
        return DetectionResult(
            runtime="docker",
            framework=None,
            version=None,
            confidence="high",
            reason="Dockerfile found",
        )
    return None


def _detect_nodejs(path: Path) -> DetectionResult | None:
    """Check for a Node.js project and detect framework."""
    pkg_path = path / "package.json"
    if not pkg_path.exists():
        return None

    try:
        pkg = json.loads(pkg_path.read_text())
    except (json.JSONDecodeError, OSError):
        return DetectionResult(
            runtime="nodejs",
            framework=None,
            version=None,
            confidence="medium",
            reason="package.json found but could not be parsed",
        )

    deps: dict[str, str] = {}
    deps.update(pkg.get("dependencies", {}))
    deps.update(pkg.get("devDependencies", {}))

    frameworks = [
        ("next", "Next.js"),
        ("vite", "Vite"),
        ("express", "Express"),
        ("fastify", "Fastify"),
    ]

    for dep_name, framework_name in frameworks:
        if dep_name in deps:
            return DetectionResult(
                runtime="nodejs",
                framework=framework_name,
                version=deps.get(dep_name),
                confidence="high",
                reason=f"package.json contains {dep_name} dependency",
            )

    return DetectionResult(
        runtime="nodejs",
        framework=None,
        version=None,
        confidence="medium",
        reason="package.json found",
    )


def _detect_python(path: Path) -> DetectionResult | None:
    """Check for a Python project and detect framework."""
    # Check pyproject.toml
    pyproject_path = path / "pyproject.toml"
    if pyproject_path.exists():
        content = pyproject_path.read_text()
        frameworks = [
            ("fastapi", "FastAPI"),
            ("flask", "Flask"),
            ("django", "Django"),
        ]
        for dep_name, framework_name in frameworks:
            if dep_name in content.lower():
                return DetectionResult(
                    runtime="python",
                    framework=framework_name,
                    version=None,
                    confidence="high",
                    reason=f"pyproject.toml references {dep_name}",
                )
        return DetectionResult(
            runtime="python",
            framework=None,
            version=None,
            confidence="medium",
            reason="pyproject.toml found",
        )

    # Check requirements.txt
    req_path = path / "requirements.txt"
    if req_path.exists():
        content = req_path.read_text().lower()
        frameworks = [
            ("fastapi", "FastAPI"),
            ("flask", "Flask"),
            ("django", "Django"),
        ]
        for dep_name, framework_name in frameworks:
            if dep_name in content:
                return DetectionResult(
                    runtime="python",
                    framework=framework_name,
                    version=None,
                    confidence="high",
                    reason=f"requirements.txt contains {dep_name}",
                )
        return DetectionResult(
            runtime="python",
            framework=None,
            version=None,
            confidence="medium",
            reason="requirements.txt found",
        )

    return None


def _detect_go(path: Path) -> DetectionResult | None:
    """Check for a Go project."""
    gomod_path = path / "go.mod"
    if not gomod_path.exists():
        return None

    version = None
    for line in gomod_path.read_text().splitlines():
        if line.startswith("go "):
            version = line.split(" ", 1)[1].strip()
            break

    return DetectionResult(
        runtime="go",
        framework=None,
        version=version,
        confidence="high",
        reason="go.mod found",
    )


def _detect_static(path: Path) -> DetectionResult | None:
    """Check for a static HTML site."""
    if (path / "index.html").exists():
        return DetectionResult(
            runtime="static",
            framework=None,
            version=None,
            confidence="low",
            reason="index.html found with no backend markers",
        )
    return None


def detect(path: Path) -> DetectionResult:
    """Detect the runtime and framework of a project.

    Detection priority: Dockerfile > package.json > pyproject.toml/requirements.txt
    > go.mod > index.html > unknown.
    """
    detectors = [
        _detect_dockerfile,
        _detect_nodejs,
        _detect_python,
        _detect_go,
        _detect_static,
    ]

    for detector in detectors:
        result = detector(path)
        if result is not None:
            return result

    return DetectionResult(
        runtime="unknown",
        framework=None,
        version=None,
        confidence="low",
        reason="No recognized project files found",
    )
