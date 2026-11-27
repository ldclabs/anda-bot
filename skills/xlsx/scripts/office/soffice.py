"""Run the local LibreOffice with a private profile and a sanitized render copy."""

from __future__ import annotations

import contextlib
import os
import re
import shutil
import subprocess
import tempfile
import zipfile
from collections.abc import Iterable
from pathlib import Path


def get_soffice_env() -> dict:
    env = os.environ.copy()
    env.setdefault("SAL_USE_VCLPLUGIN", "svp")
    return env


def run_soffice(args: Iterable[str], **kwargs) -> subprocess.CompletedProcess:
    args = list(args)
    with contextlib.ExitStack() as stack:
        if not any(str(a).startswith("-env:UserInstallation") for a in args):
            profile = stack.enter_context(
                tempfile.TemporaryDirectory(prefix="lo_profile_", ignore_cleanup_errors=True)
            )
            args = [f"-env:UserInstallation={Path(profile).as_uri()}"] + args
        if _draws_the_document(args):
            copies = Path(stack.enter_context(
                tempfile.TemporaryDirectory(prefix="lo_input_", ignore_cleanup_errors=True)
            ))
            args = [_without_outside_links(str(a), copies / str(i)) for i, a in enumerate(args)]
        return subprocess.run(["soffice"] + args, env=get_soffice_env(), **kwargs)



_DRAWN_FORMATS = frozenset(
    {"pdf", "png", "jpg", "jpeg", "gif", "bmp", "tif", "tiff", "webp", "svg", "eps", "emf", "wmf"}
)
_WEB_OR_MAIL = re.compile(r"^(?:https?|mailto):", re.I)
_RELS_NS = "http://schemas.openxmlformats.org/package/2006/relationships"
_NO_RELS = f'<Relationships xmlns="{_RELS_NS}"/>'.encode()
_MAX_RELS_BYTES = 32 * 1024 * 1024


def _draws_the_document(args: list) -> bool:
    for flag, value in zip(args, args[1:]):
        if str(flag) in ("--convert-to", "-convert-to"):
            return str(value).split(":")[0].lower() in _DRAWN_FORMATS
    return False


def _cut_outside_links(rels: bytes) -> bytes | None:
    from defusedxml import ElementTree

    try:
        root = ElementTree.fromstring(rels)
    except (ElementTree.ParseError, ValueError, LookupError):
        return _NO_RELS
    cut = False
    for entry in root.iter():
        if entry.get("TargetMode", "Internal") == "Internal":
            continue
        if _WEB_OR_MAIL.match(entry.get("Target", "")):
            continue
        entry.set("TargetMode", "External")
        entry.set("Target", "about:blank")
        cut = True
    if not cut:
        return None
    for element in root.iter():
        element.tag = element.tag.rsplit("}", 1)[-1]
    root.set("xmlns", _RELS_NS)
    return ElementTree.tostring(root)


def _without_outside_links(arg: str, folder: Path) -> str:

    if not os.path.isfile(arg) or not zipfile.is_zipfile(arg):
        return arg
    with open(arg, "rb") as file:
        if file.read(4) != b"PK\x03\x04":
            return arg
    with zipfile.ZipFile(arg) as package:
        cut = {}
        for member in package.infolist():
            if member.filename.lower().endswith(".rels"):
                try:
                    data = package.read(member) if member.file_size <= _MAX_RELS_BYTES else None
                except Exception:
                    data = None
                rels = _NO_RELS if data is None else _cut_outside_links(data)
                if rels is not None:
                    cut[member.filename] = rels
        if not cut:
            return arg
        folder.mkdir()
        copy = folder / Path(arg).name
        with zipfile.ZipFile(copy, "w") as out:
            for member in package.infolist():
                entry = zipfile.ZipInfo(member.filename, member.date_time)
                entry.compress_type = member.compress_type
                if member.filename in cut:
                    entry.compress_type = zipfile.ZIP_DEFLATED
                    out.writestr(entry, cut[member.filename])
                else:
                    with package.open(member) as source, out.open(entry, "w", force_zip64=True) as target:
                        shutil.copyfileobj(source, target)
    return str(copy)


if __name__ == "__main__":
    import sys
    result = run_soffice(sys.argv[1:])
    sys.exit(result.returncode)
