# Changelog

All notable changes to Anda Bot.

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
