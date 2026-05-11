# Changelog

All notable changes to Anda Bot.

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
