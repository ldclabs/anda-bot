#!/usr/bin/env python3
"""Run trigger evaluation for an Anda Bot skill description.

The evaluator installs a copy of the skill into an isolated Anda home, runs
`anda agent run` for each query, and checks the resulting AgentOutput JSON for
evidence that the skill was selected.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import shlex
import shutil
import socket
import subprocess
import sys
import tempfile
import time
import uuid
from concurrent.futures import ProcessPoolExecutor, as_completed
from dataclasses import dataclass
from pathlib import Path
from typing import Any

if __package__ in (None, ""):
    sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from scripts.utils import parse_skill_md


@dataclass
class EvalHome:
    path: Path
    is_temp: bool
    installed_skill: Path | None = None
    backup_skill: Path | None = None


def normalise_skill_agent_name(skill_name: str) -> str:
    """Return Anda Bot's subagent name for a kebab-case skill name."""
    return "skill_" + skill_name.strip().lower().replace("-", "_")


def anda_command_parts(anda_command: str | None) -> list[str]:
    """Split a configurable Anda command into argv parts."""
    command = anda_command or os.environ.get("ANDA_COMMAND") or "anda"
    return shlex.split(command)


def free_local_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
        sock.bind(("127.0.0.1", 0))
        return int(sock.getsockname()[1])


def set_config_addr(content: str, addr: str) -> str:
    """Set or prepend the top-level `addr:` scalar in Anda config YAML."""
    if re.search(r"(?m)^addr\s*:", content):
        return re.sub(r"(?m)^addr\s*:.*$", f"addr: {addr}", content, count=1)
    prefix = f"addr: {addr}\n"
    return prefix + content.lstrip()


def default_config_source() -> Path | None:
    env_home = os.environ.get("ANDA_HOME")
    candidates = []
    if env_home:
        candidates.append(Path(env_home) / "config.yaml")
    candidates.append(Path.home() / ".anda" / "config.yaml")
    for candidate in candidates:
        if candidate.exists():
            return candidate
    return None


def update_skill_description(content: str, description: str) -> str:
    """Return SKILL.md content with the frontmatter description replaced."""
    lines = content.splitlines()
    if not lines or lines[0].strip() != "---":
        raise ValueError("SKILL.md missing opening frontmatter marker")

    end_idx = None
    for idx, line in enumerate(lines[1:], start=1):
        if line.strip() == "---":
            end_idx = idx
            break
    if end_idx is None:
        raise ValueError("SKILL.md missing closing frontmatter marker")

    frontmatter = lines[1:end_idx]
    rewritten: list[str] = []
    replaced = False
    skipping_description_block = False

    for line in frontmatter:
        if skipping_description_block:
            if line.startswith((" ", "\t")):
                continue
            skipping_description_block = False

        if line.startswith("description:"):
            rewritten.append("description: " + json.dumps(description))
            replaced = True
            value = line[len("description:"):].strip()
            skipping_description_block = value in (">", "|", ">-", "|-")
            continue

        rewritten.append(line)

    if not replaced:
        insert_at = 1 if rewritten and rewritten[0].startswith("name:") else 0
        rewritten.insert(insert_at, "description: " + json.dumps(description))

    return "\n".join(["---", *rewritten, "---", *lines[end_idx + 1:]]) + "\n"


def copy_skill_for_eval(
    skill_path: Path,
    home: Path,
    description_override: str | None,
) -> tuple[Path, Path | None]:
    """Install a temporary copy of a skill under home/skills and return backup."""
    name, _, content = parse_skill_md(skill_path)
    skills_dir = home / "skills"
    destination = skills_dir / name
    backup = None

    skills_dir.mkdir(parents=True, exist_ok=True)
    if destination.exists():
        backup = skills_dir / f".{name}.skill-creator-backup-{uuid.uuid4().hex[:8]}"
        destination.rename(backup)

    shutil.copytree(
        skill_path,
        destination,
        ignore=shutil.ignore_patterns(
            "__pycache__",
            ".DS_Store",
            "*.pyc",
            "*.skill",
            "*-workspace",
        ),
    )

    if description_override is not None:
        skill_md = destination / "SKILL.md"
        skill_md.write_text(update_skill_description(content, description_override))

    return destination, backup


def restore_installed_skill(home: EvalHome) -> None:
    if home.installed_skill and home.installed_skill.exists():
        shutil.rmtree(home.installed_skill)
    if home.backup_skill and home.backup_skill.exists() and home.installed_skill:
        home.backup_skill.rename(home.installed_skill)


