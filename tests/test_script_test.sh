#!/usr/bin/env bash
# Tests for scripts/test — the corrupt-harness guard (floo-cli#204).
#
# Uses a cargo shim (via FLOO_TEST_CARGO) so no real build runs. Each case
# pins one branch of the guard: pass-through success, heal-and-retry on the
# corrupt-harness signature, no retry on a normal failure, and loud failure
# when the corruption persists.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
WRAPPER="${REPO_ROOT}/scripts/test"

pass() {
    echo "[pass] $*"
}

fail() {
    echo "[fail] $*" >&2
    exit 1
}

make_shim() {
    local dir="$1"
    : >"${dir}/calls"
    cat >"${dir}/cargo" <<'SHIM'
#!/usr/bin/env bash
dir="$(cd "$(dirname "$0")" && pwd)"
cmd="${1:-}"
echo "$cmd" >>"${dir}/calls"
case "$cmd" in
    fmt | clippy | clean)
        exit 0
        ;;
    test)
        n="$(grep -c '^test$' "${dir}/calls")"
        mode="$(cat "${dir}/mode")"
        case "$mode" in
            ok)
                echo "test result: ok. 1 passed"
                exit 0
                ;;
            corrupt-once)
                if [ "$n" -eq 1 ]; then
                    echo "Usage: floo-a45172a3dc4b14e2 [OPTIONS] <COMMAND>"
                    echo "error: test failed, to rerun pass \`--bin floo\`"
                    echo "note: test exited abnormally; to see the full output pass --no-capture to the harness."
                    exit 2
                fi
                echo "test result: ok. 1 passed"
                exit 0
                ;;
            corrupt-always)
                echo "Usage: floo-a45172a3dc4b14e2 [OPTIONS] <COMMAND>"
                echo "note: test exited abnormally; to see the full output pass --no-capture to the harness."
                exit 2
                ;;
            plain-failure)
                echo "test result: FAILED. 1 failed; 42 passed"
                echo "error: test failed, to rerun pass \`--bin floo\`"
                exit 101
                ;;
        esac
        ;;
esac
exit 0
SHIM
    chmod +x "${dir}/cargo"
}

run_wrapper() {
    local dir="$1"
    set +e
    FLOO_TEST_CARGO="${dir}/cargo" bash "$WRAPPER" >"${dir}/out" 2>&1
    WRAPPER_STATUS=$?
    set -e
}

clean_calls() {
    grep -c '^clean$' "$1/calls" || true
}

test_calls() {
    grep -c '^test$' "$1/calls" || true
}

# --- Case 1: success passes through, no clean, single test run ---
dir="$(mktemp -d)"
make_shim "$dir"
echo ok >"${dir}/mode"
run_wrapper "$dir"
[ "$WRAPPER_STATUS" -eq 0 ] || fail "success case: expected exit 0, got $WRAPPER_STATUS"
[ "$(clean_calls "$dir")" -eq 0 ] || fail "success case: clean must not run"
[ "$(test_calls "$dir")" -eq 1 ] || fail "success case: exactly one test run"
pass "success passes through without clean or retry"
rm -rf "$dir"

# --- Case 2: corrupt-harness signature heals — clean once, retry once, exit 0 ---
dir="$(mktemp -d)"
make_shim "$dir"
echo corrupt-once >"${dir}/mode"
run_wrapper "$dir"
[ "$WRAPPER_STATUS" -eq 0 ] || fail "heal case: expected exit 0, got $WRAPPER_STATUS"
[ "$(clean_calls "$dir")" -eq 1 ] || fail "heal case: expected exactly one clean"
[ "$(test_calls "$dir")" -eq 2 ] || fail "heal case: expected exactly two test runs"
grep -q 'corrupt test-harness binary detected' "${dir}/out" ||
    fail "heal case: warning must name the condition"
pass "corrupt-harness signature triggers one clean + one retry, then succeeds"
rm -rf "$dir"

# --- Case 3: a normal test failure is NEVER retried or cleaned ---
dir="$(mktemp -d)"
make_shim "$dir"
echo plain-failure >"${dir}/mode"
run_wrapper "$dir"
[ "$WRAPPER_STATUS" -ne 0 ] || fail "plain failure case: must exit nonzero"
[ "$(clean_calls "$dir")" -eq 0 ] || fail "plain failure case: clean must not run"
[ "$(test_calls "$dir")" -eq 1 ] || fail "plain failure case: no retry"
pass "normal test failure exits nonzero with no clean and no retry"
rm -rf "$dir"

# --- Case 4: persistent corruption fails loudly after exactly one retry ---
dir="$(mktemp -d)"
make_shim "$dir"
echo corrupt-always >"${dir}/mode"
run_wrapper "$dir"
[ "$WRAPPER_STATUS" -ne 0 ] || fail "persistent case: must exit nonzero"
[ "$(clean_calls "$dir")" -eq 1 ] || fail "persistent case: exactly one clean (no loop)"
[ "$(test_calls "$dir")" -eq 2 ] || fail "persistent case: exactly two test runs (no loop)"
pass "persistent corruption fails loudly after a single retry"
rm -rf "$dir"

echo "All scripts/test guard tests passed."
