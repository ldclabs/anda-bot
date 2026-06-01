# Changelog

All notable changes to Anda Bot.
## [0.8.7] — 2026-06-01

### Added

- **URL and data-URL support for media understanding agents**: `url` parameter alongside existing `path` in `MediaUnderstandingArgs` — supports `http`/`https`/`data` URLs, streaming HTTP responses with size limit enforcement, base64 and percent-encoded data URL decoding. Shared `reqwest::Client` injected via `with_http_client` builder.
- **Resource persistence and attachment handling**: new `ResourceStore` for persisting resources (images, files) in the database. Integrated into the AndaBot engine with full attachment lifecycle management — from media understanding through to chat message display.
- **Attachment download in Chrome extension**: one-click download for message attachments directly from the chat UI, with progress tracking and concurrent download management.
- **Multi-format attachment support**: new `Other` media type for non-image/audio/video attachments — supports text, PDF, document, spreadsheet, and other file formats. `liteparse` integration for PDF parsing.
- **Pending local attachment sync**: Chrome extension synchronizes locally queued attachments with the server on reconnect, ensuring no attachment is lost during offline usage.
- **Resource API tool**: `resources_api` tool exposed to agents for loading persisted resources by ID, with caching and lazy loading in the Chrome extension.
- **Rich text copy in Chrome extension**: `copyRichMessage()` copies both plain text and rendered HTML to the clipboard via `ClipboardItem`, preserving formatting when pasted into rich-text editors.
- **Print message in Chrome extension**: `printMessage()` opens a styled print window with rendered Markdown content and image attachments, ready for printing or saving as PDF.
- **Comprehensive unit tests for Rust core modules**: 300+ new tests covering attachments, channel types, session formatting, channel/transcription/TTS config, daemon paths, logger, and Ed25519 key operations — all following Rust testing conventions.

### Changed

- **Engine model resolution via active model label**: `ACTIVE_MODEL_LABEL` (`""`) constant.
- **MIME detection improved**: media MIME detection now prefers inferred media MIME, then `Content-Type` header, then filename extension, with a normalized fallback chain for all media understanding paths.
- **Chrome extension message actions refactored**: shared `messageActionButtonClass` extracted; action button row gains Copy, Clipboard, and Printer icons, with consistent hover/focus animations.

### Fixed

- **Musl build compatibility**: `liteparse` default features disabled at workspace level; `tesseract` OCR feature conditionally enabled only on non-musl targets. PDF OCR automatically disabled on musl targets via `cfg!(target_env = "musl")`, enabling static musl builds (Alpine Linux) that previously failed due to unavailable tesseract system libraries.
- **Workspace derivation from source prefix**: when only the `source` field is provided (e.g. from browser extension), extract workspace from the `"cli:"` prefix. Conversations created via non-CLI channels now correctly resolve a valid workspace directory.

### Dependencies

- `anda_brain` 0.6.5 → 0.6.7.
- `anda_core` 0.12.4 → 0.12.5.
- `anda_engine` 0.12.24 → 0.12.26 (`Models::resolve()` simplification — delegates to `get_model()`).
- `anda_db_tfs` 0.1.0 (new, for resource tokenization).
- `liteparse` 2 (new, for PDF parsing).

## [0.8.6] — 2026-05-29

### Added

- **StepFun step-3.7-flash model**: new `step-3.7-flash` entry in `config.yaml` (StepFun API, 256K context, 32K max output, labels: flash/image/video).
- **`effort: high` default for all models**: every model entry in `config.yaml` now specifies `effort: high`, aligning with the brain default `ModelEffort::High`.

### Changed

- **Completion hook reads `SessionRequestMeta` first**: `completion_meta()` now checks agent context state for `SessionRequestMeta` before falling back to `ctx.meta()`, restoring correct cron job route recovery when the runtime meta lacks route info.
- **Direct async send replaces `tokio::spawn`**: `on_completion` now awaits `try_send` directly instead of spawning a detached task, simplifying error handling and ensuring channel bindings are visible immediately.
- **`completion_message` takes `&RequestMeta`**: decoupled from `&AgentCtx`, making it easier to test and reuse.

### Fixed

- **TUI test**: `ContentPart::ToolOutput` in `tui/mod.rs` test now includes `is_error: None` to match the updated struct definition.

### Dependencies

- `anda_brain` 0.6.4 → 0.6.5.
- `anda_cognitive_nexus` 0.7.18 → 0.7.19.
- `anda_core` 0.12.3 → 0.12.4.
- `anda_engine` 0.12.23 → 0.12.24.
- `anda_kip` 0.7.12 → 0.7.13.
- `hyper` 1.9.0 → 1.10.0.
- `candid` 0.10.28 → 0.10.29.
- `zerocopy` 0.8.48 → 0.8.49.
- `displaydoc` 0.2.5 → 0.2.6.
- `socket2` 0.6.3 → 0.6.4.
- `mio` 1.2.0 → 1.2.1.
- `cmov` 0.5.3 → 0.5.4.

## [0.8.5] — 2026-05-27

### Added

- **MinerU document extraction skill**: new `mineru-document-extractor` skill for converting PDFs, scanned documents, images, Office formats (Word DOC/DOCX, PowerPoint PPT/PPTX, Excel XLS/XLSX), and web pages into clean Markdown, HTML, LaTeX, or DOCX via the official `mineru-open-api` CLI. Two extraction modes: flash-extract for instant zero-config conversion (no token, table/formula recognition, OCR) and precision extract with VLM-based layout analysis and batch processing. Covers 80+ languages including Chinese, English, Japanese, Korean, and Arabic.

- **`update_cron_job` tool**: new tool for modifying existing cron jobs without losing their origin metadata or run history. Supports partial updates — pass `null` for fields that should stay unchanged. Changing the schedule recalculates `next_run`; updating only non-schedule fields (job, name) preserves the existing schedule.
- **Cron run ID tracking**: `run_id` is now carried through the full cron execution pipeline — agent job prompts, shell job result notifications, and all log messages — making it straightforward to correlate a specific execution with its conversation and error traces.


## [0.8.4] — 2026-05-25

### Changed

- **Browser script action simplified**: `action` is now optional on `chrome_script` -- defaults to `execute_javascript` implicitly, removing the redundant enum from the tool schema. Models can pass `code` directly.
- **Browser result deduplication**: `tab` is no longer nested inside `page_ready` in browser action results. All navigation actions (`open_tab`, `navigate`, `reload`, `go_back`, `go_forward`, `switch_tab`) use a shared `withTopLevelTab` helper, and `compactPageReadyInfo` ensures `page_ready` never carries duplicate tab data. Updated test assertions to match.
- **`wait_for_navigation` and `wait_for_history_change` removed**: navigation and page-changing actions now rely entirely on built-in `page_ready` (included in `navigate`, `open_tab`, `open_file`, `reload`, `go_back`, `go_forward`, and page-changing `click`/`press_key` actions). The explicit wait actions were redundant and prone to timing races with fast navigations. Removed `expected_url` and `wait_until` fields from `BrowserActionArgs`.

## [0.8.3] — 2026-05-25

### Changed

