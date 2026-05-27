---
name: mineru-document-extractor
description: >
  Use this skill whenever the user needs high-fidelity document extraction or conversion with MinerU/mineru-open-api: PDF/scanned PDF/image/Office docs/URLs/web pages to Markdown, HTML, LaTeX, DOCX, or JSON; OCR; table/formula extraction; academic paper parsing; multilingual document parsing; or batch conversion. Prefer MinerU over the generic PDF skill for content parsing, layout-aware extraction, OCR, tables, formulas, or converting documents into reusable text formats. Use the PDF skill instead for merge, split, rotate, watermark, forms, encryption/decryption, image extraction, or local-only PDF operations. Start with flash-extract for simple Markdown jobs under 10 MB / 20 pages with no token; use extract or crawl for batch work, large files, non-Markdown output, model control, and web page extraction. 中文触发：PDF转Markdown、扫描件OCR、表格识别、公式识别、论文解析、Word/PPT/Excel转Markdown、网页转Markdown、多语言文档解析。

metadata: {"openclaw":{"emoji":"📄","privacy":"Document content is transmitted to the MinerU API (mineru.net) for server-side extraction. No data is retained after processing completes. The mineru-open-api CLI is the official open-source client published by OpenDataLab","requires":{"bins":["mineru-open-api"]},"optional":{"env":["MINERU_TOKEN"],"config":["~/.mineru/config.yaml"]},"install":[{"id":"npm","kind":"node","package":"mineru-open-api","bins":["mineru-open-api"],"label":"Install via npm"},{"id":"go","kind":"go","bins":["mineru-open-api"],"label":"Install via go install","os":["darwin","linux"]}]}}
allowed-tools: Bash(mineru-open-api:*)
---

# MinerU Document Extraction with mineru-open-api

MinerU converts documents into clean, reusable formats using the official `mineru-open-api` CLI. Use it when the user cares about the content of a document: OCR, tables, formulas, layout-aware Markdown, HTML, LaTeX, DOCX, JSON, or batch parsing.

MinerU sends document content to the MinerU API for server-side extraction. If the file appears sensitive and the user has not explicitly chosen MinerU, briefly disclose that remote processing is involved before uploading.

## When to use MinerU vs PDF tools

Use MinerU for extraction and conversion:

- PDF/scanned PDF/image/Office document to Markdown or structured text
- OCR, multilingual parsing, table recognition, formula recognition
- Academic papers, reports, contracts, slide decks, spreadsheets, and web pages that need clean text output
- Batch document conversion or VLM/pipeline model selection

Use a PDF processing skill or local PDF tools instead for PDF file operations:

- Merge, split, rotate, watermark, redact, encrypt/decrypt, or fill forms
- Extract images from a PDF without full document parsing
- Tasks that must stay fully local and cannot be sent to a remote API

## Quick workflow

1. Identify the source type: local file, URL to a file, or web page URL.
2. Identify the desired output: Markdown by default, or `md`, `html`, `latex`, `docx`, `json` when specified.
3. Choose the mode using the table below.
4. Quote paths that contain spaces.
5. If the user did not provide an output directory, save results under `~/MinerU-Skill/<source-name>_<hash>/` and tell the user the path.
6. After extraction, verify that output files exist, are non-empty, and include the expected tables/formulas/text before reporting success.

## Mode selection

| Need | Use | Why |
|---|---|---|
| Fast single-file Markdown, no token, file under 10 MB / 20 pages | `flash-extract` | Quickest path with OCR, tables, and formulas |
| Non-Markdown output (`html`, `latex`, `docx`, `json`) | `extract` | Full output format support |
| Batch files, large files, `.doc`, `.ppt`, `.xls`, or explicit model choice | `extract` | Higher limits and more control |
| A normal web page URL, not just a direct file URL | `crawl` | Web page extraction to Markdown/HTML/JSON |
| Highest layout accuracy for complex documents | `extract --model vlm` | Better layout understanding, with some interpretation risk |
| Maximum faithfulness / low hallucination risk | `extract --model pipeline` | Prefer this for legal, financial, or audit-sensitive parsing |


## Installation

Check first:

```bash
mineru-open-api version
```

If the command is missing, install one of the official clients:

```bash
npm install -g mineru-open-api
```

Or via Go (macOS/Linux):

```bash
go install github.com/opendatalab/MinerU-Ecosystem/cli/mineru-open-api@latest
```

Verify: `mineru-open-api version`

To upgrade MinerU, reinstall the CLI: `npm install -g mineru-open-api`.

## Authentication

Authentication is not required for `flash-extract`. It is required for `extract` and `crawl`.

