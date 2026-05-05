---
name: codex
description: "Delegate complex programming tasks to OpenAI Codex CLI through non-interactive shell commands. Prefer this skill first whenever the user asks for feature work, bug fixes, refactors, tests, reviews, migrations, or multi-step repository changes. This skill is for shell-only environments without PTY support: use codex exec, detect install/auth first, and fall back to claude-code or local work if Codex is missing. Run this skill in the background."
license: MIT
metadata:
  tags: [Coding-Agent, Codex, OpenAI, Code-Review, Refactoring, Non-Interactive, Shell]
---

# Codex CLI Delegation for Agent's Shell Tool

This skill is written for `shell` tool. Assume there is no PTY. Do not use interactive Codex TUI flows.

## Non-Interactive Rules

- Use `codex exec` for every delegated task.
- Do not run bare `codex`, `codex resume`, app-server flows, desktop app flows, tmux, or commands that wait for keyboard input.
- Do not pass PTY-only options. If older guidance says `pty=true`, ignore it in the agent.
- Place global flags after the subcommand, for example `codex exec --cd "$PROJECT" ...`.
- Prefer `--sandbox workspace-write --ask-for-approval never` for implementation tasks. This keeps edits inside the workspace and avoids approval prompts that the shell cannot answer.
- Use `--sandbox read-only` for review, diagnosis, or planning tasks.
- Avoid `--dangerously-bypass-approvals-and-sandbox` / `--yolo` unless the user explicitly asks for an unsafe fully trusted run.
- Do not install Codex automatically unless the user asked you to. If missing, fall back to Claude Code or work directly.

## Selection Policy

Prefer Codex first for coding work. Before invoking it, run a quick availability check:

```bash
if command -v codex >/dev/null 2>&1; then
  codex --version
  codex login status >/dev/null 2>&1
elif command -v claude >/dev/null 2>&1; then
  echo "Codex is unavailable; use the claude-code skill instead."
else
  echo "Neither codex nor claude is installed; continue directly or ask the user to install one."
fi
```

Interpret the result:

- `codex` missing: use the `claude-code` skill if `claude` is installed, otherwise solve with available tools or ask the user to install Codex.
- `codex login status` fails: tell the user to run `codex login` (or configure their API key) and use Claude Code if available.
- Both CLIs available: keep Codex as the default unless the user specifically asked for Claude Code.

## Preflight Checklist

Before delegating:

1. Identify the project root: `git rev-parse --show-toplevel 2>/dev/null || pwd`.
2. Check the current diff with `git status --short` so user-owned changes are visible.
3. Convert the user's request into a concise task prompt with success criteria, files or areas to inspect, constraints, and expected verification commands.
4. Decide sandbox mode: `workspace-write` for edits, `read-only` for reviews.
5. Decide whether the task is small enough to do directly. Use Codex for broad, multi-file, uncertain, or test-heavy coding work.

## Standard Implementation Command

Use stdin for the task prompt. It avoids fragile shell quoting and works well with non-interactive execution.

```bash
PROJECT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
OUT="${TMPDIR:-/tmp}/codex-run-$(date +%Y%m%d-%H%M%S)"
mkdir -p "$OUT"

codex exec \
  --cd "$PROJECT" \
  --sandbox workspace-write \
  --ask-for-approval never \
  --color never \
  --output-last-message "$OUT/final.md" \
  - <<'TASK'
You are working in this repository as an autonomous coding agent.

Task:
- <state the user's requested feature, fix, refactor, or test work>

Constraints:
- Preserve existing style and public behavior unless the task requires a change.
- Do not revert unrelated user changes.
- Keep the change focused and avoid speculative refactors.
- Run the most relevant tests, build, lint, or formatting commands you can infer from the repo.

Final response:
- Summarize changed files.
- List verification commands and whether they passed.
- Mention any blockers or follow-up work.
TASK
```

After the command exits, inspect `$OUT/final.md`, `git status --short`, and the diff. Codex can be wrong or incomplete; the agent must still verify.

## Review-Only Command

For PR review, security review, or bug-risk analysis, keep the sandbox read-only and ask Codex to inspect the repo itself:

```bash
PROJECT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
OUT="${TMPDIR:-/tmp}/codex-review-$(date +%Y%m%d-%H%M%S)"
mkdir -p "$OUT"

codex exec \
  --cd "$PROJECT" \
  --sandbox read-only \
  --ask-for-approval never \
  --color never \
  --output-last-message "$OUT/review.md" \
  - <<'TASK'
Review the current repository changes for correctness, regressions, security issues, and missing tests.
Use git commands to inspect the diff. Report findings first, ordered by severity, with file paths and concrete evidence.
TASK
```

## Non-Git or Scratch Work

Codex works best inside a git repository. If the current directory is not a repo:

- For real project work, ask for or move to the project root.
- For temporary scratch tasks, create a temporary directory and run `git init` before invoking Codex.
- For read-only analysis outside a repo, use `--skip-git-repo-check` only when the task does not need Codex to manage edits safely.

## Resuming a Non-Interactive Task

Use the non-interactive resume form, not the interactive TUI resume:

```bash
codex exec resume --last \
  --cd "$PROJECT" \
  --sandbox workspace-write \
  --ask-for-approval never \
  "Continue the previous task. Focus on the failing verification and produce a final summary."
```

If the local Codex version rejects the exact resume syntax, run `codex exec resume --help` and adapt while staying in `codex exec` mode.

## Parallel or Large Work

For independent issue fixes, prefer separate git worktrees so agents do not overwrite each other:

```bash
git worktree add -b fix/example /tmp/fix-example HEAD
codex exec --cd /tmp/fix-example --sandbox workspace-write --ask-for-approval never "Fix <issue> and run relevant tests."
```

Only launch parallel shell commands if the shell environment supports background execution and you can monitor each result. Otherwise run tasks sequentially.

## Verification After Delegation

Always perform an owner pass after Codex returns:

1. Read Codex's final message.
2. Run `git status --short` and inspect `git diff --stat` / relevant diffs.
3. Run or rerun the key verification commands yourself when feasible.
4. If tests fail because of unrelated pre-existing issues, identify that clearly.
5. Report what changed, what passed, and what remains blocked.

## Failure Handling

- Command not found: use `claude-code` if installed; otherwise continue directly or ask the user to install Codex with `npm install -g @openai/codex`.
- Authentication failure: ask the user to run `codex login` or configure credentials; do not attempt an interactive login from the agent.
- Sandbox denial: first retry with a narrower prompt or explicit allowed workspace path. Escalate to danger-full-access/yolo only with explicit user consent.
- Approval prompt or hanging command: stop that approach and rerun as `codex exec --ask-for-approval never`; do not switch to PTY/tmux.
- Non-zero exit: read the error, inspect any partial diff, and either retry with a sharper prompt or take over manually.