- **Lightweight UI replaces shadcn-svelte**: removed `shadcn-svelte` and `tailwind-variants` dependencies. 175+ component library files replaced with a single `ui.ts` module exporting shadcn-compatible class generator functions (`buttonClass`, `badgeClass`, `itemClass`, `cardClass`, `inputClass`, `dialogContentClass`, etc.) using Tailwind classes directly. All Anda app components (`App`, `ChatComposer`, `ChatMessageItem`, `ChatSettings`, `AttachmentList`, `PromptCommandPanel`, `VoicePanel`) refactored accordingly. Net deletion: ~3,900 lines.

### Fixed

- **Table rendering**: tables now use `width:max-content` with `min-width:100%` instead of fixed `100%` with `overflow:hidden`, enabling proper responsive behavior.
- **Message spacing**: grid gap increased from 2 to 4 for better visual separation.
- **MiniMax model name**: corrected from `M2.7-highspeed` to `M2.7` in default config.

## [0.8.2] — 2026-05-24

### Highlights

- **shadcn-svelte UI**: the Chrome extension UI has been rebuilt on the shadcn-svelte component library — 30+ component families (accordion, alert, avatar, command, context-menu, dialog, dropdown-menu, field, hover-card, input-group, item, kbd, native-select, popover, select, skeleton, spinner, switch, tabs, tooltip) replacing ad-hoc Svelte markup across ChatComposer, ChatMessageItem, ChatSettings, ChatChannelsSidebar, AttachmentList, PromptCommandPanel, and VoicePanel.
- **$skill-name shorthand**: `$skill-name prompt` is now equivalent to `/skill skill-name prompt` — quick skill routing without typing the leading slash. Both the Rust engine and the Chrome extension completion panel handle the dollar prefix.
- **open_file action**: `chrome_tabs` can now open local files via `file://` URLs. Paths are resolved relative to the workspace and percent-encoded for URLs containing spaces and special characters. Returns metadata including the mime type guessed from file extension.
- **Viewport screenshot controls**: `chrome_page.screenshot` now accepts `viewport_width`, `viewport_height`, and `device_scale_factor` — fixed-size screenshots via CDP device metrics emulation, independent of the browser window size.

### Changed

- **Dependencies**: version 0.8.1→0.8.2.

## [0.8.1] — 2026-05-23

### Browser Extension

- **Debugger reconnection**: commands transparently retry up to 2× on transient "not attached" errors; removed the unconditional detach-before-attach that caused excessive detach calls.
- **Native text input**: `type_text` on editable elements now uses CDP `Input.insertText` (selectAll → Backspace → insert) instead of key-by-key dispatching — faster and more reliable.
- **Mobile touch support**: click dispatches touchStart/touchEnd on pages with mobile user agents or `maxTouchPoints > 1`, matching real device behavior.
- **goBack/goForward fallback**: when the native tabs API throws (e.g. localized errors like "无法在历史记录中找到下一页"), falls back to injecting `pageActionDispatcher` via `scripting.executeScript`.
- **Smarter element resolution**: `deepQuerySelector` uses `preferredMatch()` to select the most interactable element from all `querySelectorAll` matches — `interactable()` checks visibility + `elementFromPoint` hit-test at element center, with fallback through `visible()`, then first match.
- **Key aliases**: Esc→Escape, Return→Enter, Space→Space; added Space key definition.
- **`preferredMatch` for `type_text`**: prefers editable text inputs over other visible elements; falls back to `document.activeElement` when no selector is provided.

### Engine

- **GitHub API version checking**: auto-updater now queries `api.github.com/repos/{REPO}/releases/latest` with anti-cache headers, falling back to the releases page when the API is unavailable.
- **`/stop` stores failed_reason**: the prompt is always stored as `failed_reason` (not just when non-empty) and appended to the conversation as a user message before cancellation.

### Changed

- **Dependencies**: `anda_brain` 0.6.1→0.6.2, `anda_engine` 0.12.19→0.12.20, `wasm-bindgen` 0.2.121→0.2.122, `serde_json` 1.0.149→1.0.150, `lucide-svelte`, `tailwindcss`, `svelte`, `vite`, `vitest`, `katex`, `chrome-types`, `postcss`, `prettier-plugin-svelte`.

## [0.8.0] — 2026-05-22

The Chrome Extension release — Anda Bot now lives in your browser.

### Highlights

- **Chrome Extension (Anda Bot)**: a full-featured browser side panel that connects to the Anda Bot daemon via WebSocket. Chat with your agent, manage conversations and channels, browse files, run slash commands, and control browser tabs — all without leaving Chrome. Published to the Chrome Web Store.
- **Browser Tools**: four focused tools (`chrome_tabs`, `chrome_page`, `chrome_input`, `chrome_script`) give the agent complete control over the browser — navigation, page inspection, screenshots, accessibility trees, PDF printing, form interaction, JavaScript execution, downloads, cookies, cache management, viewport annotations, file uploads, and dialog handling.
- **Auto-Update System**: end-to-end self-update spanning daemon, TUI, Chrome extension, and gateway — checks GitHub releases, downloads platform assets, verifies SHA256 checksums, and installs with restart.
- **Multimodal Media Understanding**: image, video, and audio understanding via model-label routing, with automatic model dispatch based on `image_understanding`, `video_understanding`, or `audio_understanding` labels.
- **Voice in the Browser**: voice input/output, thinking detail display, and voice orb UI in the extension side panel.

### Chrome Extension — Features

- **Side panel UI**: SvelteKit-based side panel with chat, channels, tasks, and settings views.
- **Multi-conversation history**: browse, switch, and manage conversations with real-time polling and local message reconciliation.
- **Channel management**: create, switch, and delete channels with alert dialog confirmation.
- **Slash command palette**: `/new`, `/stop`, and skill commands accessible via keyboard shortcut.
- **Voice I/O**: speech-to-text input, text-to-speech playback, and voice orb visualization.
- **Thinking display**: expandable reasoning traces with configurable detail level.
- **Submit key modes**: choose between "Enter sends" or "Ctrl/Cmd+Enter sends" in settings.
- **Onboarding flow**: guided first-run experience for new users.
- **i18n**: 6 languages — English, Chinese (Simplified), Russian, Arabic, French, Spanish.
- **Runtime model switching**: model selector with live daemon model list and refresh.
- **Auto-update UI**: notification banner when an update is downloaded, with install-and-restart button.
- **WebSocket transient recovery**: automatic reconnection on transient network errors.
- **Vitest test suite**: unit tests for polling, side panel, and voice modules.

### Browser Tools — Capabilities

- **Tab management**: list, open, close, switch, navigate, go back/forward, reload.
- **Page inspection**: snapshot (with links/forms), extract text, screenshot (viewport and full-page), get full HTML, accessibility tree, find in page, viewport size.
- **Viewport annotations**: highlight elements visually via CDP Overlay for annotated screenshots.
- **PDF printing**: generate PDFs from any tab via CDP `Page.printToPDF`.
- **Input interaction**: click, type, press key, scroll, scroll-to (selector or coordinates), hover, drag-and-drop, select dropdown, upload files, copy to clipboard.
- **JavaScript execution**: CSP-resistant execution via Chrome Debugger API bridge, supporting both isolated and main worlds.
- **Downloads**: trigger downloads, list active downloads with state filtering, cancel, open completed.
- **Cookies**: get, set, delete with full attribute support (domain, path, secure, httpOnly, sameSite, expiration).
- **Cache management**: clear browsing data (cache, cacheStorage, indexedDB, localStorage, service workers) with optional origin filtering.
- **Dialog handling**: accept or dismiss JavaScript alerts/confirms/prompts with optional prompt text.
- **Screenshot materialization**: screenshots auto-saved to disk and paths injected into results for downstream `image_understanding`.

