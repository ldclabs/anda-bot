#!/usr/bin/env python3
"""Check an Anda skill package before sharing or running it."""

import re
import sys
from pathlib import Path

if __package__:
    from .utils import split_skill_md
else:
    from utils import split_skill_md


def validate_skill(skill_path):
    folder = Path(skill_path).resolve()
    document = folder / "SKILL.md"
    if document.is_symlink() or not document.is_file():
        return False, "SKILL.md must be a regular file in the skill directory"
    try:
        frontmatter, body = split_skill_md(document.read_text(encoding="utf-8"))
    except (OSError, ValueError) as error:
        return False, str(error)

    name = frontmatter.get("name")
    if not isinstance(name, str) or len(name) > 64 or not re.fullmatch(r"[a-z0-9]+(?:-[a-z0-9]+)*", name):
        return False, "name must be 1–64 lowercase ASCII letters, digits and single internal hyphens"
    if folder.name != name:
        return False, f"Package directory {folder.name!r} must match the skill name {name!r}"
    description = frontmatter.get("description")
    if not isinstance(description, str) or not description.strip() or len(description) > 1024:
        return False, "description must be a nonblank string of at most 1024 characters"
    compatibility = frontmatter.get("compatibility")
    if compatibility is not None and (
        not isinstance(compatibility, str) or not compatibility.strip() or len(compatibility) > 500
    ):
        return False, "compatibility must be a nonblank string of at most 500 characters"
    metadata = frontmatter.get("metadata", {})
    if not isinstance(metadata, dict):
        return False, "metadata must be a mapping"
    mode = frontmatter.get("execution")
    if mode is None or isinstance(mode, str) and not mode.strip():
        mode = metadata.get("execution", "inline")
    if not isinstance(mode, str) or mode.strip().lower() not in ("inline", "subagent"):
        return False, "execution must be inline or subagent"
    for field in ("allowed-tools", "resource-tags"):
        value = frontmatter.get(field, frontmatter.get(field.replace("-", "_")))
        if value is not None and not isinstance(value, str) and not (
            isinstance(value, list) and all(isinstance(item, str) for item in value)
        ):
            return False, f"{field} must be a string or a list of strings"
    if not body.strip():
        return False, "SKILL.md has no instructions"
    return True, "Skill is valid"


if __name__ == "__main__":
    if len(sys.argv) != 2:
        sys.exit("Usage: python3 quick_validate.py <skill-directory>")
    valid, message = validate_skill(sys.argv[1])
    print(message)
    sys.exit(0 if valid else 1)
