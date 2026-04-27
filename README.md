# anda-bot

[English](README.md) | [简体中文](README_CN.md)

anda-bot is an AI agent with long-term memory, local tool use, and a self-evolving runtime powered by ANDA Hippocampus.

This repository currently exposes that agent through [anda_bot](anda_bot/README.md), the package that ships the `anda` binary.

## Overview

The project combines several pieces into one local runtime:

- An inline terminal UI for chat.
- A local daemon with an HTTP gateway.
- A long-term memory service built on ANDA Hippocampus.
- A tool-enabled agent runtime backed by anda_engine.
- Persistent conversation, channel, and cron storage powered by AndaDB.
- Optional IRC ingestion and response delivery.
- A persistent cron scheduler for shell jobs or agent prompts.

The daemon merges two HTTP surfaces on the same local address:

- `/engine/{id}` for the agent runtime.
- `/v1/anda_bot/...` for Hippocampus memory and space management.

## Quick Start

Requirements:

- A recent Rust toolchain.
- At least one configured model provider API key.

Run the TUI:

```bash
cargo run -p anda_bot --
```

Run the daemon in the foreground:

```bash
cargo run -p anda_bot -- daemon
```

Stop or restart a background daemon on Unix:

```bash
cargo run -p anda_bot -- stop
cargo run -p anda_bot -- restart
```

The runtime home defaults to `~/.anda`. Override it with `--home`:

```bash
cargo run -p anda_bot -- --home /path/to/.anda
```

## First-Run Behavior

On first launch, the binary creates the home directory and writes a starter config file to `~/.anda/config.yaml` when needed.

If the active model provider is incomplete, the TUI stays in setup mode and reports the missing keys. Edit the config file, save it, then press Enter in the TUI to reload.

The runtime also creates these subdirectories as needed:

- `keys/` for the daemon key and local user key.
- `db/` for object-store and database state.
- `logs/` for background daemon logs.
- `skills/` for runtime-loaded skills.
- `sandbox/` for shell isolation when sandboxing is enabled.

## Configuration Model

The canonical template is [anda_bot/assets/config.yaml](anda_bot/assets/config.yaml).

Core fields:

```yaml
addr: 127.0.0.1:8042
sandbox: false
# https_proxy: http://127.0.0.1:7890

model:
	active: DeepSeek
	providers:
		DeepSeek:
			family: anthropic
			model: "deepseek-v4-pro"
			api_base: "https://api.deepseek.com/anthropic"
			api_key: "..."

channels:
	irc: []
```

The active provider must resolve to a non-disabled provider with `family`, `model`, `api_base`, and `api_key` filled in.

IRC channels are optional. When configured, incoming channel or DM traffic is translated into agent prompts and the resulting completions are sent back to the original route.

## Architecture

Key implementation areas:

- [anda_bot/src/main.rs](anda_bot/src/main.rs): CLI entrypoint and command dispatch.
- [anda_bot/src/daemon.rs](anda_bot/src/daemon.rs): runtime directory management, key loading, local database startup, and service orchestration.
- [anda_bot/src/tui](anda_bot/src/tui): inline terminal chat UI.
- [anda_bot/src/engine](anda_bot/src/engine): agent runtime, tool registration, and conversation APIs.
- [anda_bot/src/brain](anda_bot/src/brain): Hippocampus integration for formation and recall.
- [anda_bot/src/channel](anda_bot/src/channel): IRC ingestion, routing, retries, and completion delivery.
- [anda_bot/src/cron](anda_bot/src/cron): persistent scheduler, job storage, and run history.
- [anda_bot/src/gateway](anda_bot/src/gateway): local HTTP API surface and client.

Notable runtime behavior:

- Conversation history is stored persistently and exposed to the TUI through a dedicated conversations tool.
- The engine registers tools for memory recall, shell, notes, todos, file read/search/edit/write, skills, and cron control.
- File tools operate relative to the current working directory where `anda` is launched.
- Shell execution uses the working directory by default, or the local sandbox when `sandbox: true`.
- Cron jobs can run either shell commands or agent prompts, and their run history is persisted.

## Repository Layout

```text
.
├── Cargo.toml
├── README.md
├── README_CN.md
└── anda_bot/
		├── Cargo.toml
		├── README.md
		├── assets/
		└── src/
```

For package-specific usage details, configuration examples, and TUI notes, see [anda_bot/README.md](anda_bot/README.md).

## License

Licensed under Apache-2.0. See [LICENSE](LICENSE).