### Engine

- **Multimodal media understanding**: new `multimodal.rs` module (894 lines) handles media resource extraction, model dispatch by label, and content injection into the system prompt.
- **OpenAI strict mode compliance**: all tool schemas pass OpenAI strict validation — `additionalProperties: false`, optional fields use `["type","null"]`, no unsupported keywords. Added schema-validation tests.
- **Codex OAuth token auto-loading**: reads `~/.codex/auth.json` and injects `access_token` when `api_base` points to Codex backend.
- **Datetime context injection**: RFC 3339 `Current datetime` field in implicit context for temporal awareness.
- **ContentPart migration**: steering and follow-up messages use `Vec<ContentPart>` instead of string concatenation, enabling multimodal content passthrough.
- **Hippocampus renamed to Brain**: all internal modules, file paths, i18n, and documentation updated to align with the `anda-brain` standalone crate.
- **Conversations API**: status tracking, agent-aware formatting, and enhanced metadata.
- **Cron metadata refactored**: `cron_job_id` replaces full job content string, reducing payload size.
- **System extra content injection**: structured `ContentPart` with `[$system: ...]` prefix replaces ad-hoc string formatting.
- **Compaction threshold simplified**: 80% of context window with 100K token minimum.
- **Config context windows**: reduced from 1,000,000 to 400,000 to match actual provider limits.

### Auto-Update System

- **Daemon**: `AutoUpdater` checks GitHub releases API, downloads platform-specific assets, verifies SHA256 checksums, installs binaries, and restarts.
- **REST endpoints**: `/auto_update`, `/auto_update/check`, `/auto_update/install_and_restart` with bearer-token auth.
- **WebSocket RPC**: `auto_update_status`, `auto_update_check`, `auto_update_install_and_restart` for the extension.
- **TUI integration**: async background check on chat init, notice banner in status area.
- **Extension UI**: amber notification banner with version tag and install button.
- **State machine**: persistent update state in `AndaDB` across daemon restarts.

### CLI & TUI

- **`/new` command**: `/new [prompt]` starts a fresh conversation across CLI, TUI, and extension. Stale conversation detection prefixes output with `[Previous conversation #N]`.
- **TUI scrollback purge**: `ClearType::Purge` clears terminal scrollback on `/new`.
- **Website pages**: privacy, terms, and support pages added.

### Changed

- **Browser tool split**: monolithic `chrome_browser` replaced by four focused tools with minimal schemas and independent timeout handling.
- **Chrome extension modularized**: `client.ts` (2,099 lines) split into channel, side-panel, conversations, polling, voice, types, commands, and chrome modules. `service_worker.ts` (1,884 lines) split into browser-actions, speech, voice, audio, TTS, settings, and types.
- **Manifest permissions**: `browsingData`, `cookies`, `downloads`, `webNavigation` added.
- **Dependencies**: `anda_brain` 0.6.0→0.6.1, `anda_core` 0.12.2→0.12.3, `anda_engine` 0.12.16→0.12.19, `weixin-agent` 0.1.0→0.2.0 (git→crates.io).

### Stats

- 198 files changed, +25,377 / -1,913 lines
- 32 commits across 8 patch releases (0.7.1 → 0.7.8)
- Chrome extension: ~5,000+ lines of Svelte/TypeScript
- Browser tools: ~3,000+ lines across Rust backend and TypeScript service worker

## [0.7.8] — 2026-05-22

### Added

- **Browser downloads**: `download`, `list_downloads`, `cancel_download`, and `open_download` actions added to `chrome_tabs`, enabling the agent to download files, list active downloads with state filtering, cancel in-progress downloads, and open completed downloads through Chrome. Types: `ChromeDownloadItem` with fields for id, url, filename, state, bytes, and timestamps.
- **Browser cookies**: `get_cookies`, `set_cookie`, and `delete_cookie` actions for full cookie management through the Chrome extension. Supports domain, path, secure, httpOnly, sameSite, expirationDate, and storeId fields. Types: `ChromeCookieInfo` and `ChromeCookieSameSite`.
- **Browser cache clearing**: `clear_browser_cache` action via `chrome.tabs` — accepts optional `since_ms` Unix timestamp and `origins` array to selectively clear cache, cacheStorage, indexedDB, localStorage, and service workers through `chrome.browsingData.remove`.
- **Page PDF printing**: `print_to_pdf` action (`chrome_page`) generates PDFs from the active tab via CDP `Page.printToPDF`. Data URL handling extended to accept `application/pdf` MIME type with `.pdf` file extension.
- **Accessibility tree**: `get_accessibility_tree` action (`chrome_page`) returns the page's accessibility tree via CDP `Accessibility.getFullAXTree`, configurable node limit (default 500).
- **Viewport annotations**: `annotate_viewport` and `clear_annotations` actions (`chrome_page`) for visual element highlighting in screenshots via CDP `Overlay` domain.
- **File upload**: `upload_file` action (`chrome_input`) uploads local files through file input elements via CDP `DOM.setFileInputFiles`. Validates non-empty file paths.
- **Dialog handling**: `handle_dialog` action (`chrome_page`) accepts or dismisses JavaScript dialogs (alert/confirm/prompt) via CDP `Page.handleJavaScriptDialog`, with optional `prompt_text` for prompt dialogs.
- **Scroll-to coordinates**: `scroll_to` now accepts viewport x/y coordinates in addition to CSS selectors, matching the `scroll_to` behavior when targeting a specific screen position.
- **TypeText without selector**: `type_text` now works without a selector — when omitted, types into the currently focused/active element, enabling keyboard-focused workflows.
- **Full-page screenshot**: `screenshot` action now supports `full_page` parameter to capture the entire scrollable page instead of just the viewport.
- **New extension permissions**: `browsingData`, `cookies`, `downloads`, and `webNavigation` added to manifest for the new capabilities.
- **Navigation-ready loading**: `goBack`, `goForward`, and `reload` now wait for the page to finish loading before returning, matching the behavior of `navigate`.
- **WebNavigation and debugger events**: `chrome.webNavigation.onCommitted`/`onCompleted` and `chrome.debugger.onEvent` typed in `ChromeApi` for reliable page-ready detection after navigation.

### Changed

- **Cargo.toml version**: bumped from 0.7.7 to 0.7.8.
- **Tool descriptions updated**: `chrome_tabs` description now mentions downloads, cookies, and cache; `chrome_page` mentions accessibility tree, PDF, annotations; `chrome_input` mentions upload_file.
- **`select_dropdown` extended**: description now notes it can target same-origin iframes containing selects, matching the improved iframe-handling in the service worker.
- **browser-actions.ts (+2002 lines)**: service worker refactored with page-ready waiting (`waitForPageReady`), CDP-based actions (printToPdf, accessibility tree, annotations, dialog handling, upload file), downloads/cookies/cache management, and ArrowCaster mappings for all new BrowserAction values.

