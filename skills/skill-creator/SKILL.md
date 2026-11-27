---
name: skill-creator
description: Create, adapt, validate, evaluate or package an Anda Bot runtime skill. Use for SKILL.md instructions, routing descriptions, bundled helpers and behavior or trigger tests.
compatibility: The offline Python helpers use Python 3.10+ and PyYAML. Live evaluations also need the Anda CLI and a configured model.
---

# Create or update an Anda skill

Start from a real request, expected result and a nearby request that another skill should handle. Inspect an existing package before editing it. Preserve the workflow's name, inputs, outputs, references and license. Put only the information needed to select the skill in its description.

## Write for the runtime

```markdown
---
name: release-notes
description: Draft release notes from a specified Git revision range for a project's users.
---

Read the specified range and group user-visible changes by effect. Follow the project's release-note format.
```

- Use a directory matching the name. A name is 1–64 lowercase ASCII letters, digits and hyphens, with no leading, trailing or consecutive hyphens. A description is 1–1024 characters. Use `compatibility` for actual local programs or services.
- Most skills run inline. Anda reads the instructions with `skills_manager` and follows them in the conversation. For an independent worker, set `execution: subagent` and give it a self-contained prompt through the returned `callable`. A declared `allowed-tools` bounds its initial tool set. `resource-tags` controls which caller resources it receives. Neither grants host permissions.
- For bundled text, use `skills_read` with the exact skill name or ID, package-relative `resource` and successive `next_cursor` values through EOF. For executable helpers, use `shell` with an absolute path under the returned `base_dir`. Keep the working directory and outputs in the authorized workspace.
- For persistent personal skills, `skills_api` supports `CreateSkill`, `ValidateSkill` and `UpdateSkill` (send the version from `GetSkill` when updating). Clone a Bundled or Shared skill to Personal before editing an installed copy. When maintaining a source repository, edit the requested source directory. After a manual change to an installed directory, use `ReloadSkills` and confirm the active copy through `skills_list`.
- Test explicit routing with `/skill release-notes <task>` or `$release-notes <task>`. A natural trigger test uses the user's prompt without either prefix.

## Verify the work

Choose representative inputs and assert something observable in the output. Run changed scripts from a different working directory with paths containing spaces. For a behavior comparison, use the same inputs with the old and new skill and inspect the resulting artifacts. The [evaluation formats](references/schemas.md) describe the fixtures and grading data. The [grader](agents/grader.md), [comparator](agents/comparator.md) and [analyzer](agents/analyzer.md) explain how to record evidence, compare results and interpret a benchmark.

Run the helpers by absolute path from the workspace. `SKILL_DIR` below is the returned directory of this skill.

```sh
python3 "$SKILL_DIR/scripts/quick_validate.py" ./release-notes
python3 "$SKILL_DIR/scripts/package_skill.py" ./release-notes ./dist
```

Packaging is optional. It creates a ZIP-format `.skill` with the skill directory at its root. Anda discovers the unpacked directory containing `SKILL.md`. Keep test outputs outside it.

## Optional model-backed evaluations

Use model-backed runs when the user requests them or their cost and scope are already covered. The runner starts a temporary Anda daemon with a fresh workspace and the selected model configuration. Use a test provider and read-only fixtures. Installed shared skills can also be discovered by the host. Keep fixtures separate when running queries in parallel.

A trigger set is a JSON array such as `[{"query":"Draft release notes for v1..v2","should_trigger":true},{"query":"Which branch am I on?","should_trigger":false}]`.

```sh
python3 "$SKILL_DIR/scripts/run_eval.py" --skill-path ./release-notes \
  --eval-set ./triggers.json --config-from ./eval-config.yaml --runs-per-query 1
```

Use `run_loop.py` with the same inputs to test successive descriptions. Review a proposed description on held-out queries before applying it. `scripts/aggregate_benchmark.py` summarizes graded behavior runs. For a shareable review, run `eval-viewer/generate_review.py` with `--static` and an output path. Keep reports in the workspace and return their absolute paths.
