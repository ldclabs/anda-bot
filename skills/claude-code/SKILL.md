---
name: claude-code
description: "Delegate complex programming tasks to Claude Code CLI through non-interactive shell commands when Codex is unavailable, unauthenticated, unsuitable, or explicitly not desired. Use this skill for feature work, bug fixes, refactors, tests, reviews, migrations, and multi-step repository changes in shell-only environments. It requires claude -p; never use interactive Claude, tmux, slash commands, or PTY-dependent workflows. Run this skill in the background."
license: MIT
metadata:
  tags: [Coding-Agent, Claude, Anthropic, Code-Review, Refactoring, Non-Interactive, Shell]
---

# Claude Code CLI Delegation for Agent's Shell Tool

This skill is written for `shell` tool. Assume there is no PTY. Do not use the interactive Claude TUI.

## Selection Policy

Codex is preferred for programming delegation. Before invoking Claude Code, check whether Codex can handle the task:

```bash
if command -v codex >/dev/null 2>&1 && codex login status >/dev/null 2>&1; then
  echo "Prefer the codex skill for this coding task."
elif command -v claude >/dev/null 2>&1; then
  claude --version
  claude auth status --text
else
  echo "Neither codex nor claude is installed; continue directly or ask the user to install one."
fi
```

Use Claude Code when:

- `codex` is not installed or not authenticated.
- Codex failed on a task where a second coding agent is useful.
- The user explicitly asks for Claude Code.
- The repo or task is already configured around Claude Code conventions.

If `claude auth status --text` fails, ask the user to run `claude auth login`, `claude auth login --console`, or configure `ANTHROPIC_API_KEY`. Do not start an interactive login flow from the agent.

## Non-Interactive Rules

- Use `claude -p` for every delegated task.
- Do not run bare `claude`, `claude "query"`, tmux, slash commands, `/review`, `/compact`, interactive permission flows, or anything that waits for keyboard input.
- Do not use `--tmux`, `-w --tmux`, or PTY orchestration from older Claude Code guides.
- Pass the task through stdin or a carefully quoted prompt. Stdin is safer for long task descriptions.
- Set `--max-turns` so the agent cannot run indefinitely.
- Use `--allowedTools` to make non-interactive tool use explicit. Keep tools narrower for review-only work.
- Avoid `--dangerously-skip-permissions` unless the user explicitly approves a fully trusted run.

## Preflight Checklist

Before delegating:

1. Identify the project root: `git rev-parse --show-toplevel 2>/dev/null || pwd`.
2. Check `git status --short` and preserve unrelated user changes.
3. Turn the user's request into clear success criteria, constraints, and verification commands.
4. Choose the tool list: broad for implementation, read-only for review.
5. Prefer Codex if it is available and authenticated.

## Standard Implementation Command

Use this pattern for feature work, bug fixes, refactors, or tests:

```bash
PROJECT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
OUT="${TMPDIR:-/tmp}/claude-code-run-$(date +%Y%m%d-%H%M%S)"
mkdir -p "$OUT"

cd "$PROJECT" || exit 1
cat <<'TASK' | claude -p "Execute the coding task described on stdin. Make focused repository changes, run relevant verification, and finish with a concise summary." \
  --allowedTools "Read,Edit,Write,Bash,Glob,Grep,EnterPlanMode,EnterWorktree,Skill" \
  --output-format json \
  --max-turns 42 \
  --fallback-model haiku \
  > "$OUT/result.json"
Task:
- <state the user's requested feature, fix, refactor, or test work>

Constraints:
- Preserve existing style and public behavior unless the task requires a change.
- Do not revert unrelated user changes.
- Keep the change focused and avoid speculative refactors.
- Run relevant tests, build, lint, or formatting commands when you can infer them.

Final response:
- Summarize changed files.
- List verification commands and pass/fail status.
- Mention blockers or follow-up work.
TASK
```

Then inspect `$OUT/result.json`, `git status --short`, and the diff. A successful Claude result is useful evidence, not final proof.

If the local Claude Code version does not support `--fallback-model`, rerun without that flag.

## Review-Only Command

For code review or diagnosis, restrict tools:

```bash
PROJECT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
OUT="${TMPDIR:-/tmp}/claude-code-review-$(date +%Y%m%d-%H%M%S)"
mkdir -p "$OUT"

cd "$PROJECT" || exit 1
cat <<'TASK' | claude -p "Review the repository changes described on stdin. Use git commands as needed and report findings first." \
  --allowedTools "Read,Bash,Glob,Grep" \
  --output-format json \
  --max-turns 8 \
  > "$OUT/review.json"
Review current changes for correctness, regressions, security issues, and missing tests.
Return findings ordered by severity with concrete file paths and evidence.
TASK
```

Use a narrower `--allowedTools` set if the repo has known commands. Use broader read-only Bash only when needed for discovery.

## Session Continuation

Prefer fresh print-mode tasks. If you need to continue a previous Claude Code task, stay non-interactive:

```bash
claude -c -p "Continue the previous coding task. Focus on the remaining verification failure and produce a final summary." \
  --allowedTools "Read,Edit,Write,Bash,Glob,Grep,EnterPlanMode,EnterWorktree,Skill" \
  --output-format json \
  --max-turns 42
```

For a specific session, use `claude -r <session-id> -p "..."` if supported by the installed version. If the command asks for interactive selection, stop and use `-c` or start a fresh print-mode task instead.

## Structured Output Handling

`--output-format json` returns a single JSON result. Useful fields include:

- `subtype`: `success`, `error_max_turns`, `error_budget`, or another failure subtype.
- `result`: Claude's final natural-language summary.
- `session_id`: save this if you may need a continuation.
- `num_turns`, `duration_ms`, `total_cost_usd`: useful for reporting and budget awareness.

If `jq` is available, inspect the final message with:

```bash
jq -r '.result // empty' "$OUT/result.json"
```

If `jq` is not available, read the JSON with the available shell tools or Python.

## Verification After Delegation

Always perform an owner pass after Claude returns:

1. Check the JSON subtype and final result.
2. Run `git status --short` and inspect relevant diffs.
3. Run or rerun the key verification commands yourself when feasible.
4. If Claude made unrelated changes, revert only the changes from Claude that are clearly outside the task; do not revert user-owned edits.
5. Report changed files, verification status, and remaining blockers.

## Failure Handling

- Command not found: use Codex if available; otherwise continue directly or ask the user to install Claude Code with `npm install -g @anthropic-ai/claude-code`.
- Authentication failure: ask the user to run the appropriate `claude auth login` command; do not launch an interactive login.
- Tool permission failure: retry with a more explicit `--allowedTools` list that covers the needed safe commands. Do not switch to the interactive TUI.
- `error_max_turns`: inspect partial changes, then rerun with a sharper prompt or a modestly higher `--max-turns`.
- Non-zero exit or malformed JSON: read stderr/stdout, inspect partial diffs, and decide whether to retry, use Codex, or take over manually.
- Hanging command: stop the run if possible and relaunch in print mode with stricter prompt and tool limits; never move to tmux/PTY.