## [0.7.7] — 2026-05-22

### Added

- **Runtime model switching in Chrome extension**: the Chat Settings panel now shows a model selector dropdown with available models fetched from the daemon, plus a refresh button. Switching the active model happens live without restarting the daemon — backed by new `model_names` and `set_model` WebSocket RPC methods in `browser_ws.rs`.
- **Model state TypeScript types**: added `ModelState` and `DaemonModelState` interfaces to the extension client, with `normalizeModelState` deduplication.
- **i18n strings for model UI**: `activeModel`, `refreshModels`, `modelListEmpty`, and `modelUpdated` added to all 6 supported locales.

### Fixed

- **WebSocket poll transient-error handling**: `isTransientWebSocketError` now correctly returns `true` for poll failures (previously `false`), so polling retries rather than bailing on transient disconnects. The detection pattern was also broadened to catch more transient WS error messages (timeout, disconnected, etc.).

### Changed

- **Model state refreshed on connection lifecycle**: the extension fetches model state after init, settings save, and connection test — clearing model state when the token is removed.

- **Auto-update system**: full self-update mechanism spanning daemon, TUI, Chrome extension, and gateway. The `AutoUpdater` checks the GitHub releases API for new versions, downloads the correct platform asset, verifies SHA256 checksums, and can install the new binary and restart the daemon — all through a persistent state machine stored in `AndaDB`. Backend exposes three REST endpoints (`/auto_update`, `/auto_update/check`, `/auto_update/install_and_restart`) with bearer-token auth, plus equivalent WebSocket RPC methods (`auto_update_status`, `auto_update_check`, `auto_update_install_and_restart`) for the browser extension.
- **Auto-update CLI updater refactor**: `ReleaseTarget`, `UpdateFinish`, and constants promoted to `pub(crate)` so the auto-updater reuses the same platform detection, asset naming, and download logic as the manual `anda update` CLI command.
- **Auto-update Chrome extension UI**: an amber notification banner appears in the side panel when an update is downloaded, showing the latest version tag and an "install & restart" button with a confirmation dialog. Update state is refreshed on init and after settings save — cleared when the token is removed.
- **Auto-update TUI integration**: the TUI fires an async auto-update check on chat init and displays a notice banner in the status area when an update is available, using `oneshot` channels for non-blocking background checks.
- **Daemon DB helpers**: `bot_db_config()`, `connect_bot_db()`, and `open_bot_db()` extracted as public methods on `Daemon`, enabling the auto-updater and future subsystems to share the same bot database.
- **Extension client `AutoUpdateState` types**: TypeScript interfaces for `AutoUpdateStatus` and `AutoUpdateState` with all fields (status, current_tag, latest_tag, SHA256, checksum_verified, etc.).
- **i18n strings for update UI**: `updateReadyTitle`, `updateReadyBody`, `installRestartUpdate`, `updateRestartConfirm` added to all 6 supported locales.
- **Cli updater takes `&Daemon`**: `cli::updater::run()` now accepts `&Daemon` instead of `home_dir: &Path`, giving the updater access to daemon-level utilities for the shared update logic.


## [0.7.6] — 2026-05-21

### Changed

- **OpenAI strict mode compliance for all tool schemas**: every tool function now passes OpenAI strict validation — all properties listed in `required` (optional fields use `["type","null"]`), `additionalProperties: false` on all objects, no unsupported schema keywords. Added `json_schema.rs` test utility with `assert_openai_strict_parameters` and schema-validation tests covering brain client, cron tools, browser tools, conversation API, goal tool, multimodal agents, transcription, and TTS.
- **Dependency bumps**: `anda_brain` 0.6.0→0.6.1, `anda_core` 0.12.2→0.12.3, `anda_engine` 0.12.16→0.12.19.
- **Config context windows reduced**: memory-capable model context windows lowered from 1,000,000 to 400,000 to align with actual provider limits.
- **Model rename**: `gpt-5.4-mini` → `gpt-5.4`.
- **Codex token loading moved into `Config::from_file`**: previously handled in `main.rs`, now applied consistently at config load time so all callers benefit.
- **Compaction threshold simplified**: switched from half-window-with-clamps to 80% of context_window with 100K token minimum, providing more predictable compaction behavior.
- **Removed `max_output_tokens` overrides** from agent completion configs — now relying on provider/model defaults.
- **Removed `minLength: 1`** from TTS text parameter schema (not allowed under OpenAI strict mode).

## [0.7.5] — 2026-05-21

### Changed

- **Hippocampus renamed to Brain**: all internal module names, file paths, docsite pages, i18n translations, and documentation references renamed from `hippocampus` to `brain` — aligning with the standalone `anda-brain` rename.


## [0.7.4] — 2026-05-21

### Changed

- **Current datetime injected into engine context**: the `extra_user_context` now includes an RFC 3339-formatted `Current datetime` field at the top of the implicit context, giving agents accurate temporal awareness without relying on training-cutoff heuristics.
- **Cron job metadata refactored**: `cron_job` (full job content as a string) replaced with `cron_job_id` (u64) in request metadata, reducing serialized payload size. The job name now falls back to the numeric ID when `cron_job_name` is empty. Cron job content is no longer echoed in completion messages.
- **Stop/New command handling reordered**: `PromptCommand::Stop` now breaks the prompt loop immediately without processing any remaining commands. `PromptCommand::New` is logged as unexpected (it should be handled at the agent level in `run()`, not in the session runner) and no longer injects its prompt as follow-up content.
- **Session Working status auto-repair**: when a conversation status is not `Working` but the runner has pending tasks (`!is_idle()`), the status is now persisted as `Working` with `failed_reason` cleared — recovering from stale status states without manual intervention.
- **Sidebar channel toggle via Button**: the collapsed/expanded chevron icon in the channel sidebar is now wrapped in a shadcn-svelte `Button` with `aria-label` and `title`, replacing an unlabeled clickable div.


### Added

- **Codex OAuth token auto-loading**: when a model provider is configured with `api_base: "https://chatgpt.com/backend-api/codex"`, the daemon now automatically reads `~/.codex/auth.json` and injects the `access_token` as `api_key`. This enables seamless Codex backend usage without hardcoding tokens in config — just log in once and the daemon picks up the token on restart.
### Fixed

- **Duplicate `scrollIntoView` calls**: the App.svelte `$effect` now tracks `prevLastMessageId` to only trigger `scrollIntoView` when the last message ID actually changes, preventing redundant scroll animations on unrelated reactivity triggers.

## [0.7.3] — 2026-05-20

### Added

