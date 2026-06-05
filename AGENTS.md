# AGENTS.md

Guidance for coding agents working in this repository. This file applies to the
entire tree unless a more specific `AGENTS.md` is added in a subdirectory.

## Project Overview

Anda Bot is a local-first Rust AI agent with long-term memory, subagents, a TUI,
voice support, browser integration, cron automation, and IM channel runtimes.
The main binary is `anda`, implemented by the Rust package `anda_bot`.

The repository also contains:

- `chrome-extension/`: Svelte + TypeScript Chrome Side Panel client.
- `website/`: SvelteKit marketing/support site.
- `docsite/`: Docusaurus documentation site with localized docs.
- `skills/`: runtime skill packages distributed with the project.
- `scripts/`: release/install helper scripts.

## Development Rules

- Keep changes scoped to the subsystem requested. Avoid broad refactors unless
  they are necessary to make the requested behavior correct.
- Prefer existing project patterns over new abstractions. This codebase already
  has clear boundaries for config, daemon startup, engine tools, channels,
  transcription, TTS, cron, and the TUI.
- Do not commit secrets, tokens, generated local state, or user data. Runtime
  state normally belongs under `~/.anda`; repository examples must use
  placeholders.
- When changing public behavior, update the relevant docs and examples. The
  top-level `README.md` and `README_cn.md` are paired and should stay in sync.
- If user-facing docs under `docsite/docs/` change, keep localized counterparts
  aligned where they already exist, especially `docsite/i18n/zh-Hans/...`.
- Use `rg` for search and focused reads before editing. Do not assume module
  boundaries from filenames alone.

## Rust Workspace

- Rust sources live under `anda_bot/src/`; the workspace root is `Cargo.toml`.
- The package uses Rust edition 2024 and workspace-managed dependencies.
- Put shared dependency versions in the root `Cargo.toml`, then reference them
  from `anda_bot/Cargo.toml`.
- The executable entrypoint is `anda_bot/src/main.rs`; daemon lifecycle is in
  `anda_bot/src/daemon.rs`; config parsing lives under `anda_bot/src/config/`.
- Engine-facing behavior is mostly under `anda_bot/src/engine/`; IM channel
  behavior is under `anda_bot/src/channel/`; cron is under `anda_bot/src/cron/`.
- Prefer typed structs, `serde`/`serde_saphyr`, and existing request metadata
  helpers instead of ad hoc JSON/string parsing.
- When adding persisted fields to DB-backed structs, check schema versions,
  indexes, compatibility defaults, and tests.

## Channel And Identity Invariants

Channel code has security and routing implications. Preserve these invariants:

- `ChannelMessage.external_user` is the runtime signal for untrusted external IM
  senders. External users must not be treated as the owner or a trusted partner.
- `ChannelRuntime` routes channel conversations by `(channel, reply_target,
  thread)`. Preserve `thread` when a platform has a conversation/thread/session
  identifier.
- For WeChat, `session_id` is the safest available discussion-space marker in
  the current SDK boundary. Preserve it into `thread` and request metadata when
  maintaining family-mode or group-chat behavior.
- If `allow_external_users` is changed for any channel, review config structs,
  `anda_bot/assets/config.yaml`, README examples, docs, prompt wrapping, memory
  attribution, and targeted channel tests together.
- Replies must return to the original route. Keep `reply_target`, `thread`, and
  request metadata synchronized across channel receive, agent execution, cron
  follow-ups, and completion hooks.

## Frontend And Docs

- Use pnpm for JavaScript subprojects; the workspace is declared in
  `pnpm-workspace.yaml`.
- Prefer `pnpm --dir <subproject> ...` commands because package names may not be
  stable enough for filter-based commands.
- `website/` and `chrome-extension/` are Svelte projects. Follow existing
  component, store, and styling conventions.
- `docsite/` is Docusaurus. Keep install commands, release URLs, channel
  examples, and Brain terminology aligned with the top-level READMEs.
- For UI work, verify responsive behavior and keep localized strings in sync
  with existing `_locales` or Docusaurus `i18n` files.

## Useful Commands

Rust:

```bash
cargo fmt
cargo test -p anda_bot -- --nocapture
cargo clippy --all-targets --all-features
make test
make lint
```

Targeted Rust tests:

```bash
cargo test -p anda_bot external_user -- --nocapture
cargo test -p anda_bot wechat_thread -- --nocapture
```

Chrome extension:

```bash
pnpm --dir chrome-extension check
pnpm --dir chrome-extension test
pnpm --dir chrome-extension i18n
pnpm --dir chrome-extension build
```

Website:

```bash
pnpm --dir website check
pnpm --dir website test
pnpm --dir website lint
pnpm --dir website build
```

Docs:

```bash
pnpm --dir docsite typecheck
pnpm --dir docsite build
```

Run only the checks that match the files touched, then broaden verification when
the change crosses runtime boundaries or public contracts.

## Known Environment Notes

- Full Rust tests may start local HTTP services. In sandboxed environments,
  failures like `PermissionDenied: Operation not permitted` during local bind
  setup can be environmental; preserve the exact failure text in reports.
- Some commands require network access or preinstalled dependencies. If
  dependency installation is needed, ask before changing the environment.
- The repository may have unrelated working-tree changes. Do not revert changes
  you did not make unless explicitly asked.
