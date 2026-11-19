"""
Excel Formula Recalculation Script
Recalculates all formulas in an Excel file using LibreOffice
"""

import argparse
import json
import shutil
import subprocess
import tempfile
from contextlib import closing
from pathlib import Path
from zipfile import ZipFile

from openpyxl import load_workbook
from office.soffice import get_soffice_env

# Calc's OOXMLRecalcMode 0 forces all formulas to be recalculated on load.
CALC_PROFILE = """<?xml version="1.0" encoding="UTF-8"?>
<oor:items xmlns:oor="http://openoffice.org/2001/registry">
  <item oor:path="/org.openoffice.Office.Calc/Formula/Load">
    <prop oor:name="OOXMLRecalcMode" oor:op="fuse"><value>0</value></prop>
  </item>
</oor:items>"""


def recalc(filename, timeout=30):
    path = Path(filename).resolve()
    if not path.is_file():
        return {"error": f"File {filename} does not exist"}
    if timeout <= 0:
        return {"error": "Timeout must be positive"}
    extension = path.suffix.lower()
    if extension not in {".xlsx", ".xlsm", ".xltx", ".xltm"}:
        return {"error": f"Unsupported workbook format: {extension}"}

    try:
        # Keep the input until the export has been checked. The private profile
        # also keeps an open interactive Calc session out of the conversion.
        with tempfile.TemporaryDirectory(dir=path.parent) as temporary:
            staging = Path(temporary)
            profile = staging / "profile"
            (profile / "user").mkdir(parents=True)
            (profile / "user/registrymodifications.xcu").write_text(CALC_PROFILE, encoding="utf-8")
            result = subprocess.run(
                [
                    "soffice",
                    f"-env:UserInstallation={profile.as_uri()}",
                    "--headless",
                    "--convert-to",
                    extension[1:],
                    "--outdir",
                    str(staging),
                    str(path),
                ],
                capture_output=True,
                text=True,
                timeout=timeout,
                check=True,
                env=get_soffice_env(),
            )
            staged = staging / (path.stem + extension)
            if not staged.is_file():
                detail = result.stderr.strip() or result.stdout.strip()
                return {"error": detail or "LibreOffice did not export the workbook"}
            if extension in {".xlsm", ".xltm"}:
                with ZipFile(path) as original, ZipFile(staged) as exported:
                    project = "xl/vbaProject.bin"
                    if project in original.namelist() and (
                        project not in exported.namelist()
                        or original.read(project) != exported.read(project)
                    ):
                        return {"error": "LibreOffice did not preserve the workbook's VBA project"}

            errors = {}
            total_errors = 0
            with closing(load_workbook(staged, data_only=True, read_only=True)) as workbook:
                for sheet in workbook:
                    for row in sheet.iter_rows():
                        for cell in row:
                            if cell.data_type == "e":
                                total_errors += 1
                                detail = errors.setdefault(cell.value, {"count": 0, "locations": []})
                                detail["count"] += 1
                                if len(detail["locations"]) < 20:
                                    detail["locations"].append(f"{sheet.title}!{cell.coordinate}")

            with closing(load_workbook(staged, read_only=True)) as workbook:
                formula_count = sum(
                    cell.data_type == "f"
                    for sheet in workbook
                    for row in sheet.iter_rows()
                    for cell in row
                )
            shutil.copymode(path, staged)
            staged.replace(path)
        return {
            "status": "errors_found" if total_errors else "success",
            "total_errors": total_errors,
            "error_summary": errors,
            "total_formulas": formula_count,
        }
    except subprocess.TimeoutExpired:
        return {"error": f"LibreOffice timed out after {timeout} seconds"}
    except subprocess.CalledProcessError as err:
        return {"error": err.stderr.strip() or str(err)}
    except Exception as err:
        return {"error": str(err)}


def main():
    parser = argparse.ArgumentParser(description=__doc__.strip())
    parser.add_argument("filename")
    parser.add_argument("timeout_seconds", type=int, nargs="?", default=30)
    args = parser.parse_args()
    result = recalc(args.filename, args.timeout_seconds)
    print(json.dumps(result, indent=2))
    if "error" in result:
        raise SystemExit(1)


if __name__ == "__main__":
    main()