- **Multimodal media understanding**: the engine now supports image, video, and audio understanding via model-label routing. A new `multimodal.rs` module (894 lines) handles media resource extraction, model dispatch, and content injection into the system prompt. Models with `image_understanding`, `video_understanding`, or `audio_understanding` labels are automatically selected for the corresponding media types.
- **Screenshot materialization**: browser screenshots taken via the Chrome extension now have their `data_url` automatically decoded and saved to disk under `browser-screenshots/`. The saved file path is injected back into the action result, making screenshots immediately consumable by downstream tools like `image_understanding` without a separate download step.
- **CSP-resistant JavaScript execution**: introduced `chrome_script` with a Chrome Debugger API bridge (`debugger` world) that bypasses Content Security Policy restrictions, replacing the previous `chrome.scripting.executeScript` approach that failed on CSP-strict pages (e.g., GitHub, X).
- **Browser tool split**: the monolithic `chrome_browser` tool was split into four focused, single-responsibility tools — `chrome_tabs` (navigation/tab management), `chrome_page` (inspection/screenshot/extraction), `chrome_input` (click/type/scroll/interaction), and `chrome_script` (JavaScript execution). Each tool has a minimal schema, clearer error messages, and independent timeout handling. The legacy tool and `ChromeBrowserToolKind::Legacy` variant have been fully removed.
- **Alert dialog component**: a full `AlertDialog` shadcn-svelte component family (overlay, content, header, title, description, footer, cancel, action) added to the Chrome extension UI library, enabling confirmation dialogs for destructive actions.
- **Channel deletion with confirmation**: users can now delete channels via the UI with an alert dialog confirmation step, preventing accidental data loss.
- **Side tasks panel**: the Chrome extension sidebar was restructured into a `SidePanel` component with a tab-based layout separating chat channels from side tasks, improving navigation and extensibility.
- **I18n audit tool**: a new `scripts/check-i18n.mjs` (558 lines) validates i18n coverage across all 6 supported locales, detecting missing keys, untranslated messages, and stale entries.
- **Vitest infrastructure**: Chrome extension client logic now has a proper test framework with `vitest.config.ts`, initial test suites for `poll-conversation`, `side-panel`, and `voice` modules, and extracted pure functions for testability.
- **WeChat login status handling**: three new `LoginStatus` variants are now handled — `NeedVerifyCode` (pair-code verification required on phone), `VerifyCodeBlocked` (too many wrong verification codes), and `BindedRedirect` (bot already bound to this instance, no new credentials issued).

### Changed

- **weixin-agent promoted to crates.io**: `weixin-agent` 0.1.0 → 0.2.0, moved from a git dependency (`[patch.crates-io]`) to the public crates.io registry. The `[patch]` entry has been removed from the workspace `Cargo.toml`.
- **Agent system instructions**: `render_system_instructions()` now uses named format parameters (`{ins}`, `{knowledge}`, `{notes}`, etc.) instead of positional `{}` placeholders, improving readability and reducing argument ordering bugs.
- **Chrome extension decomposed**: `client.ts` (2,099 lines) split into focused modules — `channel.svelte.ts` (561 lines), `side-panel.svelte.ts` (653 lines), `conversations.ts`, `poll-conversation.ts`, `voice.ts`, `types.ts`, `commands.ts`, `chrome.ts`. `service_worker.ts` (1,884 lines) similarly decomposed into `browser-actions.ts` (1,046 lines), `page-speech.ts`, `page-voice.ts`, `page-audio.ts`, `tts.ts`, `settings.ts`, and `types.ts`.
- **Legacy `chrome_browser` tool removed**: the original monolithic browser tool has been fully removed from both `browser.rs` and `engine.rs`, leaving only the four split tools (`chrome_tabs`, `chrome_page`, `chrome_input`, `chrome_script`).


### Added

- **Codex OAuth token auto-loading**: when a model provider is configured with `api_base: "https://chatgpt.com/backend-api/codex"`, the daemon now automatically reads `~/.codex/auth.json` and injects the `access_token` as `api_key`. This enables seamless Codex backend usage without hardcoding tokens in config — just log in once and the daemon picks up the token on restart.
### Fixed

- **CSP bypass result extraction**: the Debugger bridge's `Runtime.evaluate` results were not properly unwrapped when the evaluated expression returned an object handle rather than a value, causing `chrome_script` output to be empty on certain pages. Now correctly inspects and extracts object properties via the RemoteObject protocol.
- **Debugger concurrency**: multiple overlapping `chrome_script` calls could collide on the single DevTools debugger session. Fixed by serializing bridge calls through a per-tab mutex.
- **Agent session/ancestor handling**: `spawn_session_runner` now correctly owns the media understanding step (previously duplicated at 3 call sites), and the session runner content filter excludes stale inline data from follow-up messages.
- **Windows PowerShell key**: corrected a `ctrl+shift+p` → `ctrl+shift+P` casing issue that prevented the command palette shortcut from working on Windows.

### Dependencies

- `crypto-common` 0.2.1 → 0.2.2
- `typetag` 0.2.21 → 0.2.22
- `weixin-agent` 0.1.0 → 0.2.0 (git → crates.io)

## [0.7.2] — 2026-05-17

### Added

- **Submit key mode setting**: users can choose between two input modes in Chrome extension settings: "Enter sends" (Shift+Enter for newlines, the new default) or "Ctrl/Cmd+Enter sends" (Enter for newlines). Configurable via a radio button group in the settings panel. Persisted to `chrome.storage.local` alongside `baseUrl` and `token`.
- **Local message reconciliation**: when a locally drafted message is confirmed by the server, `reconcileLocalMessages()` merges the server-side message with the local draft instead of appending a duplicate. This prevents ghost drafts from appearing after round-trips.
- **WebSocket transient error recovery**: `isTransientWebSocketError()` detects `WebSocket connection closed/timed out/not connected` errors and transitions the UI status to `reconnecting` instead of surfacing an error to the user.
- **Locale strings for enter key behavior**: new i18n messages (`enterKeyBehavior`, `enterSendsMessage`, `shiftEnterNewLine`, `modifierEnterSendsMessage`, `enterNewLineModifierSends`, `sendWithEnter`) added for all 6 supported locales (ar, en, es, fr, ru, zh_CN).
- **Token input upgraded**: Bearer token field in settings changed from `Textarea` (4 rows) to single-line `Input type="text"` for cleaner layout.

### Changed

- **Polling interval increased**: `pollingIntervalMs` raised from 2000ms to 3000ms to reduce unnecessary server load.
- **Submit tooltip adapts to mode**: send button tooltip now shows "Enter to send. Shift + Enter for a new line." in `enter` mode, and "Command/Control + Enter to send. Enter for a new line." in `modifier-enter` mode.
- **Manifest permissions simplified**: `"host_permissions": ["*://*/*"]` + `"optional_host_permissions": ["file:///*"]` replaced with `"host_permissions": ["<all_urls>"]`, equivalent behavior.
- **`updateConversationMessages` guard**: now passes through even when `incoming` is empty if `updatedAt` is set, ensuring timestamps update even without new messages.

## [0.7.1] — 2026-05-16

### Added

- **`/new` command**: `/new [prompt]` (alias `/clear`) starts a fresh conversation, completing the current one if unfinished. Works across CLI, TUI, channel runtime, and Chrome extension. Trusted channel users can use `/new` in IM; `$external_user` attempts are ignored.
- **Stale conversation detection in channel output**: when agent output arrives for a route whose current conversation has moved on (e.g., after `/new`), the channel message is prefixed with `[Previous conversation #N]` so recipients know the context has shifted.
- **TUI scrollback purge on `/new`**: `ClearType::Purge` clears the terminal scrollback buffer when starting a new conversation, giving a truly fresh visual experience. Added `clear_message_view()` and `pending_scrollback_purge` state.
- **Chrome extension `/new` integration**: command palette entry for `/new`, `parseNewPromptCommand()` in `client.ts`, and `clearConversationDisplay()` for full display reset on new conversation.
- **System extra content injection**: `system_extra_content()` serializes request context metadata (`ctx.extra`) as structured `ContentPart` prefixed with `[$system: ...]`, replacing ad-hoc string formatting. Applied at both initial prompt and follow-up input boundaries.
- **`session_creation_lock`**: mutex serializes session creation to prevent races between concurrent requests for the same source.
- **`get_session_by_source()`**: finds an active session by source key, enabling session reuse across conversation boundaries (e.g., after `/new` without a prompt).
- **`finish_when_idle` flag**: when `/new` detaches a running session, `finish_when_idle=true` tells the session runner to complete and exit gracefully once idle — no abrupt kill.

