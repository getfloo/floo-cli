"""Create gzipped tarballs of project source, respecting .flooignore."""

from __future__ import annotations

import fnmatch
import os
import tarfile
import tempfile
from pathlib import Path

from floo.constants import MAX_ARCHIVE_SIZE_MB
from floo.errors import FlooError

DEFAULT_IGNORE_PATTERNS = [
    ".git",
    "node_modules",
    "__pycache__",
    ".venv",
    "venv",
    ".env",
    "*.pyc",
    ".DS_Store",
]


def _load_flooignore(path: Path) -> list[str]:
    """Load additional ignore patterns from .flooignore file."""
    ignore_file = path / ".flooignore"
    if not ignore_file.exists():
        return []

    patterns: list[str] = []
    for line in ignore_file.read_text().splitlines():
        line = line.strip()
        if line and not line.startswith("#"):
            patterns.append(line)
    return patterns


def _should_ignore(name: str, patterns: list[str]) -> bool:
    """Check if a file/directory name matches any ignore pattern."""
    basename = os.path.basename(name)
    for pattern in patterns:
        if fnmatch.fnmatch(basename, pattern) or fnmatch.fnmatch(name, pattern):
            return True
    return False


def create_archive(path: Path) -> Path:
    """Create a gzipped tarball of the project at the given path.

    Returns the path to the created .tar.gz file.
    """
    patterns = DEFAULT_IGNORE_PATTERNS + _load_flooignore(path)

    fd, tmp_name = tempfile.mkstemp(suffix=".tar.gz")
    os.close(fd)
    archive_path = Path(tmp_name)

    with tarfile.open(archive_path, "w:gz") as tar:
        for root, dirs, files in os.walk(path):
            # Filter directories in-place to skip ignored dirs
            rel_root = os.path.relpath(root, path)
            dirs[:] = [
                d
                for d in dirs
                if not _should_ignore(d, patterns)
                and not _should_ignore(os.path.join(rel_root, d), patterns)
            ]

            for file in files:
                rel_path = os.path.relpath(os.path.join(root, file), path)
                if not _should_ignore(file, patterns) and not _should_ignore(rel_path, patterns):
                    tar.add(os.path.join(root, file), arcname=rel_path)

    size_mb = archive_path.stat().st_size / (1024 * 1024)
    if size_mb > MAX_ARCHIVE_SIZE_MB:
        archive_path.unlink()
        raise FlooError(
            code="ARCHIVE_TOO_LARGE",
            message=f"Archive is {size_mb:.0f}MB, exceeding the {MAX_ARCHIVE_SIZE_MB}MB limit.",
            suggestion="Add large files to .flooignore to reduce archive size.",
        )

    return archive_path
