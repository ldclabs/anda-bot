---
name: handoff
description: Write a handoff when the user wants a fresh session or another agent to continue the current conversation, with the objective, verified state and next step.
metadata:
  policy:
    allow_implicit_invocation: false
---

# Write a handoff

Use the current conversation, including the user's latest corrections. For example, `/skill handoff continue the migration` asks for a handoff focused on the migration.

- State the objective, completion criteria and working directory. Include the branch and commit when relevant.
- Record the decisions made, completed work, and checks actually run with their outcomes. Link to existing plans, issues and artifacts by absolute path or URL.
- List unfinished work, pending approvals or background jobs, and the first concrete step for the next session. Suggest relevant installed skills by name when they would help.
- Include only the context needed to resume. Omit credentials, secret values and unrelated personal details.

Use the requested destination, or create a uniquely named Markdown file in the OS temporary directory with the available file or shell tools. Return the exact absolute path so the next session can open it. If this channel has no file access, provide the handoff inline.
