---
name: workflow
description: Full task workflow — issue tracking, worktrees, implementation, verification, PR, and plan structure.
user-invocable: false
---

# Workflow Skill

> End-to-end task process for floo-cli development. Activates for any non-trivial implementation task. When this skill is active, follow every rule below.

## When to Activate

- Any implementation task: new command, bug fix, refactor, or chore
- Any task entering plan mode (3+ steps, architecture decisions, multi-file changes)
- NOT for pure questions, exploration, or docs-only changes

---

## Plan Structure

Every plan for a non-trivial task MUST start with these sections before technical details.

### Required Worktree Section

Every plan — features AND bug fixes — MUST include this section.

```
## Worktree

| Branch | Worktree Path |
|--------|---------------|
| `fix/20-url-encoding` | `.claude/worktrees/fix-20-url-encoding/` |
```

Branch naming: `feat/<id>-short-desc` or `fix/<id>-short-desc`. The worktree name uses `-`
instead of `/` (e.g., `feat/add-rollback` → `feat-add-rollback`); `EnterWorktree` handles
this naturally via its `name` parameter.
This section is filled in during planning (before approval), not during execution.

### Required Feature Scope Section (features only)

For any task that adds new functionality, include this section before technical details.
Skip for pure bug fixes and refactors (but Worktree is still required).

```
## Feature Scope

### Problem Statement
[What user problem does this solve? Why now? 2-3 sentences max.]

### What Changes for Users
[Concrete description — new commands, flags, output changes. Describe the experience.]

### Acceptance Criteria
- [ ] [Testable criterion — what must be true when done]
- [ ] ...

### Out of Scope
[What this does NOT include. Prevents scope creep.]
```

---

## Issue-Driven Workflow

CLI issues live in `getfloo/floo-cli` (this repo). API/infra issues live in `getfloo/floo`.

Before starting any task:

1. **Find the issue.** The user will provide the issue number, or query:
   `gh issue list --label "v0" --state open`
2. **Claim it.** Add the `status:in-progress` label:
   `gh issue edit <n> --add-label "status:in-progress"`
3. **Close it via PR.** The PR body must include `Closes #<n>` — GitHub auto-closes on merge.
   For cross-repo issues, use `Closes getfloo/floo#<n>`.

**Label taxonomy:**
- **Type:** `type:bug` · `type:feature` · `type:chore` · `type:docs`
- **Release:** `v0` (launch target) · `v1` (post-launch)
- **Priority:** `priority:critical` · `priority:high` · `priority:medium` · `priority:low`
- **Component:** `component:cli`
- **Status:** `status:in-progress` (agent actively working) · `status:blocked` (waiting on dependency)

---

## Git Worktree Workflow

**ALWAYS use git worktrees for feature development.** Never commit feature work directly on
`main`. Multiple features may be developed in parallel, so `main` must stay clean. Worktrees
live under `.claude/worktrees/` (gitignored) — Claude Code's native path.

```bash
# 0. Preflight: pull latest main
git checkout main && git pull

# 1. Create worktree — Claude Code creates worktree, switches session CWD
EnterWorktree "fix-20-url-encoding"
```

---

## Execution Steps (after plan approval)

**0. Claim the issue.** `gh issue edit <n> --add-label "status:in-progress"`

**1. Create worktree.** Create the branch and worktree exactly as declared in the plan's
Worktree table. Use `EnterWorktree "<name>"` to create the worktree and switch the session
CWD. Work exclusively in `.claude/worktrees/<name>/` — never the main checkout.

**2. Implement.** Write code following all CLAUDE.md conventions and activated skill rules.
Commit incrementally with conventional commits (`feat:`, `fix:`, `test:`, etc.).

**Bug handling:** Small-to-medium bugs → fix directly and note in the commit message. Larger
bugs (security issues, scope beyond the current task) → file a GitHub issue and reference it
in the PR body. Do not expand scope mid-implementation; file the issue and keep going.

**3. Verify (MANDATORY — never skip).** Run ALL checks. Every single check must pass.
No exceptions, no "I'll run them later", no skipping because "the change is small."

```bash
cargo test && cargo clippy -- -D warnings && cargo fmt --check
```

When in doubt, run more tests rather than fewer. Skipping tests that should have been run is
a serious error.

**4. Review (MANDATORY — never skip).** Run `pr-review-toolkit:code-reviewer` and
`pr-review-toolkit:silent-failure-hunter` in parallel on the diff. Fix ALL findings —
every single one, regardless of severity. Then re-run Step 3 (all tests must still pass).
Run the Code Review Checklist below.

**5. PR (MANDATORY).** Push the branch and open a PR — this is required to close the loop on
every task.

```
gh pr create --title "<title>" --body "$(cat <<'EOF'
## Summary
<1-3 bullet points>

Closes #<issue-number>

## Changes
<files changed and why>

## Test plan
- [ ] <test scenarios run>

## Discovered issues
<list issue links filed during implementation, or "None">

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

**6. Update project memory.** If any corrections, surprises, or new patterns emerged:
update MEMORY.md or the relevant skill file so the same mistake doesn't recur.

---

## Code Review Checklist

After completing any implementation task, run this checklist before committing. Not optional.

**Correctness (build phase — check every item):**
- [ ] No fallback values (`unwrap_or_default`, `unwrap_or("")`) on paths that should always succeed
- [ ] No bare `catch`/`match` arms that eat errors silently — every error must propagate via `?` or return a typed `Err`
- [ ] Failures propagate to the caller — non-zero exit, `Err(...)` — never silently succeed with wrong data

**Tests & Quality:**
- [ ] All tests pass (`cargo test`)
- [ ] Lint is clean (`cargo clippy -- -D warnings`)
- [ ] Format is clean (`cargo fmt --check`)
- [ ] New commands have tests covering: happy path, auth errors, not found

**Style & Conventions:**
- [ ] No `println!` — use `output` module functions
- [ ] No `unwrap()` in production paths — use `?` operator
- [ ] `--json` works on any new/modified command
- [ ] Error codes are UPPER_SNAKE_CASE and added to the reference list

**Security:**
- [ ] No hardcoded secrets, URLs, or API keys
- [ ] All HTTP calls via `FlooClient`, never direct `reqwest`
- [ ] Input validation on all user-facing parameters

**Architecture:**
- [ ] Changes follow the patterns in the activated skill(s)
- [ ] No new patterns introduced that conflict with existing conventions
- [ ] If a convention changed, the relevant skill was updated too

**Before creating a PR:** Run `pr-review-toolkit:code-reviewer` and
`pr-review-toolkit:silent-failure-hunter` in parallel on the diff. Fix ALL findings.
Re-run all tests after fixes.

---

## Orchestration Principles

**Plan Mode:** Enter plan mode for any non-trivial task (3+ steps, architecture decisions,
multi-file changes). If implementation goes sideways, STOP and re-plan — don't keep pushing.

**Subagent Strategy:** Offload research, exploration, and parallel analysis to subagents.
Use Explore for codebase discovery, Plan for design, pr-review-toolkit agents before every PR.
One focused task per subagent. Keep the main context window clean.

**Verification Before Done:** Never mark a task complete without proving it works. Run tests,
check logs, demonstrate correctness. Ask: "Would a staff engineer approve this?"

**Self-Improvement Loop:** After any correction from the user, update MEMORY.md or the
relevant skill file with the pattern so the same mistake doesn't recur.
