#!/usr/bin/env bash
# Tests for .githooks/pre-push (getfloo/floo-cli#217): a Rust change runs
# ./scripts/test (block on fail, allow on pass); docs-only pushes skip the
# suite; branch deletions are no-ops.
set -euo pipefail

HOOK_SRC="$(cd "$(dirname "$0")/.." && pwd)/.githooks/pre-push"
ZERO='0000000000000000000000000000000000000000'
PASS=0
FAIL=0

check() {  # label  got  want
  if [ "$2" = "$3" ]; then
    printf '  ok   %s\n' "$1"; PASS=$((PASS + 1))
  else
    printf '  FAIL %s (got rc=%s, want rc=%s)\n' "$1" "$2" "$3" >&2; FAIL=$((FAIL + 1))
  fi
}

# make_repo TEST_EXIT — a git repo with the hook installed and a scripts/test
# stub that exits TEST_EXIT. Echoes the repo dir. Leaves an init commit.
make_repo() {
  local d; d=$(mktemp -d)
  git -C "$d" init -q
  git -C "$d" config user.email t@e.com
  git -C "$d" config user.name t
  git -C "$d" config commit.gpgsign false
  mkdir -p "$d/.githooks" "$d/scripts" "$d/src"
  cp "$HOOK_SRC" "$d/.githooks/pre-push"; chmod +x "$d/.githooks/pre-push"
  printf '#!/usr/bin/env bash\nexit %s\n' "$1" > "$d/scripts/test"; chmod +x "$d/scripts/test"
  printf 'fn main() {}\n' > "$d/src/main.rs"
  printf '# floo-cli\n' > "$d/README.md"
  git -C "$d" add -A
  git -C "$d" commit -qm init
  printf '%s' "$d"
}

# run_hook REPO LOCAL_SHA REMOTE_SHA — feed the pre-push stdin line and echo the
# hook's exit code. Stdin format: <local ref> <local sha> <remote ref> <remote sha>.
run_hook() {
  local rc=0
  printf 'refs/heads/feat %s refs/heads/feat %s\n' "$2" "$3" \
    | ( cd "$1" && ./.githooks/pre-push ) >/dev/null 2>&1 || rc=$?
  printf '%s' "$rc"
}

echo "pre-push hook tests"

# 1. Rust change + passing suite → allow (rc 0).
d=$(make_repo 0); base=$(git -C "$d" rev-parse HEAD)
printf 'fn main() { let _ = 1; }\n' > "$d/src/main.rs"; git -C "$d" add -A && git -C "$d" commit -qm "feat: rust"
head=$(git -C "$d" rev-parse HEAD)
check "rust change, suite passes -> allow" "$(run_hook "$d" "$head" "$base")" "0"
rm -rf "$d"

# 2. Rust change + failing suite → block (rc 1).
d=$(make_repo 1); base=$(git -C "$d" rev-parse HEAD)
printf 'fn main() { let _ = 2; }\n' > "$d/src/main.rs"; git -C "$d" add -A && git -C "$d" commit -qm "feat: rust"
head=$(git -C "$d" rev-parse HEAD)
check "rust change, suite fails -> block" "$(run_hook "$d" "$head" "$base")" "1"
rm -rf "$d"

# 3. Docs-only change + failing suite → allow (suite not run).
d=$(make_repo 1); base=$(git -C "$d" rev-parse HEAD)
printf '# floo-cli\nmore docs\n' > "$d/README.md"; git -C "$d" add -A && git -C "$d" commit -qm "docs: readme"
head=$(git -C "$d" rev-parse HEAD)
check "docs-only change -> skip suite (allow)" "$(run_hook "$d" "$head" "$base")" "0"
rm -rf "$d"

# 4. Cargo.toml change is Rust/build → gated.
d=$(make_repo 1); base=$(git -C "$d" rev-parse HEAD)
printf '[package]\nname="x"\n' > "$d/Cargo.toml"; git -C "$d" add -A && git -C "$d" commit -qm "build: cargo.toml"
head=$(git -C "$d" rev-parse HEAD)
check "Cargo.toml change, suite fails -> block" "$(run_hook "$d" "$head" "$base")" "1"
rm -rf "$d"

# 5. Branch deletion (local sha = zeros) → no-op (allow), suite never runs.
d=$(make_repo 1); base=$(git -C "$d" rev-parse HEAD)
check "branch deletion -> no-op (allow)" "$(run_hook "$d" "$ZERO" "$base")" "0"
rm -rf "$d"

# 6. New branch (remote sha = zeros) with no origin/main → force a run. Rust or
#    not, the base is unknown, so the suite MUST run (fail-closed). Suite fails.
d=$(make_repo 1)
printf '# docs only\n' > "$d/README.md"; git -C "$d" add -A && git -C "$d" commit -qm "docs"
head=$(git -C "$d" rev-parse HEAD)
check "new branch, unknown base -> force run (block)" "$(run_hook "$d" "$head" "$ZERO")" "1"
rm -rf "$d"

# 7. FAIL-CLOSED on an unresolvable range: an existing-branch update whose
#    remote_sha is a commit not in the local object DB (force-push from a stale
#    clone) must run the suite, not silently skip. Suite fails → block.
d=$(make_repo 1)
printf 'fn main() { let _ = 3; }\n' > "$d/src/main.rs"; git -C "$d" add -A && git -C "$d" commit -qm "feat: rust"
head=$(git -C "$d" rev-parse HEAD)
bogus="deadbeefdeadbeefdeadbeefdeadbeefdeadbeef"
check "unresolvable range (unknown remote sha) -> force run (block)" "$(run_hook "$d" "$head" "$bogus")" "1"
rm -rf "$d"

# 8. Rust file renamed AWAY (src/main.rs -> src/main.bak): --no-renames surfaces
#    the removed .rs path, so the suite still runs. Suite fails → block.
d=$(make_repo 1); base=$(git -C "$d" rev-parse HEAD)
git -C "$d" mv src/main.rs src/main.bak; git -C "$d" commit -qm "refactor: rename away rust"
head=$(git -C "$d" rev-parse HEAD)
check "rust renamed away -> suite still runs (block)" "$(run_hook "$d" "$head" "$base")" "1"
rm -rf "$d"

echo
printf "  %d passed, %d failed\n" "$PASS" "$FAIL"
[ "$FAIL" -eq 0 ] || exit 1
echo "✓ pre-push hook tests green"
