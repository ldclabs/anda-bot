#!/usr/bin/env python3
"""Improve a skill description based on Anda Bot trigger eval results."""

from __future__ import annotations

import argparse
import json
import re
import sys
import uuid
from pathlib import Path
from typing import Any

if __package__ in (None, ""):
    sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from scripts.run_eval import run_anda_prompt
from scripts.utils import parse_skill_md


def _call_anda(
    prompt: str,
    anda_command: str | None,
    home: Path | None,
    timeout: int,
) -> str:
    """Run `anda agent run` with the prompt and return final visible content."""
    output = run_anda_prompt(
        prompt=prompt,
        anda_command=anda_command,
        home=home,
        timeout=timeout,
        session_id=f"skill-description-{uuid.uuid4().hex}",
    )
    failed = output.get("failed_reason")
    if failed:
        raise RuntimeError(f"anda agent run failed: {failed}")
    return str(output.get("content", "")).strip()


def improve_description(
    skill_name: str,
    skill_content: str,
    current_description: str,
    eval_results: dict[str, Any],
    history: list[dict[str, Any]],
    anda_command: str | None = None,
    home: Path | None = None,
    timeout: int = 180,
    test_results: dict[str, Any] | None = None,
    log_dir: Path | None = None,
    iteration: int | None = None,
) -> str:
    """Ask Anda Bot to improve the description based on eval results."""
    failed_triggers = [
        r for r in eval_results["results"]
        if r["should_trigger"] and not r["pass"]
    ]
    false_triggers = [
        r for r in eval_results["results"]
        if not r["should_trigger"] and not r["pass"]
    ]

    train_score = f"{eval_results['summary']['passed']}/{eval_results['summary']['total']}"
    if test_results:
        test_score = f"{test_results['summary']['passed']}/{test_results['summary']['total']}"
        scores_summary = f"Train: {train_score}, Test: {test_score}"
    else:
        scores_summary = f"Train: {train_score}"

    prompt = f"""You are optimizing the `description` frontmatter for an Anda Bot runtime skill named "{skill_name}".

Anda Bot loads each skill as a subagent named `skill_<skill_name>` with hyphens converted to underscores. The frontmatter description is the primary signal used when the main agent or `tools_select` decides whether this skill is relevant. Users can also force a skill with `/skill {skill_name} ...`, but this optimization is about natural selection.

Current description:
<current_description>
{current_description}
</current_description>

Current scores ({scores_summary}):
<scores_summary>
"""
    if failed_triggers:
        prompt += "FAILED TO TRIGGER (should have triggered but did not):\n"
        for r in failed_triggers:
            prompt += f'  - "{r["query"]}" (triggered {r["triggers"]}/{r["runs"]} times)\n'
        prompt += "\n"

    if false_triggers:
        prompt += "FALSE TRIGGERS (triggered but should not have):\n"
        for r in false_triggers:
            prompt += f'  - "{r["query"]}" (triggered {r["triggers"]}/{r["runs"]} times)\n'
        prompt += "\n"

    if history:
        prompt += "PREVIOUS ATTEMPTS (do not repeat these; try a different framing):\n\n"
        for h in history:
            train_s = f"{h.get('train_passed', h.get('passed', 0))}/{h.get('train_total', h.get('total', 0))}"
            test_s = (
                f"{h.get('test_passed', '?')}/{h.get('test_total', '?')}"
                if h.get("test_passed") is not None
                else None
            )
            score_str = f"train={train_s}" + (f", test={test_s}" if test_s else "")
            prompt += f"<attempt {score_str}>\n"
            prompt += f'Description: "{h["description"]}"\n'
            if "results" in h:
                prompt += "Train results:\n"
                for r in h["results"]:
                    status = "PASS" if r["pass"] else "FAIL"
                    prompt += f'  [{status}] "{r["query"][:80]}" (triggered {r["triggers"]}/{r["runs"]})\n'
            if h.get("note"):
                prompt += f'Note: {h["note"]}\n'
            prompt += "</attempt>\n\n"

    prompt += f"""</scores_summary>

Skill content:
<skill_content>
{skill_content}
</skill_content>

Write a stronger description that generalizes from the failures without overfitting to specific eval queries. Keep it focused on user intent, task boundaries, and the situations where this skill should beat nearby skills.

Constraints:
- 100-200 words at most.
- Hard limit: 1024 characters.
- Imperative phrasing is usually best, for example "Use this skill when ...".
- Include important near-miss boundaries if they prevent false triggers.
- Do not mention this eval harness.

Respond with only the new description inside <new_description> tags."""

    text = _call_anda(prompt, anda_command, home, timeout)

    match = re.search(r"<new_description>(.*?)</new_description>", text, re.DOTALL)
    description = match.group(1).strip().strip('"') if match else text.strip().strip('"')

    transcript: dict[str, Any] = {
        "iteration": iteration,
        "prompt": prompt,
        "response": text,
        "parsed_description": description,
        "char_count": len(description),
        "over_limit": len(description) > 1024,
    }

    if len(description) > 1024:
        shorten_prompt = (
            f"{prompt}\n\n"
            f"---\n\n"
            f"A previous attempt produced this description, which is "
            f"{len(description)} characters and exceeds the 1024-character "
            f"hard limit:\n\n"
            f'"{description}"\n\n'
            f"Rewrite it under 1024 characters while preserving the most "
            f"important trigger words and boundaries. Respond with only the "
            f"new description in <new_description> tags."
        )
        shorten_text = _call_anda(shorten_prompt, anda_command, home, timeout)
        match = re.search(r"<new_description>(.*?)</new_description>", shorten_text, re.DOTALL)
        shortened = match.group(1).strip().strip('"') if match else shorten_text.strip().strip('"')

        transcript["rewrite_prompt"] = shorten_prompt
        transcript["rewrite_response"] = shorten_text
        transcript["rewrite_description"] = shortened
        transcript["rewrite_char_count"] = len(shortened)
        description = shortened

    transcript["final_description"] = description

    if log_dir:
        log_dir.mkdir(parents=True, exist_ok=True)
        log_file = log_dir / f"improve_iter_{iteration or 'unknown'}.json"
        log_file.write_text(json.dumps(transcript, indent=2))

    return description