### Changed

- **ContentPart migration**: steering and follow-up messages now use `Vec<ContentPart>` internally instead of string concatenation (`format!("{}\n\n{}")`). Resource attachments are converted to `ContentPart` directly via `follow_up_content()`. This aligns with `anda_engine` 0.12.9's upgraded steering API and enables multimodal content passthrough.
- **Background task hooks carry context**: `on_background_progress` and `on_background_end` now pass `ctx.meta().extra` through `ConversationInput`, so background task outputs correctly include the originating request context.
- **Session detach returns value**: `detach_session()` returns `Option<Arc<Session>>` instead of just removing, enabling the caller to set `finish_when_idle` before the session exits.
- **`clear_route_conversation()`**: new channel runtime method removes current route→conversation binding while preserving the old conv→route mapping for stale output detection.
- **`force_standalone_conversation` flag**: new conversations started via `/new` skip history chaining and ancestor linking, keeping the fresh session truly independent.
- **Status bar updated**: TUI help text now includes `/new [message]` as the first slash command.

### Dependencies

- `anda_core` 0.12.0 → 0.12.1
- `anda_engine` 0.12.7 → 0.12.9
- `anda_hippocampus` 0.5.2 → 0.5.3
- `ic-agent` / `ic-transport-types` 0.47.2 → 0.47.3

## [0.7.0] — 2026-05-13

### Added

- **Streaming TTS playback with chunked synthesis**: long assistant responses are split into ~800-character chunks and synthesized/played back in a pipeline (play current chunk while synthesizing the next), replacing the previous wait-for-full-synthesis approach. This dramatically reduces response latency in voice conversations.
- **Markdown-to-speech text normalization**: `prepare_voice_tts_text()` strips markdown formatting prefixes (`>`, `-`, `#`, `*`, `|`), collapses whitespace, filters TTS-unsafe characters (emoji, control chars), and normalizes punctuation for natural-sounding speech synthesis.
- **Stepfun STT provider**: new `transcription/stepfun.rs` module with SSE-based streaming ASR, supporting hotwords, ITN, PCM codec/rate/bits/channel configuration, and optional prompts.
- **Stepfun TTS provider**: new `tts/stepfun.rs` module supporting `stepaudio-2.5-tts` with configurable voice, speed, volume, sample rate, optional pronunciation maps, and markdown filtering.
- **Per-provider audio format**: each TTS provider now declares its native audio format via `audio_format()` (e.g., `stepfun` returns `pcm`, others `mp3`). `audio_artifact_for_provider()` builds artifacts with the correct MIME type for each provider.
- **Voice status spinner**: `VoiceStatusSpinner` renders rotating progress indicators during long operations (sending, waiting, synthesizing, playing) for clear feedback in voice mode.
- **Graceful Ctrl-C in voice mode**: all async await points in the voice loop are wrapped through `wait_with_voice_status()`, enabling clean interruption during any phase (send, poll, synthesize, play).
- **Daemon startup diagnostics**: background daemons now capture stdout/stderr to a log file. On startup failure, the last 64KB of the daemon log is tailed and parsed for structured JSON log entries or plain error lines, surfaced in the error message.
- **`try_wait()` on `BackgroundDaemon`**: exposes the child process for early exit detection during startup wait loops.
- **Cron job metadata in channel output**: cron-triggered agent completions now display the job name, kind (shell/agent), and job content in the IM channel message, so recipients know which scheduled job produced the output.
- **`fmt::Display` for `JobKind`**: human-readable `"shell"` / `"agent"` labels.

### Changed

- **Voice response text includes artifacts**: `VoiceConversationCursor` now tracks `seen_artifacts`, and `assistant_text_from_messages()` includes artifact descriptions in the synthesized text when present (e.g., "I created a file for you").
- **Error handling**: `main()` wrapped in `run()` for proper `log::error` on unhandled failures instead of default Rust panic output.
- **TTS config**: `default_voice` field removed; per-provider voices are configured directly under each provider section.

### Refactored

- **TTS providers modularized**: monolithic `tts.rs` split into per-provider modules: `tts/edge.rs`, `tts/google.rs`, `tts/openai.rs`, `tts/stepfun.rs`. Provider-specific config types moved to `config/tts.rs`.
- **Transcription providers modularized**: monolithic `transcription.rs` split into per-provider modules: `transcription/google.rs`, `transcription/groq.rs`, `transcription/local_whisper.rs`, `transcription/openai.rs`, `transcription/stepfun.rs`. Provider-specific config types moved to `config/transcription.rs`.

## [0.6.5] — 2026-05-12

### Added

- **Startup self-check & conversation recovery**: `AndaBot::init()` now triggers a startup self-check (5s after launch) that scans all source-bound conversations and auto-resumes any in `Submitted`/`Working` state from the last saved history. Interrupted conversations survive process restarts: the agent reconstructs system instructions, chat history, request metadata, and tool selection, then sends a recovery prompt and continues the session. Recovery is scoped to currently active IM channels.
- **`log_level` config**: new `log_level` field in `config.yaml` (default `warn`) controls structured log verbosity. `logger::init_daily_json_logger` now accepts an explicit level parameter instead of reading from env.
- **Cron agent job prompt enrichment**: cron-triggered agent jobs now receive a system runtime prompt including `_id`, `name`, and `instructions` metadata, helping the agent distinguish scheduled work from user-initiated requests.
- **Conversation child chaining**: new conversations are now linked as the `child` of the previous conversation in the chain. `latest_conversation_in_chain()` traverses the child chain (up to 256 hops, with cycle detection) to find the latest conversation for a given source.
- **`conversation_chat_history()`**: extracts and marks messages from stored conversation JSON, stripping dangling `tool_calls` without corresponding `tool_result` blocks.
- **`GetConversation` with `_id: 0`**: resolves to the latest conversation document via `latest_document_id()`.
- **`external_user_name()` helper**: formats `$external_user:<name>` participant names consistently.
- **Tests**: `startup_status_policy_resumes_only_running_states`, `request_meta_from_conversation_recovers_route_from_source_key`, `conversation_chat_history_marks_startup_runtime_messages`, `format_local_date_returns_datetime_with_timezone`.

### Changed