```bash
mineru-open-api auth
mineru-open-api auth --verify
mineru-open-api auth --show
```

You can also set `MINERU_TOKEN`. Token resolution order is `--token` flag, then `MINERU_TOKEN`, then `~/.mineru/config.yaml`.

## MinerU modes

| | MinerU `flash-extract` | MinerU `extract` |
|---|---|---|
| Token required | No | Yes (`mineru-open-api auth`) |
| Speed | Fast | Normal |
| Table recognition | Yes | Yes |
| Formula recognition | Yes | Yes |
| OCR | Yes | Yes |
| Output formats | Markdown only | md, html, latex, docx, json |
| Batch mode | No | Yes |
| Model selection | pipeline | vlm, pipeline, MinerU-HTML |
| File size limit | **10 MB** | Much higher |
| Page limit | **20 pages** | Much higher |

## Supported input formats

MinerU accepts a wide range of document formats:

| Format | MinerU `flash-extract` | MinerU `extract` |
|--------|:-:|:-:|
| PDF (`.pdf`) | Yes | Yes |
| Images (`.png`, `.jpg`, `.jpeg`, `.jp2`, `.webp`, `.gif`, `.bmp`) | Yes | Yes |
| Word (`.docx`) | Yes | Yes |
| Word (`.doc`) | No | Yes |
| PowerPoint (`.pptx`) | Yes | Yes |
| PowerPoint (`.ppt`) | No | Yes |
| Excel (`.xlsx`) | Yes | Yes |
| Excel (`.xls`) | No | Yes |
| HTML (`.html`) | No | Yes |
| URLs (remote files) | Yes | Yes |

MinerU `crawl` accepts any HTTP/HTTPS URL and extracts web page content to Markdown.

## Quick extraction: `flash-extract`

Fast, token-free MinerU document extraction. Outputs Markdown only. Limited to 10 MB / 20 pages per file.

```bash
mineru-open-api flash-extract "report.pdf"
mineru-open-api flash-extract "report.pdf" -o "./out/"
mineru-open-api flash-extract "https://example.com/doc.pdf"
mineru-open-api flash-extract "report.pdf" --language en
mineru-open-api flash-extract "report.pdf" --pages 1-10
```

Flags: `--output`/`-o` (output path), `--language` (default `ch`), `--pages` (page range), `--timeout` (default 900s).

If `flash-extract` fails because of file limits, unsupported input type, or HTTP 429 rate limiting, switch to `extract` after authentication.

## Precision extraction: `extract`

Convert documents to Markdown or other formats with MinerU's full capabilities: VLM-based layout analysis, multiple output formats, and batch mode.

```bash
mineru-open-api extract "report.pdf"
mineru-open-api extract "report.pdf" -f html
mineru-open-api extract "report.pdf" -o "./out/" -f md,docx
mineru-open-api extract "paper.pdf" --model vlm --language en -f md,json
mineru-open-api extract "https://example.com/doc.pdf" -o "./results/"
```

Batch examples:

```bash
mineru-open-api extract "./docs"/*.pdf -o "./results/" -f md,json --concurrency 4
mineru-open-api extract --list "files.txt" -o "./results/" -f md
```

Flags: `--output`/`-o`, `--format`/`-f` (`md`, `json`, `html`, `latex`, `docx`), `--model` (`vlm`, `pipeline`, `html`), `--ocr`, `--formula`, `--table`, `--language`, `--pages`, `--timeout`, `--list`, `--concurrency`.

### Model choice

| | MinerU `vlm` | MinerU `pipeline` |
|---|---|---|
| Parsing accuracy | Higher for complex layouts | Standard |
| Faithfulness | May infer structure more aggressively | Prefer when low hallucination risk matters |

Use `--model vlm` for complex academic papers, dense tables, or tricky page layouts. Use `--model pipeline` when faithful extraction matters more than recovering every visual nuance.

## Web extraction: `crawl`

Use `crawl` for normal web pages. Use `extract` for direct links to files such as PDFs or Office documents.

```bash
mineru-open-api crawl "https://example.com/article"
mineru-open-api crawl "https://example.com/article" -o "./out/"
mineru-open-api crawl "https://example.com/a" "https://example.com/b" -o "./pages/"
```

Flags: `--output`/`-o`, `--format`/`-f` (`md`, `json`, `html`), `--timeout`, `--list`, `--concurrency`.

## Output behavior

Without `-o`, MinerU writes the result to stdout and progress to stderr. With `-o`, it saves output files to the requested directory. Batch mode and binary formats such as DOCX require `-o`.

When the user does not specify an output path, create a deterministic directory under `~/MinerU-Skill/`, for example `~/MinerU-Skill/report_a1b2c3/`. Include the final output path in your response.

