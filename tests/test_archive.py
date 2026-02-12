"""Tests for source archive creation."""

from __future__ import annotations

import tarfile

from floo.archive import create_archive


def test_basic_archive(tmp_path):
    """Create an archive of a simple project."""
    (tmp_path / "index.js").write_text("console.log('hello')")
    (tmp_path / "package.json").write_text("{}")

    archive_path = create_archive(tmp_path)
    assert archive_path.exists()
    assert archive_path.suffix == ".gz"

    with tarfile.open(archive_path, "r:gz") as tar:
        names = tar.getnames()
        assert "index.js" in names
        assert "package.json" in names

    archive_path.unlink()


def test_git_excluded(tmp_path):
    """Ensure .git directory is excluded from archives."""
    (tmp_path / "index.js").write_text("console.log('hello')")
    git_dir = tmp_path / ".git"
    git_dir.mkdir()
    (git_dir / "HEAD").write_text("ref: refs/heads/main")

    archive_path = create_archive(tmp_path)

    with tarfile.open(archive_path, "r:gz") as tar:
        names = tar.getnames()
        assert not any(".git" in n for n in names)

    archive_path.unlink()


def test_node_modules_excluded(tmp_path):
    """Ensure node_modules is excluded from archives."""
    (tmp_path / "index.js").write_text("console.log('hello')")
    nm_dir = tmp_path / "node_modules"
    nm_dir.mkdir()
    (nm_dir / "foo.js").write_text("module.exports = {}")

    archive_path = create_archive(tmp_path)

    with tarfile.open(archive_path, "r:gz") as tar:
        names = tar.getnames()
        assert not any("node_modules" in n for n in names)

    archive_path.unlink()


def test_flooignore_patterns(tmp_path):
    """Respect .flooignore patterns."""
    (tmp_path / "index.js").write_text("console.log('hello')")
    (tmp_path / "secret.key").write_text("super-secret")
    (tmp_path / ".flooignore").write_text("# Ignore secrets\nsecret.key\n")

    archive_path = create_archive(tmp_path)

    with tarfile.open(archive_path, "r:gz") as tar:
        names = tar.getnames()
        assert "index.js" in names
        assert "secret.key" not in names

    archive_path.unlink()


def test_pyc_excluded(tmp_path):
    """Ensure .pyc files are excluded by default."""
    (tmp_path / "app.py").write_text("print('hello')")
    (tmp_path / "app.pyc").write_text("bytecode")

    archive_path = create_archive(tmp_path)

    with tarfile.open(archive_path, "r:gz") as tar:
        names = tar.getnames()
        assert "app.py" in names
        assert "app.pyc" not in names

    archive_path.unlink()
