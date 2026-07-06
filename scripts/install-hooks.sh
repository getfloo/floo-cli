#!/usr/bin/env bash
# install-hooks.sh — point this checkout's git hooks at the tracked .githooks/
# directory (getfloo/floo-cli#217). Run once after cloning.
#
#   ./scripts/install-hooks.sh
#
# Installs a pre-push gate that runs ./scripts/test (fmt + clippy + cargo test)
# on Rust changes before a push, blocking on failure. Bypass an individual push
# with `git push --no-verify`.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
git -C "$ROOT" config core.hooksPath .githooks

echo "floo-cli git hooks installed (core.hooksPath = .githooks)."
echo "  pre-push runs ./scripts/test on Rust changes; bypass with 'git push --no-verify'."
