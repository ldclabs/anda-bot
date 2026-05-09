# Changelog

All notable changes to Anda Bot.

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