def prepare_eval_home(
    skill_path: Path,
    description_override: str | None,
    home: Path | None,
    config_from: Path | None,
) -> EvalHome:
    """Create or prepare an Anda home for trigger evaluation."""
    if home is None:
        eval_home = Path(tempfile.mkdtemp(prefix="anda-skill-eval-"))
        source_config = config_from or default_config_source()
        config_content = source_config.read_text() if source_config and source_config.exists() else ""
        (eval_home / "config.yaml").write_text(
            set_config_addr(config_content, f"127.0.0.1:{free_local_port()}")
        )
        prepared = EvalHome(path=eval_home, is_temp=True)
    else:
        eval_home = home.expanduser().resolve()
        eval_home.mkdir(parents=True, exist_ok=True)
        prepared = EvalHome(path=eval_home, is_temp=False)

    installed, backup = copy_skill_for_eval(skill_path, prepared.path, description_override)
    prepared.installed_skill = installed
    prepared.backup_skill = backup
    return prepared


def stop_anda_home(anda_command: str | None, home: Path) -> None:
    cmd = [*anda_command_parts(anda_command), "--home", str(home), "stop"]
    try:
        subprocess.run(
            cmd,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            timeout=15,
            check=False,
        )
    except Exception:
        pass


def start_anda_home(anda_command: str | None, home: Path) -> None:
    cmd = [*anda_command_parts(anda_command), "--home", str(home), "start"]
    subprocess.run(cmd, capture_output=True, text=True, timeout=60, check=True)


def cleanup_eval_home(anda_command: str | None, home: EvalHome, keep_home: bool) -> None:
    stop_anda_home(anda_command, home.path)
    if home.is_temp:
        if keep_home:
            print(f"Kept eval Anda home: {home.path}", file=sys.stderr)
        else:
            shutil.rmtree(home.path, ignore_errors=True)
    else:
        restore_installed_skill(home)


def run_anda_prompt(
    prompt: str,
    anda_command: str | None,
    home: Path | None,
    timeout: int,
    workspace: Path | None = None,
    session_id: str | None = None,
    agent_name: str = "",
) -> dict[str, Any]:
    """Run an Anda prompt and return parsed AgentOutput JSON."""
    output_path = Path(tempfile.gettempdir()) / f"anda-agent-output-{uuid.uuid4().hex}.json"
    cmd = [*anda_command_parts(anda_command)]
    if home is not None:
        cmd.extend(["--home", str(home)])
    cmd.extend(["agent", "run"])
    if agent_name:
        cmd.extend(["--name", agent_name])
    cmd.extend(["--prompt", prompt])
    if workspace is not None:
        cmd.extend(["--workspace", str(workspace)])
    if session_id:
        cmd.extend(["--session-id", session_id])
    cmd.extend([
        "--output-json",
        str(output_path),
        "--wait-timeout-secs",
        str(timeout),
        "--poll-interval-ms",
        "500",
    ])

    try:
        result = subprocess.run(
            cmd,
            capture_output=True,
            text=True,
            timeout=max(timeout + 20, 30),
            check=False,
        )
        if result.returncode != 0:
            raise RuntimeError(
                "anda agent run failed "
                f"({result.returncode})\nstdout: {result.stdout[-2000:]}\n"
                f"stderr: {result.stderr[-2000:]}"
            )
        if not output_path.exists():
            raise RuntimeError("anda agent run did not write --output-json")
        return json.loads(output_path.read_text())
    finally:
        output_path.unlink(missing_ok=True)


def skill_was_used(output: dict[str, Any], skill_name: str) -> bool:
    """Inspect AgentOutput for direct or indirect skill selection evidence."""
    agent_name = normalise_skill_agent_name(skill_name)
    for call in output.get("tool_calls", []) or []:
        name = call.get("name")
        args = json.dumps(call.get("args", {}), ensure_ascii=False)
        result = json.dumps(call.get("result", {}), ensure_ascii=False)

        if name == agent_name:
            return True
        if name == "skills_manager" and skill_name in args:
            return True
        if name in {"tools_select", "tools_search"} and (
            agent_name in result or skill_name in result
        ):
            return True
    return False


def run_single_query(
    query: str,
    skill_name: str,
    timeout: int,
    home: str,
    anda_command: str | None,
    workspace: str | None = None,
    force_skill: bool = False,
) -> bool:
    """Run a query and return whether the skill was selected."""
    prompt = f"/skill {skill_name} {query}" if force_skill else query
    output = run_anda_prompt(
        prompt=prompt,
        anda_command=anda_command,
        home=Path(home),
        timeout=timeout,
        workspace=Path(workspace) if workspace else None,
        session_id=f"skill-eval-{uuid.uuid4().hex}",
    )
    return skill_was_used(output, skill_name)