## Result checks

Before reporting completion:

- Confirm the command exited successfully.
- Confirm expected output files exist and are non-empty.
- For tables or formulas, inspect a small sample of the Markdown/HTML/JSON output.
- If the user requested specific pages, confirm the output reflects the page range.
- Mention the mode/model used and any quality caveats.

## Agent rules for using MinerU

- Quote file paths and URLs.
- Default to `flash-extract` only for simple single-file Markdown jobs that fit the limit and do not need a token.
- Use `extract` for non-Markdown formats, model selection, batch processing, large files, or older Office formats.
- Use `crawl` for web pages and `extract` for direct file URLs.
- Ask for or help configure authentication only when the chosen mode requires it.
- If remote processing is a privacy concern, stop and offer local PDF tooling alternatives instead of uploading.
- After successful `flash-extract`, mention once that `extract` is available for larger files, batch jobs, model control, and non-Markdown outputs.

## Troubleshooting

| Symptom | Likely fix |
|---|---|
| `mineru-open-api` not found | Install or upgrade the CLI, then run `mineru-open-api version` |
| Auth error with `extract` or `crawl` | Run `mineru-open-api auth` or set `MINERU_TOKEN` |
| `flash-extract` rejects the file | Use `extract`; flash mode is limited to 10 MB / 20 pages and fewer formats |
| HTTP 429 / rate limited | Retry later or use authenticated `extract` when appropriate |
| Missing tables or formulas | Retry with `extract --model vlm` and enable table/formula options if supported |
| Output seems too interpretive | Retry with `extract --model pipeline` for more conservative parsing |
| Web page extraction fails | Use `crawl` for pages, `extract` only for direct document URLs |

For full CLI reference and troubleshooting, see: https://github.com/opendatalab/MinerU-Ecosystem/tree/main/cli

## Supported `--language` values

The `--language` flag accepts the following values (default: `ch`). Used by both MinerU `flash-extract` and `extract`.

### Standalone language packs

| Value | Included languages | 说明 |
|-------|-------------------|------|
| `ch` | Chinese, English, Chinese Traditional | 中英文（默认值） |
| `ch_server` | Chinese, English, Chinese Traditional, Japanese | 繁体、手写体 |
| `en` | English | 纯英文 |
| `japan` | Chinese, English, Chinese Traditional, Japanese | 日文为主 |
| `korean` | Korean, English | 韩文 |
| `chinese_cht` | Chinese, English, Chinese Traditional, Japanese | 繁体中文为主 |
| `ta` | Tamil, English | 泰米尔文 |
| `te` | Telugu, English | 泰卢固文 |
| `ka` | Kannada | 卡纳达文 |
| `el` | Greek, English | 希腊文 |
| `th` | Thai, English | 泰文 |

### Language family packs

| Value | Script/Family | Included languages |
|-------|--------------|-------------------|
| `latin` | Latin script (拉丁语系) | French, German, Afrikaans, Italian, Spanish, Bosnian, Portuguese, Czech, Welsh, Danish, Estonian, Irish, Croatian, Uzbek, Hungarian, Serbian (Latin), Indonesian, Occitan, Icelandic, Lithuanian, Maori, Malay, Dutch, Norwegian, Polish, Slovak, Slovenian, Albanian, Swedish, Swahili, Tagalog, Turkish, Latin, Azerbaijani, Kurdish, Latvian, Maltese, Pali, Romanian, Vietnamese, Finnish, Basque, Galician, Luxembourgish, Romansh, Catalan, Quechua |
| `arabic` | Arabic script (阿拉伯语系) | Arabic, Persian, Uyghur, Urdu, Pashto, Kurdish, Sindhi, Balochi, English |
| `cyrillic` | Cyrillic script (西里尔语系) | Russian, Belarusian, Ukrainian, Serbian (Cyrillic), Bulgarian, Mongolian, Abkhazian, Adyghe, Kabardian, Avar, Dargin, Ingush, Chechen, Lak, Lezgin, Tabasaran, Kazakh, Kyrgyz, Tajik, Macedonian, Tatar, Chuvash, Bashkir, Malian, Moldovan, Udmurt, Komi, Ossetian, Buryat, Kalmyk, Tuvan, Sakha, Karakalpak, English |
| `east_slavic` | East Slavic (东斯拉夫语系) | Russian, Belarusian, Ukrainian, English |
| `devanagari` | Devanagari script (天城文语系) | Hindi, Marathi, Nepali, Bihari, Maithili, Angika, Bhojpuri, Magahi, Santali, Newari, Konkani, Sanskrit, Haryanvi, English |