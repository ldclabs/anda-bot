---
name: git-assistant
description: Inspect Git history and diffs, prepare scoped commits, resolve merge or rebase conflicts, and write changelogs for a specified revision range.
license: MIT
compatibility: Git on the host and shell access to the target repository.
---

# Git assistant

Use `shell` in the repository selected for this Anda session. Confirm the root and branch with `git rev-parse --show-toplevel` and `git status --short --branch`. Inspect both `git diff` and `git diff --cached` on the affected paths. A requested commit may share a worktree or index with someone else's edits.

- **Commit.** Stage the requested files or hunks. Review the staged diff, run relevant checks and `git diff --cached --check`. Include any attribution the user requested and report the branch and short commit ID. A request for a commit message only needs the proposed message.
- **Conflicts.** Use `git diff --name-only --diff-filter=U` to find unresolved paths. Compare the base, both sides and their callers before editing. During a rebase, “ours” is the branch being replayed onto. Verify the result and inspect the index for unresolved entries with `git ls-files -u` before continuing.
- **History and changelogs.** Establish the requested range, inspect the commits and their diffs, and group changes by user-visible effect. Attribute changes to the commits or tags that introduced them.
- **Shared history and publication.** Use the user's requested branch and remote. Push, publish, discard local work or rewrite shared history only when the request covers that action and its target. If intent is ambiguous, explain the specific refs or changes at stake. Use a lease for an authorized force push.

Report what Git actually changed and any remaining conflicts or failed checks.
