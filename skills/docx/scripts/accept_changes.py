"""Accept all tracked changes in a DOCX file using LibreOffice.

Requires LibreOffice Writer with Basic macro support.
"""

import argparse
import shutil
import subprocess
import tempfile
from pathlib import Path

from office.soffice import get_soffice_env

ACCEPT_CHANGES_MACRO = """<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE script:module PUBLIC "-//OpenOffice.org//DTD OfficeDocument 1.0//EN" "module.dtd">
<script:module xmlns:script="http://openoffice.org/2000/script" script:name="Module1" script:language="StarBasic">
    Sub AcceptAllTrackedChanges()
        Dim document As Object
        Dim dispatcher As Object
        Dim args(0) As New com.sun.star.beans.PropertyValue
        document = ThisComponent
        dispatcher = createUnoService("com.sun.star.frame.DispatchHelper")
        dispatcher.executeDispatch(document.CurrentController.Frame, ".uno:AcceptAllTrackedChanges", "", 0, Array())
        If document.Redlines.Count = 0 Then
            args(0).Name = "FilterName"
            args(0).Value = "Office Open XML Text"
            document.storeToURL(document.URL &amp; ".accepted.docx", args())
        End If
        document.setModified(False)
        document.close(True)
        StarDesktop.terminate()
    End Sub
</script:module>"""


def accept_changes(
    input_file: str,
    output_file: str,
) -> tuple[None, str]:
    input_path = Path(input_file)
    output_path = Path(output_file)

    if not input_path.is_file():
        return None, f"Error: Input file not found: {input_file}"

    if input_path.suffix.lower() != ".docx":
        return None, f"Error: Input file is not a DOCX file: {input_file}"

    try:
        output_path = output_path.resolve()
        output_path.parent.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(dir=output_path.parent) as temporary:
            staging = Path(temporary)
            document = staging / "document.docx"
            shutil.copyfile(input_path, document)
            # A separate application profile keeps this macro and its shutdown
            # away from interactive sessions and other simultaneous conversions.
            profile = staging / "profile"
            command = ["soffice", f"-env:UserInstallation={profile.as_uri()}", "--headless"]
            env = get_soffice_env()
            subprocess.run(
                command + ["--terminate_after_init"],
                capture_output=True,
                text=True,
                timeout=10,
                check=True,
                env=env,
            )
            macro = profile / "user/basic/Standard/Module1.xba"
            macro.write_text(ACCEPT_CHANGES_MACRO, encoding="utf-8")
            subprocess.run(
                command
                + [
                    "--norestore",
                    str(document),
                    "macro:///Standard.Module1.AcceptAllTrackedChanges()",
                ],
                capture_output=True,
                text=True,
                timeout=30,
                check=True,
                env=env,
            )
            accepted = staging / "document.docx.accepted.docx"
            if not accepted.is_file():
                return None, "Error: LibreOffice did not produce a document with all changes accepted"
            shutil.copymode(input_path, accepted)
            accepted.replace(output_path)
    except subprocess.TimeoutExpired:
        return None, "Error: LibreOffice timed out accepting tracked changes"
    except subprocess.CalledProcessError as err:
        return None, f"Error: LibreOffice failed: {err.stderr.strip() or err}"
    except OSError as err:
        return None, f"Error: {err}"

    return None, f"Successfully accepted all tracked changes: {input_file} -> {output_file}"


if __name__ == "__main__":
    parser = argparse.ArgumentParser(
        description="Accept all tracked changes in a DOCX file"
    )
    parser.add_argument("input_file", help="Input DOCX file with tracked changes")
    parser.add_argument(
        "output_file", help="Output DOCX file (clean, no tracked changes)"
    )
    args = parser.parse_args()

    _, message = accept_changes(args.input_file, args.output_file)
    print(message)

    if message.startswith("Error:"):
        raise SystemExit(1)
