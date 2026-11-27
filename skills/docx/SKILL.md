---
name: docx
description: Create, read or edit Word documents and templates (.docx, .dotx), including layout, tracked changes, comments and preserving existing OOXML parts.
compatibility: Uses local tools by task. Node.js with docx for creation, Python 3.10+ with defusedxml and lxml for package edits, Pandoc or python-docx for extraction. Poppler can rasterize a supplied PDF preview.
license: Complete terms in LICENSE.txt
---

# DOCX creation, editing, and analysis

A `.docx` is a ZIP archive of XML files. Choose your approach by task:

| Task | Approach |
|---|---|
| **Create** a new document | Write a `docx` (npm) script — see gotchas below |
| **Edit** an existing document | unpack → edit `word/document.xml` → `zip` (docx-js cannot open existing files) |
| **Read** content | `pandoc -t markdown file.docx` or python-docx for paragraphs and tables |

Run commands in the current Anda workspace. In the examples, `SKILL_DIR` is the absolute `base_dir` returned for this skill by `skills_manager`. When input is an attachment, call `resources_api` with `DownloadResource` and its `_id`; use the returned path. Write edits to a new workspace file and return its absolute path.

## Creating with docx-js — gotchas

From the working project, check that Node can resolve `docx` before choosing this route.

- **Page size defaults to A4.** For US Letter set `page: { size: { width: 12240, height: 15840 } }` (DXA; 1440 = 1″).
- **Landscape:** pass portrait dimensions and `orientation: PageOrientation.LANDSCAPE` — docx-js swaps width/height internally.
- **Tables need dual widths:** set `columnWidths` on the table AND `width` on every cell, both in `WidthType.DXA` (PERCENTAGE breaks in Google Docs). Column widths must sum to the table width.
- **Table shading:** use `ShadingType.CLEAR`, never `SOLID` (renders black).
- **Lists:** never insert `•` literally; use a `numbering` config with `LevelFormat.BULLET`.
- **`ImageRun` requires `type:`** (`"png"`, `"jpg"`, …).
- **`PageBreak` must be inside a `Paragraph`.**
- **Never use `\n`** — use separate `Paragraph` elements.
- **Headings:** style every level you use (`title`, `heading1`, `heading2`…) in `styles.default`: `heading1: { run: { font: "Arial", size: 32, bold: true }, paragraph: { spacing: { before: 240, after: 120 }, keepNext: true } }`. Every level needs its own font (even with a document default), `before` and `after` (even if 0) and `keepNext`: the built-in styles set none of them.
- **TOC:** headings must use built-in `HeadingLevel.*`; custom heading styles need `outlineLevel` set or they won't appear.
- **Don't use a table as a horizontal rule** — use a paragraph bottom border instead.
- **Dot-leader / right-aligned-on-same-line:** use `PositionalTab` (`alignment: PositionalTabAlignment.RIGHT`, `leader: PositionalTabLeader.DOT`) inside a `TextRun`, not literal `.` or space padding.

## Verify the output

Reopen the output and check the requested text, headings, tables and relationships. Run the bundled XML validator. When editing a template, pass the original file to identify newly introduced validation errors.

```sh
python3 "$SKILL_DIR/scripts/office/validate.py" output.docx
```

For pagination and line wrapping, inspect the document in a local Word-compatible viewer when available. If a PDF exported from the same revision is supplied, render its pages with `pdftoppm` and inspect them. Report which visual checks were possible.

## Editing existing documents

For a legacy binary `.doc`, use a local application's Save As DOCX if available, or request an OOXML copy.

```bash
python3 -c "import sys,zipfile; zipfile.ZipFile(sys.argv[1]).extractall('unpacked')" doc.docx   # not unzip, which restores symlinks — docx from external parties is untrusted
python3 "$SKILL_DIR/scripts/merge_runs.py" unpacked/   # coalesce fragmented runs so text is findable
# edit unpacked/word/document.xml in place — do NOT reformat or pretty-print
(cd unpacked && zip -XrD ../out.docx .)   # use a fresh output filename
python3 "$SKILL_DIR/scripts/office/validate.py" out.docx --original doc.docx   # XSD checks; --auto-repair fixes common issues
# redlining? add --author "<the name you redlined under>" to check every edit is tracked
```

Word splits text across many `<w:r>` runs (revision ids, spell-check markers), so a phrase you can see in the document often doesn't exist as a contiguous string in the XML. `merge_runs.py` merges adjacent identically-formatted runs in `word/document.xml` without changing content or rendering; it also accepts a `.docx` directly (`python3 "$SKILL_DIR/scripts/merge_runs.py" doc.docx -o merged.docx`).

**Tracked changes:** when redlining, validate with `--author "<the name you redlined under>"` (needs `--original`) — it reports any text you changed without a `<w:ins>`/`<w:del>` around it, which is easy to do by accident and invisible in the accepted view. Wrap runs in `<w:ins>`/`<w:del>` with `w:id`, `w:author`, `w:date` attributes. Inside `<w:del>`, the text element is `<w:delText>`, not `<w:t>`. A deleted paragraph mark (`<w:pPr><w:rPr><w:del w:id=".." w:author=".." w:date=".."/></w:rPr></w:pPr>`) means "merge this paragraph into the next" — so deleting a paragraph outright is that plus a `<w:del>` around every run. The `<w:del/>` must come before the rPr's other children; their order is schema-enforced.

For a clean copy, run `python3 "$SKILL_DIR/scripts/accept_changes.py" in.docx out.docx`. It accepts inserted or deleted text, moved runs, common formatting changes and deleted paragraph marks directly in the package. It reports structural revisions that need a Word editor, such as tracked table-cell merges. Keep the original and check paragraphs and lists around accepted deletions.

## Comments

Comments require six cross-linked files. Use the helper — directory mode when you'll also be editing `document.xml` (saves an unzip/rezip cycle), `.docx`-direct mode otherwise:

```bash
# Against an already-unpacked directory (preferred when also placing markers)
python3 "$SKILL_DIR/scripts/comment.py" unpacked/ "Fees & expenses cap is too low"
python3 "$SKILL_DIR/scripts/comment.py" unpacked/ "Agreed" --parent 0

# Against a .docx directly
python3 "$SKILL_DIR/scripts/comment.py" contract.docx "This cap is too low" -o annotated.docx
```

The script writes `comments.xml`, `commentsExtended.xml`, `commentsIds.xml`, `commentsExtensible.xml`, the relationships, and the content-type overrides. Comment IDs are auto-assigned. It then prints the `<w:commentRangeStart>`/`<w:commentRangeEnd>`/`<w:commentReference>` snippet to add to `word/document.xml` so the comment anchors to specific text — until you place those markers, the comment exists but is not visible.
