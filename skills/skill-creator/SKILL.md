---
name: skill-creator
description: Create, update, test, and package Anda Bot runtime skills. Use this skill when users want to create a new SKILL.md, improve an existing skill, convert a skill from another agent runtime to Anda Bot, write trigger evals, run skill behavior tests, optimize the frontmatter description, or package a reusable skill.
---

# Skill Creator

This skill helps create and improve Anda Bot runtime skills. Anda Bot loads
skills from `~/.anda/skills` or other configured skill directories. Each skill
is a directory containing `SKILL.md` and optional bundled resources.

Use this skill to:

- Create a new skill from a workflow the user describes.
- Update an existing skill while preserving its name and contract.
- Convert a skill written for another agent runtime into an Anda Bot skill.
- Create behavior test prompts and trigger evals.
- Run evals through `anda agent run` when a configured Anda runtime is
  available.
- Package a finished skill for installation.

## Core Loop

At a high level:

1. Understand what the skill should help Anda Bot do.
2. Draft or revise `SKILL.md`.
3. Create a few realistic test prompts.
4. Run the prompts with the skill and compare against a baseline when useful.
5. Review qualitative outputs and quantitative checks with the user.
6. Improve the skill based on evidence.
7. Repeat until the skill is useful and not overfit.
8. Optionally optimize the frontmatter description and package the skill.

Be flexible. If the user explicitly wants a quick edit without evals, make the
edit and run lightweight validation instead of forcing the full loop.

## Communicating With The User

Adjust terminology to the user's familiarity. "Eval" and "benchmark" are fine
for technical users, but briefly explain them for users who may not use those
terms. Avoid making skill creation sound harder than it is: the core file is a
Markdown instruction file plus optional helper files.

## Creating Or Updating A Skill

### Capture Intent

Extract intent from the current conversation first. If the user says "turn this
workflow into a skill", identify the sequence of steps, tools used, corrections
the user made, input formats, output formats, and success criteria.

Answer these questions before drafting:

1. What should this skill enable Anda Bot to do?
2. When should the skill trigger? Include user phrases, file types, task types,
   and near-misses where it should not trigger.
3. What should the output look like?
4. What tools, local files, scripts, APIs, or external programs does it need?
5. Should test cases be created now?

Skills with objective outputs, file transforms, code generation, or fixed
workflow steps benefit from test cases. Skills for subjective writing or taste
work may only need example prompts and human review.

### Research And Existing Patterns

Before editing, inspect nearby skills and any repository instructions. Prefer
the existing style and local tool names. For Anda Bot skills:

- Use `shell`, `read_file`, `search_file`, `note`, `tools_select`, and available
  subagents only when those tools are available to the skill.
- Mention `/skill skill-name message` as the explicit user invocation path.
- For CLI runs, use `anda agent run --prompt "..."`.
- Do not refer to another agent product's hosted UI, document model, slash
  commands, or connector settings unless the skill is specifically about
  delegating to that product.

### Write `SKILL.md`

Use this structure:

```markdown
---
name: skill-name
description: Use this skill when ...
---

# Skill Name

Instructions...
```

Frontmatter requirements:

- `name`: kebab-case, 1-64 characters, lowercase letters/numbers/hyphens.
- `description`: 1-1024 characters. This is the primary trigger text.
- `compatibility`: optional, for required programs or services.
- `allowed-tools`: optional, space-delimited tools if the skill should narrow
  the default tool set.

The description should state both what the skill does and when to use it. It
should be specific enough to beat nearby skills, but not a long list of every
possible query.

### Skill Anatomy

```text
skill-name/
  SKILL.md
  scripts/
  references/
  assets/
```

Use bundled resources when they prevent repeated work:

- `scripts/`: deterministic or repetitive operations.
- `references/`: longer documentation loaded only when needed.
- `assets/`: templates, icons, fonts, fixtures, or sample files.

Keep `SKILL.md` concise enough to load directly. If it approaches 500 lines,
move detailed references into `references/` and point to them clearly.

### Writing Patterns

Prefer imperative instructions and explain why a workflow matters. Avoid
overusing all-caps rules. A skill should guide judgment, not just force a brittle
script.

When defining output formats, give exact templates:

```markdown
## Report structure
Use this structure:
# [Title]
## Executive summary
## Findings
## Recommendations
```

Examples help when the skill handles repeated formats:

```markdown
Input: Added JWT-based authentication
Output: feat(auth): implement JWT authentication
```

### Safety

Do not create skills that exfiltrate data, hide behavior, bypass security
controls, mislead users, or surprise the user relative to the stated purpose.
For skills that run shell commands or touch external systems, include clear
guardrails around destructive operations, credentials, and privacy-sensitive
data.

## Test Cases

After drafting or substantially editing a skill, create 2-3 realistic test
prompts. Save them to `evals/evals.json` inside the skill directory or a sibling
workspace when the user wants repeatable testing:

```json
{
  "skill_name": "example-skill",
  "evals": [
    {
      "id": 1,
      "prompt": "User's task prompt",
      "expected_output": "Description of expected result",
      "files": []
    }
  ]
}
```

See `references/schemas.md` for the complete schema, including assertions and
timing fields.

## Running Behavior Evals

Use a sibling workspace named `<skill-name>-workspace/`. Organize by iteration:

```text
example-skill-workspace/
  iteration-1/
    eval-0-descriptive-name/
      with_skill/
      without_skill/
      eval_metadata.json
```

For each eval, run one or both configurations:

- `with_skill`: explicit `/skill skill-name <prompt>` or a fresh agent run where
  the skill is expected to be selected naturally.
- `without_skill`: baseline run without the skill installed, or the previous
  version when improving an existing skill.