- **System instructions use local datetime format**: `format_local_date()` produces `"YYYY-MM-DD HH(AM/PM) ±TZ"` via chrono `clock` feature, replacing the RFC 3339 `rfc3339_datetime()` format across all system instruction rendering.
- **Conversation continuation semantics**: existing conversations in `Submitted`/`Working`/`Idle` state can now be continued with an empty prompt (the session enters wait mode). Previously all prompts required non-empty content.
- **`user_info()` parameter type**: `Principal` → `String` for broader compatibility (aligned with anda_hippocampus v0.5.2 changes).
- **`mark_special_user_messages` unified**: `mark_system_runtime_messages` and `mark_external_user_messages` merged into a single function. External user messages with existing names now preserve them via `external_user_name()`.
- **Session field renamed**: `source` → `source_key` for clarity.
- **`source_state` updated on compaction**: compaction now updates the source→conversation mapping to the new conversation id.
- **Runtime prompt wording**: `"not from the external user"` → `"not from the user"`.
- **Code extraction**: `persist_conversation_state()`, `spawn_session_runner()`, `available_tool_names()` extracted from inline logic to reusable methods for startup recovery code paths.
- **`chrono`**: `clock` feature enabled for local timezone support.

### Dependencies

- `anda_hippocampus` → 0.5.2 (user init routing, local_date_hour, prompt improvements)

## [0.6.4] — 2026-05-11


### Added

- **Codex OAuth token auto-loading**: when a model provider is configured with `api_base: "https://chatgpt.com/backend-api/codex"`, the daemon now automatically reads `~/.codex/auth.json` and injects the `access_token` as `api_key`. This enables seamless Codex backend usage without hardcoding tokens in config — just log in once and the daemon picks up the token on restart.
### Fixed

