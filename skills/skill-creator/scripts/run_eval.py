#!/usr/bin/env python3
"""Run trigger evaluation for an Anda Bot skill description.

The evaluator installs a copy of the skill into an isolated Anda home, runs
`anda agent run` for each query, and checks the resulting AgentOutput JSON for
evidence that the skill was selected.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import shlex
import shutil
import socket
import subprocess
import sys
import tempfile
import time
import uuid
from concurrent.futures import ThreadPoolExecutor, as_completed
from contextlib import contextmanager
from pathlib import Path
from typing import Any

if __package__ in (None, ""):
    sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from scripts.quick_validate import validate_skill
from scripts.utils import parse_skill_md, split_skill_md, yaml


def anda_command_parts(anda_command: str | None) -> list[str]:
    """Split a configurable Anda command into argv parts."""
    command = anda_command or os.environ.get("ANDA_COMMAND") or "anda"
    return shlex.split(command)


def free_local_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
        sock.bind(("127.0.0.1", 0))
        return int(sock.getsockname()[1])


@contextmanager
def eval_runtime(anda_command, home, config_from, keep_home=False, skill_path=None, description=None):
    """Start and stop only a daemon owned by this run. Reuse model settings, never live state."""
    source_home = home or Path(os.environ.get("ANDA_HOME") or Path.home() / ".anda")
    source = config_from or source_home / "config.yaml"
    config = {"addr": f"127.0.0.1:{free_local_port()}"}
    if config_from is not None or home is not None or source.is_file():
        settings = yaml.safe_load(source.read_text(encoding="utf-8"))
        if not isinstance(settings, dict):
            raise ValueError(f"Expected a configuration mapping in {source}")
        if "model" in settings:
            config["model"] = settings["model"]

    runtime = Path(tempfile.mkdtemp(prefix="anda-skill-eval-")).resolve()
    command = [*anda_command_parts(anda_command), "--home", str(runtime)]
    try:
        config_path = runtime / "config.yaml"
        config_path.write_text(yaml.safe_dump(config), encoding="utf-8")
        config_path.chmod(0o600)
        if skill_path is not None:
            valid, message = validate_skill(skill_path)
            if not valid:
                raise ValueError(message)
            name, _, content = parse_skill_md(skill_path)
            destination = runtime / "skills" / name
            shutil.copytree(skill_path, destination, symlinks=True, ignore=shutil.ignore_patterns(
                ".git", ".venv", "__pycache__", "node_modules", "*.pyc", "*.skill", ".DS_Store"
            ))
            if description is not None:
                frontmatter, body = split_skill_md(content)
                frontmatter["description"] = description
                updated = yaml.safe_dump(frontmatter, sort_keys=False, allow_unicode=True)
                (destination / "SKILL.md").write_text(f"---\n{updated}---\n{body}\n", encoding="utf-8")
                valid, message = validate_skill(destination)
                if not valid:
                    raise ValueError(message)

        # A new home on a headless machine may have no OS credential store.
        subprocess.run([*command, "user", "init-file-keys"], check=True, stdout=sys.stderr, timeout=30)
        try:
            subprocess.run([*command, "start"], check=True, stdout=sys.stderr, timeout=60)
            yield runtime
        finally:
            try:
                subprocess.run([*command, "stop"], check=True, stdout=sys.stderr, timeout=30)
            except (OSError, subprocess.SubprocessError):
                keep_home = True
                raise
    finally:
        if keep_home:
            print(f"Kept eval Anda home: {runtime}", file=sys.stderr)
        else:
            shutil.rmtree(runtime)


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
    cmd.extend(["--wait-timeout-secs", str(timeout), "--poll-interval-ms", "500"])
    with tempfile.TemporaryDirectory(prefix="anda-eval-output-") as temp:
        output_path = Path(temp) / "output.json"
        result = subprocess.run(
            [*cmd, "--output-json", str(output_path)], stdout=subprocess.DEVNULL,
            stderr=subprocess.PIPE, text=True, timeout=timeout + 20,
        )
        if result.returncode:
            raise RuntimeError(f"anda agent run failed ({result.returncode}): {result.stderr[-2000:]}")
        output = json.loads(output_path.read_text(encoding="utf-8"))
        if output.get("failed_reason"):
            raise RuntimeError(f"anda agent run failed: {output['failed_reason']}")
        return output


def skill_was_used(output: dict[str, Any], skill_name: str) -> bool:
    """Count a targeted read or delegation, including persisted CLI conversation history."""
    normal = skill_name.replace("-", "_")
    if len(normal) <= 58:
        agent = "skill_" + normal
    else:
        agent = "skillh_" + hashlib.sha256(normal.encode()).hexdigest()[:54]
    if output.get("tools_usage", {}).get(agent, {}).get("requests", 0):
        return True
    calls = list(output.get("tool_calls", []))
    for message in output.get("chat_history", []):
        for part in message.get("content", []):
            if not isinstance(part, dict):
                continue
            if message.get("role") == "assistant" and part.get("type") == "ToolCall":
                calls.append(part)
            # skills_read can be addressed by an opaque ID. Its result carries the resolved name.
            if message.get("role") == "tool" and part.get("type") == "ToolOutput" and part.get("name") == "skills_read":
                value = part.get("output")
                if isinstance(value, dict) and value.get("name") == skill_name and not part.get("isError"):
                    return True
    for call in calls:
        name = call.get("name")
        if name in {agent, f"SA_{agent}"}:
            return True
        args = call.get("args")
        if not isinstance(args, dict):
            continue
        if name == "skills_manager" and args.get("name") == skill_name:
            return True
        if name == "skills_read" and args.get("skill") == skill_name:
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
    session_id = f"skill-eval-{uuid.uuid4().hex}"
    run_dir = Path(workspace) if workspace else Path(home) / "workspace" / session_id
    if not workspace:
        run_dir.mkdir(parents=True)
    output = run_anda_prompt(
        prompt=prompt,
        anda_command=anda_command,
        home=Path(home),
        timeout=timeout,
        workspace=run_dir,
        session_id=session_id,
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
    if not eval_set or runs_per_query < 1 or num_workers < 1 or not 0 < trigger_threshold <= 1:
        raise ValueError("Provide at least one query, worker and run, and a threshold in (0,1]")
    results = []
    start_time = time.time()

    with eval_runtime(anda_command, home, config_from, keep_home, skill_path, description) as runtime:
        with ThreadPoolExecutor(max_workers=num_workers) as executor:
            future_to_info = {}
            for item in eval_set:
                for run_idx in range(runs_per_query):
                    future = executor.submit(
                        run_single_query,
                        item["query"],
                        skill_name,
                        timeout,
                        str(runtime),
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
                query_triggers[query].append(future.result())

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
    parser.add_argument("--home", default=None, help="Source Anda home for model configuration only. Each run uses a temporary home")
    parser.add_argument("--config-from", default=None, help="YAML file providing the model configuration for the temporary home")
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