def main() -> None:
    parser = argparse.ArgumentParser(description="Improve an Anda Bot skill description")
    parser.add_argument("--eval-results", required=True, help="Path to eval results JSON from run_eval.py")
    parser.add_argument("--skill-path", required=True, help="Path to skill directory")
    parser.add_argument("--history", default=None, help="Path to history JSON with previous attempts")
    parser.add_argument("--anda-command", default=None, help="Command used to invoke Anda (default: anda or ANDA_COMMAND)")
    parser.add_argument("--home", default=None, help="Anda home used for the improvement prompt")
    parser.add_argument("--timeout", type=int, default=180, help="Timeout for the improvement prompt")
    parser.add_argument("--verbose", action="store_true", help="Print progress to stderr")
    args = parser.parse_args()

    skill_path = Path(args.skill_path).expanduser().resolve()
    if not (skill_path / "SKILL.md").exists():
        print(f"Error: No SKILL.md found at {skill_path}", file=sys.stderr)
        sys.exit(1)

    eval_results = json.loads(Path(args.eval_results).read_text())
    history = []
    if args.history:
        history = json.loads(Path(args.history).read_text())

    name, _, content = parse_skill_md(skill_path)
    current_description = eval_results["description"]

    if args.verbose:
        print(f"Current: {current_description}", file=sys.stderr)
        print(
            f"Score: {eval_results['summary']['passed']}/{eval_results['summary']['total']}",
            file=sys.stderr,
        )

    new_description = improve_description(
        skill_name=name,
        skill_content=content,
        current_description=current_description,
        eval_results=eval_results,
        history=history,
        anda_command=args.anda_command,
        home=Path(args.home).expanduser().resolve() if args.home else None,
        timeout=args.timeout,
    )

    if args.verbose:
        print(f"Improved: {new_description}", file=sys.stderr)

    output = {
        "description": new_description,
        "history": history + [{
            "description": current_description,
            "passed": eval_results["summary"]["passed"],
            "failed": eval_results["summary"]["failed"],
            "total": eval_results["summary"]["total"],
            "results": eval_results["results"],
        }],
    }
    print(json.dumps(output, indent=2))


if __name__ == "__main__":
    main()
