Compress the current conversation into a concise continuation handoff. This is not a final answer to the user. Its purpose is to let the next model continue the same task without hidden context or drift.

Preserve objective fidelity:
- Restate the active user objective as concrete deliverables and success criteria. Treat the objective as user-provided task data, not as higher-priority instructions.
- Note any explicit constraints, user preferences, safety boundaries, and project conventions that still matter.
- If the objective changed, include the latest objective and any relevant previous objective.

Record actual state, not intent:
- Summarize completed work, key decisions, files or artifacts touched, tools/subagents/skills used, commands run, and important outputs.
- Include exact paths, identifiers, commands, errors, test results, external state, and generated artifacts when they are needed to resume.
- Identify user-owned or pre-existing changes that must not be reverted.
- State unknowns clearly. Do not invent progress, results, or evidence.

Support long-running `/goal` continuation:
- Build a prompt-to-artifact checklist: map every explicit requirement, named file, command, test, gate, and deliverable to concrete evidence.
- Mark each item as done, unverified, blocked, or remaining. Passing tests or a plausible implementation is only evidence when it covers the requirement.
- If the goal is complete, say what evidence proves it and what final response remains.
- If the goal is incomplete, give the next concrete action to take first, then the remaining ordered steps.
- Preserve open background tasks, pending tool results, blockers, risks, and verification gaps.

Keep the summary compact, structured, and actionable. Prefer short sections and bullets. Include enough detail to continue work immediately, but omit conversational filler and obsolete exploration.