If subagents are available, use them for independent runs. If not, run cases
serially with `anda agent run` or perform a manual sanity check and say that the
baseline is weaker.

Example explicit skill run:

```bash
anda agent run \
  --prompt "/skill example-skill <eval prompt>" \
  --output-json <workspace>/iteration-1/eval-0/with_skill/output.json
```

When the skill should trigger automatically, omit `/skill` and inspect the
`tool_calls` in `output.json` for `skill_example_skill`, `skills_manager`, or
`tools_select` evidence.

### Assertions

While runs are in progress, draft objective assertions where possible. Good
assertions are clear to a reviewer and mechanically checkable when feasible.
For subjective work, rely more on human qualitative review.

Each eval directory should include:

```json
{
  "eval_id": 0,
  "eval_name": "descriptive-name",
  "prompt": "The user's task prompt",
  "assertions": []
}
```

### Timing

If a run returns usage or duration data, save it immediately:

```json
{
  "total_tokens": 84852,
  "duration_ms": 23332,
  "total_duration_seconds": 23.3
}
```

## Grading And Review

Once runs finish:

1. Grade assertions. Use `agents/grader.md` when spawning a grader or grading
   inline. `grading.json` expectations must use `text`, `passed`, and
   `evidence`.
2. Aggregate benchmark data:

   ```bash
   python <skill-creator-path>/scripts/aggregate_benchmark.py \
     <workspace>/iteration-N \
     --skill-name <name>
   ```

3. Run an analyst pass using `agents/analyzer.md` when the results are
   non-trivial.
4. Generate the review UI:

   ```bash
   nohup python <skill-creator-path>/eval-viewer/generate_review.py \
     <workspace>/iteration-N \
     --skill-name "my-skill" \
     --benchmark <workspace>/iteration-N/benchmark.json \
     > /dev/null 2>&1 &
   VIEWER_PID=$!
   ```

   In headless environments, use `--static <output_path>` and give the user the
   HTML path.

Tell the user the Outputs tab is for qualitative review and the Benchmark tab is
for quantitative checks. When they finish, read `feedback.json` and improve the
skill based on concrete complaints.

## Improving A Skill

When feedback arrives:

- Generalize from the issue rather than overfitting to one prompt.
- Remove instructions that cause repeated waste or confusion.
- Explain the reason behind important constraints.
- Bundle helper scripts when multiple runs independently recreate the same
  logic.
- Preserve the original `name` unless the user explicitly wants a renamed skill.

If the installed skill is read-only, copy it to a writable workspace, edit the
copy, and package from there.

## Description Optimization

The `description` field determines when Anda Bot sees the skill as relevant.
After creating or improving a skill, offer to optimize it.

### Create Trigger Evals

Create about 20 trigger queries:

- 8-10 should trigger.
- 8-10 should not trigger.
- Include realistic paths, filenames, tool contexts, vague phrasing, typos, and
  near-misses.
- Avoid obviously irrelevant negatives.

Save JSON like:

```json
[
  {"query": "the user prompt", "should_trigger": true},
  {"query": "another prompt", "should_trigger": false}
]
```

### Review The Eval Set

Use `assets/eval_review.html`:

1. Replace `__EVAL_DATA_PLACEHOLDER__` with the JSON array.
2. Replace `__SKILL_NAME_PLACEHOLDER__` with the skill name.
3. Replace `__SKILL_DESCRIPTION_PLACEHOLDER__` with the current description.
4. Write the rendered HTML to a temp file and open it if browser access exists.
5. If no browser is available, show the eval set inline and ask for approval.

### Run The Optimization Loop

When the user approves, run:

```bash
python <skill-creator-path>/scripts/run_loop.py \
  --eval-set <path-to-trigger-eval.json> \
  --skill-path <path-to-skill> \
  --max-iterations 5 \
  --verbose
```

The scripts use `anda agent run` by default. Useful options:

- `--anda-command <command>`: command used to invoke Anda, default `anda`.
- `--home <path>`: reuse a specific Anda home. If omitted, scripts create a
  temporary home, copy model config from `~/.anda/config.yaml` when present, and
  install the skill under that temp home.
- `--config-from <path>`: source config for temporary homes.
- `--keep-home`: keep the temporary home for inspection.

The loop evaluates the description, asks Anda Bot to propose an improved
description, and writes a report showing train and held-out test performance.
Apply the returned `best_description` only after inspecting it for clarity and
overfitting.

### How Anda Bot Skill Triggering Works

Anda Bot loads each skill as a subagent named `skill_<skill_name>` where hyphens
become underscores. A user can force a skill with:

```text
/skill skill-name message
```

Without `/skill`, the main agent can select a skill through the skill manager,
`tools_select`, or direct subagent invocation. Trigger evals should therefore be
substantive enough that loading a specialized workflow is useful. Very simple
queries may be answered directly even when the description is reasonable.

## Packaging

Package a finished skill with:

```bash
python <skill-creator-path>/scripts/package_skill.py <path/to/skill-folder>
```

Return the resulting `.skill` path. Do not rename the package unless the user
asked for a new skill identity.

## Reference Files

Read these only when needed:

- `agents/grader.md`: assertion grading guidance.
- `agents/comparator.md`: blind A/B comparison.
- `agents/analyzer.md`: benchmark analysis.
- `references/schemas.md`: JSON structures for evals, grading, benchmark, and
  timing files.

## Final Checklist

Before calling a skill done:

- `SKILL.md` has valid frontmatter and a focused description.
- The body is written for Anda Bot, not another agent product.
- Required tools and dependencies are stated.
- Test prompts or trigger evals exist when useful.
- Scripts and references are bundled only when they reduce repeated work.
- Any generated package preserves the skill name and directory contract.