- **IME composition jitter in TUI input** ([#1](https://github.com/ldclabs/anda-bot/issues/1)): the main render loop now only redraws the terminal when observable state has actually changed, using `ChatRenderSnapshot` / `StatusRenderSnapshot` comparison plus a `needs_render` flag. Previously the terminal was redrawn on every loop iteration (~6–7 fps), causing IME composition candidate windows to flicker and shift on systems like Fedora 42. Render-on-demand triggers include: chat message changes, new streaming tokens, terminal resize, key input, paste, thinking state transitions, and daemon status changes.

### Added

- **↑/↓ cursor navigation in multi-line input**: `Up` and `Down` arrow keys now move the cursor vertically through multi-line input, tracking a preferred visual column (`input_preferred_col`) so repeated ↑/↓ stays on the same column. Built on `move_cursor_vertically()`, `input_cursor_points()`, and `input_cursor_for_visual_position()`.
- **Input scrollbar**: when input text exceeds the available area, a vertical scrollbar (`┃` thumb on `│` track) appears at the right edge. The viewport auto-scrolls to keep the cursor visible, using `InputViewport` + `input_scroll_top()`.
- **Ctrl+J as alternative newline**: for terminals that do not distinguish `Shift+Enter`, `Ctrl+J` now also inserts a newline (`input_newline_key()`).
- **Input viewport abstraction**: new `InputViewport` struct and `build_input_viewport()` encapsulate line rendering, scroll position, cursor placement, and virtual continuation lines when the cursor wraps to a new row after the last actual character.
- **7 new tests**: `input_newline_key_accepts_shift_enter_and_ctrl_j`, `move_cursor_vertically_preserves_visual_column`, `move_cursor_vertically_handles_wrapped_lines`, `input_viewport_follows_cursor_to_bottom_of_long_paste`, `input_viewport_keeps_cursor_line_visible_when_moved_up`, `input_viewport_adds_virtual_line_when_cursor_wraps_past_full_row`, `input_scroll_top_tracks_cursor_without_exceeding_content`.

### Changed

- **Keyboard shortcut help updated**: status bar now reads `"Enter send  •  Shift+Enter/Ctrl+J newline  •  ↑/↓ move lines  •  Ctrl+U clear  •  Ctrl+C quit"`. README.md, README_cn.md, and anda_bot/README.md all reflect the new shortcuts.
- **`handle_key` now receives `input_content_width`**: needed for vertical cursor movement calculations, which depend on the actual visible content width.
- **`wrapped_cursor_position` split**: new `wrapped_cursor_position_usize` internal variant returns `(u16, usize)` for scroll-aware cursor row tracking.

## [0.6.3] — 2026-05-11

### Added

- **Markdown rendering in TUI chat messages**: assistant `ContentPart::Text` messages are now rendered through a GFM-compliant markdown pipeline (`tui/markdown.rs`, ~450 lines) using the `markdown` crate. Supported elements receive distinct ratatui styles: 4 heading levels (panda white / bamboo light / bamboo green / leaf mint, all bold), **bold** (bold modifier), *emphasis* (italic), ~~strikethrough~~ (dim + crossed-out), `inline code` and code blocks (accent teal on footer background), [links](url) (underlined teal), > blockquotes (dim italic), lists (bamboo light), and tables with left/center/right alignment support plus styled header/separator/body rows. Grayscale fallback: when markdown parse fails, text is rendered as plain text instead of erroring.
- **Grapheme-aware line wrapping for styled spans**: the new `wrap_styled_body_line` function wraps styled `Line<'static>` values grapheme-by-grapheme, preserving individual span styles across line boundaries. Control characters and zero-width graphemes are filtered during wrap. Consecutive same-style spans are merged into single spans where possible.
- **Table rendering in TUI**: GFM tables are parsed from the source text, column-widths computed via Unicode display width, and cells aligned according to the source alignment hints (`:---`, `:---:`, `---:`). Separator rows are rendered between header and body. Alignment defaults to `---` (3 dashes, right-padded).
- **New tests**: `chat_message_lines_render_markdown_source_styles` (verifies heading bold + color, inline bold + inline code styles), `chat_message_lines_render_markdown_tables` (verifies aligned markdown source output for tables).
- **Background shell intermediate output streaming**: the `Session` tool hook now implements `on_background_progress`, forwarding intermediate background task output to the agent as `$system` runtime prompts (not just final completion). This lets the agent see long-running task progress in real time.

### Changed

- **Shell runtime runs in insecure mode**: `NativeRuntime::new(workspace).insecure()` allows shell commands wider system access when needed.
- **anda_engine upgraded to 0.12.4**.
- **`push_wrapped_block` renamed → `push_markdown_block`**: now delegates to `markdown::render()` instead of doing simple `text.lines()` + `wrap_visual()`. The old plain-text wrapping logic is replaced by styled span wrapping.
- **Background shell prompt label unified**: `"background shell task"` → `"background shell"` across both `on_background_end` and the new `on_background_progress` hooks.

## [0.6.2] — 2026-05-10

### Changed

- **Cron jobs run as original caller**: `CronJobOrigin` now captures the `caller` Principal at creation time via `from_meta_with_caller()`. When a cron job executes (shell or agent), it impersonates the original creator rather than the system controller. `notify_shell_result` also uses the caller for agent notification. This ensures scheduled jobs remember who created them and operate with the correct identity.
- **SessionRequestMeta**: new `SessionRequestMeta` struct (stored as session state via `ctx.base.set_state`) persists the original request metadata across the session lifetime. When an agent reconnects to an existing session, `request_meta` is updated with the current conversation ID. `CreateCronTool` reads `SessionRequestMeta` from context state to resolve the caller identity for cron job origin capture.

## [0.6.1] — 2026-05-10

### Added

- **Goal as a first-class agent-callable tool**: the `goal` tool is now directly callable by the agent (not just via `/goal` slash command). When the agent encounters a complex multi-turn request, it can autonomously start or update goal mode by calling `goal` with a concrete objective and verification criteria, then continue working normally. The session stays alive as long as a goal is active. `GoalTool` shares the session's goal state and `active_at` timestamp via `Arc`, and `SelfInstructions.md` includes explicit guidance on when the agent should (and should not) use the tool.

## [0.6.0] — 2026-05-09

### Added

- **External user support with trust boundaries**: new `allow_external_users` config field for all 5 channel types (Discord, Telegram, IRC, Lark, WeChat). When enabled, messages from non-allowlisted senders are tagged as `external_user: true` and wrapped with `[$external_user: channel="...", sender="..."]` prefix, allowing the agent to distinguish untrusted guests from the owner/partner. A comprehensive Trust Boundaries section in `SelfInstructions.md` governs how the agent handles external user data.
- **Cron job origin context**: new `CronJobOrigin` struct captures the full request context (user, source, reply_target, thread, workspace, conversation_id, external_user) when a cron job is created. Origin is persisted in the job record (schema v2) and round-tripped back into `RequestMeta` on each execution, so scheduled jobs "remember" which channel and conversation they came from.
- **Shell cron result notification**: when a scheduled shell job completes, the result (stdout or error) is fed back to the agent via `system_runtime_prompt("cron shell job result")`, enabling the agent to incorporate the outcome and notify the originating user in-channel.
- **Channel route recovery from RequestMeta**: `on_completion` hook now falls back to `route_from_meta()` when `route_for_conversation()` misses, reconstructing the channel route from persisted `RequestMeta` extras. New bindings are persisted for future lookups.

### Changed

- **System prompt format upgrade**: `[$system runtime message: ...]` → `[$system: kind="..."]` (structured key-value format) across compaction, goal continuation, subagent progress/final output, and background shell task notifications. New `mark_special_user_messages()` unifies backfilling for both `$system` and `$external_user` names.
- **Formation attribution for external users**: memory formation now uses `$external_user:<sender>` as counterparty instead of the caller when `external_user` is set, keeping guest memories isolated from the trusted user's profile.
- **Channel message schema v2**: `ChannelMessage` gains `external_user: Option<bool>` field, with all channel implementations updated to populate it and tests added for the new behavior.
- **CronRuntime::connect** simplified: controller Principal now derived from `Principal::management_canister()` instead of requiring an explicit `engine_id` parameter.
- **Documentation**: `allow_external_users` documented in README.md, README_cn.md, and anda_bot/README.md; config.yaml updated with commented examples; config tests assert the new field.

## [0.5.4] — 2026-05-09

### Added

- **Multi-workspace support**: `workspace_dir: PathBuf` → `workspaces: Vec<PathBuf>` in `EngineConfig`, covering workspace, sandbox, channels, and skills directories. File tools (`ReadFile`, `SearchFile`, `EditFile`, `WriteFile`) resolve paths across all configured workspaces via `with_workspaces()`.
- **$system runtime message protocol**: new `engine/system` module introduces structured runtime messages with a `[$system runtime message: <kind>]` prefix and disclaimer, allowing the model to distinguish system/operational prompts from external user intent. Wraps compaction handoffs, goal continuation prompts, subagent progress/final output, and background shell task notifications. A `mark_system_runtime_messages()` function backfills the `$system` name on persisted user-role messages that contain these prompts, ensuring correct attribution across sessions.

### Changed

- **SelfInstructions rewrite**: system prompt restructured to be more concise and persona-driven. New sections: `Participants` (explicitly naming `$self`, `$system`, and the external user), `Personality And Relationship`, and `Communication`. Memory/Growth and Working Philosophy sections simplified. Tone shifted from tutorial-style to a confident partner persona.
- **Quick dependency bumps**: `anda_cognitive_nexus` 0.7.18, `anda_engine` 0.12.2, `anda_hippocampus` 0.5.1, `hashbrown` 0.17.1, `quick-xml` 0.39.4, `tokio` 1.52.3.

## [0.5.3] — 2026-05-08

### Added

- **Session introspection API & CLI**: new `ListSessions` / `GetSession` tool calls on the engine allow agents (and external callers) to inspect active session state. CLI: `anda session list` and `anda session get <id>` with `--json` output.
- **Documentation site (docsite)**: Docusaurus-based documentation at docs.anda.bot covering quick start, memory (Hippocampus), runtime (channels, configuration), and workflows (long-horizon goals). Full i18n across 6 languages (EN, ZH, ES, FR, RU, AR).
- **SubAgentManager & SkillManager** added to base tools, enabling agents to spawn subagents and load skills without custom tool config.

### Changed

- **Workspace prompt hardened**: system prompt now labels the workspace as AUTHORITATIVE with an explicit warning not to trust workspace paths from past `user_history_conversations` — they may belong to different sessions.
- **Context continuity**: the current conversation is now included in `history_conversations`, giving the model access to the full thread (not just ancestors).
- **Dependencies**: `anda_engine` 0.12.0 → 0.12.1.

## [0.5.2] — 2026-05-07

### Changed

- **SubAgent module alignment**: imports migrated from `anda_engine::context` to `anda_engine::subagent` (the new top-level module in anda_engine v0.12).
- **Core function promotions**: `prompt_with_resources` and `text_resource_documents` moved from local helpers to `anda_core`.
- **Goal completion behavior**: instead of silently keeping the session active after goal completion, a supervisor evaluation message is now injected into the chat history.
- **Terminology**: `SessionJob` → `SessionRunner`, `task_id` → `session_id`, "Background task" → "Subagent session" throughout.
- **CLI**: `--skills-only` flag renamed to `--skills`.
- **Website rebrand**: landing page repositioned as "open-source Rust terminal agent with graph memory, subagents, and external tool integration"; all 6 locales updated; hero section redesigned.

### Added

- **Background subagent progress reporting**: new `on_background_progress` hook surfaces intermediate subagent output to the user in real-time via chat messages.
- **Side agent shell access**: `ShellTool` added to side agent's allowed tools for read-only filesystem inspection.


### Added

- **Codex OAuth token auto-loading**: when a model provider is configured with `api_base: "https://chatgpt.com/backend-api/codex"`, the daemon now automatically reads `~/.codex/auth.json` and injects the `access_token` as `api_key`. This enables seamless Codex backend usage without hardcoding tokens in config — just log in once and the daemon picks up the token on restart.
### Fixed

- **recall_memory discipline**: tool description updated to explicitly discourage calls for facts already present in the active conversation.

### Dependencies

- `anda_core`, `anda_engine`, `anda_engine_server`, `anda_web3_client`: 0.11 → 0.12
- `anda_hippocampus`: 0.4 → 0.5

## [0.5.1] — 2026-05-07

### Changed

- **Conversation state consolidation**: `source_conversation` and `tools_usage` state moved from `AndaBot` into `ConversationsTool`, reducing surface area in agent.rs and centralizing serialization logic.
- **Async non-blocking send**: synchronous `send` replaced with oneshot-channel `start_send` / `finish_pending_send` to decouple the UI event loop from network I/O.
- Added `awaiting_response` field to track the gap between request dispatch and response arrival.

## [0.5.0] — 2026-05-07

Initial tracked release.
