---
name: auto-research
description: Long-horizon research and implementation loop for Anda Bot. Use when a task should keep moving through registered tools such as goal, subagents_manager, cron scheduling tools, file tools, and file-backed state instead of ending after one response; especially for autonomous research, multi-hour audits, repeated experiments, stall recovery, or unattended follow-up.
allowed-tools: subagents_manager, create_cron_job, recall_memory, skills_manager
metadata:
  source: https://victorchen96.github.io/auto_research/framework.html
  type: Agent Framework
  tags: autonomous, long-horizon, zero-interaction, anti-loop, heartbeat-watchdog, multi-agent, unattended, orchestration
---

# Auto-Research

Turn a broad or long-running request into an Anda Bot loop with a file-backed
ledger. The ledger is the source of truth: every iteration reads it before work
and writes it after work.

Use this skill to run the work, not to propose a framework. If the user invoked
the skill with a real objective, execute until a completion criterion is met, a
safety approval is required, or the ledger proves the task is structurally
blocked.

## Operating Rules

- Ledger first: persist state in files, not conversation memory. Brain and notes
  may provide context, but they do not replace the ledger.
- Ready means execute: once setup is sufficient, start the next work packet,
  check, retry, or monitor without asking for routine confirmation.
- Autonomy with safety: decide locally unless the next action needs missing
  credentials, irreversible destructive changes, paid/external side effects, or
  user-owned policy approval.
- Fresh work packets: each iteration receives only the task spec, recent ledger
  state, tried directions, and a checkable completion criterion.
- Direction diversity: after a stall, change a structural constraint, source
  set, decomposition, tool, or validation method; do not tune only wording.
- Separation: workers gather evidence and run checks; the orchestrator judges
  progress and pivots; a patrol only checks liveness, restarts, or nudges.

## Start Or Continue

1. Choose the task directory. Prefer a user-specified directory. Otherwise use
   `auto-research/<slug>/` in the active Anda workspace; for repository work,
   keep state outside the repo unless the user asked for repo-local artifacts.
2. Create or read:

   ```text
   state/task_spec.md
   state/progress.json
   state/findings.jsonl
   state/directions.json
   state/checks.md
   logs/events.jsonl
   ```

3. Normalize `task_spec.md` to include objective, boundaries, success criteria,
   allowed side effects, and validation commands or evidence requirements.
4. Ensure `progress.json` has `iteration`, `status`, `stale_count`,
   `last_seen_ms`, and `last_finding_count`.

Start is complete only when those files exist or have been read, the success
criteria are explicit enough to check, and the next work packet can be launched
without more planning.

## Iteration Loop

For each iteration:

1. Update `progress.json.last_seen_ms` before any other work.
2. Read `task_spec.md`, `progress.json`, `directions.json`, recent
   `findings.jsonl`, and `checks.md`.
3. Pick one direction that differs from every tried direction. Append it to
   `directions.json` before starting the work.
4. Launch a focused worker when subagents are available; otherwise do the work
   directly. Give the worker a bounded prompt with objective, directory, inputs,
   output file, validation command, and completion criterion.
5. Append durable findings to `findings.jsonl` and decisions or failures to
   `logs/events.jsonl`.
6. Run the relevant check before the next iteration: tests, build, citation
   verification, reproduction command, or an explicit evidence review.
7. Update `progress.json` with new counts, status, stale count, and next action.

Continue the loop while success criteria remain unmet and the next action is
safe and actionable.

## Worker Prompt Shape

Use this shape for `subagents_manager`, a skill subagent, or a fresh focused
run:

```text
Objective: ...
Task directory: ...
Read first: state/task_spec.md, state/progress.json, state/directions.json
Direction for this iteration: ...
Write findings to: state/findings.jsonl
Write decisions/failures to: logs/events.jsonl
Validation: ...
Done when: ...
Constraints: do not ask the user; stop only for required approval or a proven blocker.
```

## Stall And Pivot Rules

| Signal | Action |
| --- | --- |
| No new findings, no check improvement, or repeated failure mode | Increment `stale_count` |
| `stale_count >= 2` | Pivot structurally before the next worker |
| `stale_count >= 4` | Stop nudging and report a structurally blocked state |
| Last output was a question but no approval is required | Convert the question into a local decision and continue |
| External dependency is down or unauthorized | Log exact evidence, retry only if a bounded retry is useful, then report the blocker |
| Citation, benchmark, or generated data is used | Verify near the point of use; do not batch large unverified claims |

A pivot changes the frame: source corpus, experiment design, decomposition,
toolchain, validation method, or ownership boundary. Rephrasing the same prompt
is not a pivot.

## Patrol Boundaries

A patrol may perform only:

1. Liveness check: read `progress.json.last_seen_ms` and recent events.
2. Restart or nudge: start the next safe callback or worker.
3. Escalation: report the exact blocker after repeated failed nudges.

It must not edit findings, rewrite task state, impersonate a worker report, or
declare the task complete.

## Completion

Finish only when:

- Every success criterion in `task_spec.md` is met or explicitly marked blocked.
- The latest validation result is recorded in `state/checks.md`.
- `progress.json.status` is `complete` or `blocked`.
- Open timers, cron jobs, patrols, or follow-up loops are either stopped or
  named in the final report with their current state.

The final report should name the ledger directory, summarize validated results,
list remaining risks or blockers, and avoid claiming quality from unverified
self-ratings.
