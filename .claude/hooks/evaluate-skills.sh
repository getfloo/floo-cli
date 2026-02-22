#!/usr/bin/env bash
# evaluate-skills.sh — UserPromptSubmit hook
# Forces the coding agent to evaluate all skills and activate relevant ones.
# Outputs additionalContext before processing the user's prompt.

cat <<'SKILLS_CONTEXT'
## Skill Evaluation Protocol

Before proceeding with this task, evaluate each skill below. For each one, decide YES or NO based on whether the task involves that domain. Then activate the relevant skills by reading their SKILL.md files.

### Background Skills (auto-activate when relevant)

1. **rust** (`.claude/skills/rust/SKILL.md`)
   Activate: YES if the task touches Rust code, CLI commands, output module, API client, or any file in `src/`.
   - Adding new CLI commands → must support `--json` mode, use output contract
   - Working with errors → use correct error type (FlooError vs FlooApiError vs anyhow)
   - Adding error codes → add to the Error Code Reference table in the skill

2. **workflow** (`.claude/skills/workflow/SKILL.md`)
   Activate: YES if the task is any non-trivial implementation (new command, bug fix, or chore). Provides worktree setup, plan structure, execution steps, code review checklist, and PR process. NOT needed for pure questions, exploration, or docs-only changes.

### Protocol

1. **Evaluate:** For each skill, output a one-line `[skill]: YES/NO — reason`.
2. **Activate:** For each YES skill, read its SKILL.md file. The rules in those files override general conventions for their domain.
3. **Implement:** Proceed with the task, following all activated skill rules.

If no skills are relevant (e.g., git operations, docs-only changes, general questions), skip activation and proceed normally.

### Worktree Preflight Protocol

Before any mutating action (file edits, git commits, branch changes), include:
1. `Worktree path: <path>` under `.claude/worktrees/<name>/`
2. `Main checkout untouched: yes`

If not already in a worktree, create one first with `EnterWorktree "<name>"`. Never implement feature work directly in the main checkout.
Worktree names use `-` instead of `/` (e.g., `feat/add-rollback` → `.claude/worktrees/feat-add-rollback/`).

### Response Contract (Coding Tasks)

For coding tasks, include a short preflight block at the start of responses:
- `Skills evaluated: ...`
- `Skills activated: ...`
- `Worktree path: ...`
- `Main checkout untouched: yes/no`

### Post-Implementation Protocol

After completing implementation work, follow the Code Review Checklist and PR process in the `workflow` skill.
SKILLS_CONTEXT
