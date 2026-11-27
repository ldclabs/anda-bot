#!/usr/bin/env python3
"""Package a validated skill as a directory-rooted .skill ZIP archive."""

import argparse
import os
import tempfile
import zipfile
from pathlib import Path

if __package__:
    from .quick_validate import validate_skill
else:
    from quick_validate import validate_skill


def package_skill(skill_path, output_dir=None):
    source = Path(skill_path).resolve()
    valid, message = validate_skill(source)
    if not valid:
        print(f"Validation failed: {message}")
        return None

    destination = Path(output_dir or ".").resolve() / f"{source.name}.skill"
    destination.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile(dir=destination.parent, delete=False) as temp:
        pending = Path(temp.name)
    try:
        with zipfile.ZipFile(pending, "w", zipfile.ZIP_DEFLATED) as archive:
            for root, dirs, files in os.walk(source):
                root = Path(root)
                ignored = {".git", ".venv", "__pycache__", "node_modules"}
                if root == source:
                    ignored.add("evals")
                dirs[:] = sorted(d for d in dirs if d not in ignored and not (root / d).is_symlink())
                for filename in sorted(files):
                    path = root / filename
                    if (path == pending or path.is_symlink() or not path.is_file()
                        or filename in {".DS_Store", ".env"} or filename.startswith(".env.")
                        or path.suffix in {".pyc", ".skill"}):
                        continue
                    archive.write(path, path.relative_to(source.parent))
        pending.replace(destination)
    finally:
        pending.unlink(missing_ok=True)
    print(f"Packaged {destination}")
    return destination


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("skill_directory", type=Path)
    parser.add_argument("output_directory", type=Path, nargs="?")
    args = parser.parse_args()
    raise SystemExit(0 if package_skill(args.skill_directory, args.output_directory) else 1)
