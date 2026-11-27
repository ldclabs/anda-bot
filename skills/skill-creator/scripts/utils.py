"""Read the UTF-8 frontmatter used by the skill creation helpers."""

from pathlib import Path

try:
    import yaml
except ImportError:
    raise SystemExit("The offline skill helpers require PyYAML. Anda can also validate content with skills_api.")


def split_skill_md(content: str) -> tuple[dict, str]:
    lines = content.strip().splitlines(keepends=True)
    if not lines or lines[0].strip() != "---":
        raise ValueError("SKILL.md is missing the opening ---")
    for end, line in enumerate(lines[1:], 1):
        if line.strip() == "---":
            try:
                frontmatter = yaml.safe_load("".join(lines[1:end]))
            except yaml.YAMLError as error:
                raise ValueError(f"Invalid YAML frontmatter: {error}") from error
            if not isinstance(frontmatter, dict):
                raise ValueError("Frontmatter must be a mapping")
            return frontmatter, "".join(lines[end + 1:])
    raise ValueError("SKILL.md is missing the closing ---")


def parse_skill_md(skill_path: Path) -> tuple[str, str, str]:
    content = (skill_path / "SKILL.md").read_text(encoding="utf-8")
    frontmatter, _ = split_skill_md(content)
    return frontmatter["name"], frontmatter["description"], content
