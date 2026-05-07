# Changelog

All notable changes to Anda Bot.

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
