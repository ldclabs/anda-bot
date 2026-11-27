---
name: pdf
description: Read, create, transform or verify PDF files, including form filling, page operations, table extraction and OCR of scanned pages.
license: Complete terms in LICENSE.txt
compatibility: Uses local PDF tools according to the operation, such as Poppler, pypdf, pdfplumber, ReportLab or Tesseract.
---

# Work with PDFs

For a question about an attached document, `attachment_understanding` can inspect its resource ID. When a program needs the bytes, use `resources_api` with `DownloadResource` and the attachment's `_id`. Use the path it returns.

Run available PDF tools through `shell` in the current workspace. Use the skill's `base_dir` returned by `skills_manager` to locate bundled helpers. Keep the original and write to a distinct output file. Read the relevant guide with `skills_read`, passing `pdf` as `skill` and the exact filename as `resource`. Follow `next_cursor` until the guide is complete.

| Task | Local workflow |
| --- | --- |
| Read text or tables | Use `pdftotext -layout input.pdf -` for text and `pdfplumber` for tables that need geometry. Check page order and column boundaries against the original. |
| Read a scan | Render the needed pages with Poppler and use OCR configured for the document language. Check names, numbers and uncertain characters against the image. |
| Merge, split, rotate or encrypt | Use `pypdf` or `qpdf`. Reopen the result and check page count, ordering, rotations and any requested encryption. |
| Generate a PDF | Use ReportLab or a local renderer suited to the source format. Embed a font with the required glyphs and render the output to inspect layout. |
| Fill a form | Follow the [form workflow](forms.md). It distinguishes real PDF fields from flat pages and includes coordinate and visual checks. |
| Extract images or use advanced APIs | See the [PDF reference](reference.md) for Poppler, pypdfium2, pdf-lib and worked examples. |

For a file deliverable, return its absolute workspace path. If a necessary library or renderer is unavailable, identify the affected operation or check.
