---
name: xlsx
description: Create, edit or analyze Excel workbooks and templates or tabular CSV/TSV files. Use for data cleanup, formulas, formatting, charts and file conversion.
license: Complete terms in LICENSE.txt
compatibility: Python's csv module handles CSV and TSV. Use openpyxl for Excel files, or an existing pandas installation for bulk analysis.
---

# Work with spreadsheets

Use the local Python environment through `shell` in the current Anda workspace. For an attachment, download its `_id` with `resources_api` (`DownloadResource`) and use the returned path. Keep the original file and write edits to a distinct output path.

| Task | Approach |
| --- | --- |
| Inspect or edit an existing `.xlsx` or `.xltx` | Open with `openpyxl.load_workbook` in its default formula-preserving mode. Inspect sheet names, headers, dimensions, styles and merged cells before editing. |
| Edit a macro-enabled workbook | Use `keep_vba=True` and preserve the `.xlsm` or `.xltm` extension. When shapes, slicers or unsupported features must survive, edit only the relevant worksheet XML part in a copy of the package. |
| Create a new workbook | Use openpyxl or an available XlsxWriter installation. Set column widths and date, currency and percentage formats explicitly. |
| Read or write CSV/TSV | Use `csv` with `newline=""`, the source encoding and the correct delimiter. Preserve significant leading zeros and identifiers as text. Use pandas when its type inference and data operations suit the task. |

## Editing a workbook

- Match the existing sheet names, input conventions, formatting and units. Change only the requested cells. A merged range stores its value at the top-left cell.
- Use Excel formulas for values that should respond to edits. Write functions in English, quote sheet names containing spaces, and use the workbook's conventions for relative and absolute references. Excel calculates formula results when it opens the workbook.
- `data_only=True` reads the cached result last saved by a spreadsheet application. Use this mode only for reading. Saving that view can replace formulas with their cached values.
- Keep assumptions that drive financial models in labeled cells. Use number formats for display, e.g. a stored value of `0.15` for `15%`. Preserve established styles when editing a model.
- Save and reopen the output to confirm it is readable and contains the requested sheets, columns and edits. For a data transformation, check row counts and a few representative records. Return the absolute path.

The bundled `scripts/` remain available for explicit maintenance tasks. For a separate request to refresh cached formula values, `scripts/recalc.py` can use an already installed local converter. If none is available it reports `skipped` and leaves the file intact. Routine spreadsheet tasks need only the reader or writer for the chosen format.