def run_eval(
    eval_set: list[dict[str, Any]],
    skill_name: str,
    description: str,
    num_workers: int,
    timeout: int,
    skill_path: Path,
    runs_per_query: int = 1,
    trigger_threshold: float = 0.5,
    anda_command: str | None = None,
    home: Path | None = None,
    config_from: Path | None = None,
    keep_home: bool = False,
    workspace: Path | None = None,
    force_skill: bool = False,
) -> dict[str, Any]:
    """Run the full eval set and return results."""
    results = []
    prepared_home = prepare_eval_home(skill_path, description, home, config_from)
    start_time = time.time()

    try:
        start_anda_home(anda_command, prepared_home.path)

        with ProcessPoolExecutor(max_workers=max(1, num_workers)) as executor:
            future_to_info = {}
            for item in eval_set:
                for run_idx in range(runs_per_query):
                    future = executor.submit(
                        run_single_query,
                        item["query"],
                        skill_name,
                        timeout,
                        str(prepared_home.path),
                        anda_command,
                        str(workspace) if workspace else None,
                        force_skill,
                    )
                    future_to_info[future] = (item, run_idx)

            query_triggers: dict[str, list[bool]] = {}
            query_items: dict[str, dict[str, Any]] = {}
            for future in as_completed(future_to_info):
                item, _ = future_to_info[future]
                query = item["query"]
                query_items[query] = item
                query_triggers.setdefault(query, [])
                try:
                    query_triggers[query].append(future.result())
                except Exception as exc:
                    print(f"Warning: query failed: {exc}", file=sys.stderr)
                    query_triggers[query].append(False)
    finally:
        cleanup_eval_home(anda_command, prepared_home, keep_home)

    for query, triggers in query_triggers.items():
        item = query_items[query]
        trigger_rate = sum(triggers) / len(triggers)
        should_trigger = item["should_trigger"]
        if should_trigger:
            did_pass = trigger_rate >= trigger_threshold
        else:
            did_pass = trigger_rate < trigger_threshold
        results.append({
            "query": query,
            "should_trigger": should_trigger,
            "trigger_rate": trigger_rate,
            "triggers": sum(triggers),
            "runs": len(triggers),
            "pass": did_pass,
        })

    passed = sum(1 for r in results if r["pass"])
    total = len(results)

    return {
        "skill_name": skill_name,
        "description": description,
        "results": results,
        "summary": {
            "total": total,
            "passed": passed,
            "failed": total - passed,
            "duration_seconds": round(time.time() - start_time, 3),
        },
    }


def main() -> None:
    parser = argparse.ArgumentParser(description="Run Anda Bot trigger evaluation for a skill")
    parser.add_argument("--eval-set", required=True, help="Path to eval set JSON file")
    parser.add_argument("--skill-path", required=True, help="Path to skill directory")
    parser.add_argument("--description", default=None, help="Override description to test")
    parser.add_argument("--num-workers", type=int, default=3, help="Number of parallel workers")
    parser.add_argument("--timeout", type=int, default=60, help="Timeout per query in seconds")
    parser.add_argument("--runs-per-query", type=int, default=3, help="Number of runs per query")
    parser.add_argument("--trigger-threshold", type=float, default=0.5, help="Trigger rate threshold")
    parser.add_argument("--anda-command", default=None, help="Command used to invoke Anda (default: anda or ANDA_COMMAND)")
    parser.add_argument("--home", default=None, help="Anda home to use. Defaults to an isolated temporary home")
    parser.add_argument("--config-from", default=None, help="Config file copied into temporary homes")
    parser.add_argument("--workspace", default=None, help="Workspace passed to anda agent run")
    parser.add_argument("--keep-home", action="store_true", help="Keep temporary Anda home after the run")
    parser.add_argument("--force-skill", action="store_true", help="Prefix each query with /skill skill-name")
    parser.add_argument("--verbose", action="store_true", help="Print progress to stderr")
    args = parser.parse_args()

    eval_set = json.loads(Path(args.eval_set).read_text())
    skill_path = Path(args.skill_path).expanduser().resolve()

    if not (skill_path / "SKILL.md").exists():
        print(f"Error: No SKILL.md found at {skill_path}", file=sys.stderr)
        sys.exit(1)

    name, original_description, _ = parse_skill_md(skill_path)
    description = args.description or original_description

    if args.verbose:
        print(f"Evaluating: {description}", file=sys.stderr)

    output = run_eval(
        eval_set=eval_set,
        skill_name=name,
        description=description,
        num_workers=args.num_workers,
        timeout=args.timeout,
        skill_path=skill_path,
        runs_per_query=args.runs_per_query,
        trigger_threshold=args.trigger_threshold,
        anda_command=args.anda_command,
        home=Path(args.home).expanduser().resolve() if args.home else None,
        config_from=Path(args.config_from).expanduser().resolve() if args.config_from else None,
        keep_home=args.keep_home,
        workspace=Path(args.workspace).expanduser().resolve() if args.workspace else None,
        force_skill=args.force_skill,
    )

    if args.verbose:
        summary = output["summary"]
        print(f"Results: {summary['passed']}/{summary['total']} passed", file=sys.stderr)
        for r in output["results"]:
            status = "PASS" if r["pass"] else "FAIL"
            rate_str = f"{r['triggers']}/{r['runs']}"
            print(
                f"  [{status}] rate={rate_str} expected={r['should_trigger']}: {r['query'][:70]}",
                file=sys.stderr,
            )

    print(json.dumps(output, indent=2))


if __name__ == "__main__":
    main()
