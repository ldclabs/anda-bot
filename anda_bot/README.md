# Anda Bot

Born of panda. Awakened as Anda.

Anda Bot is an AI agent with long-term memory, local tool use, and a self-evolving runtime powered by ANDA Hippocampus. The executable packages that agent with a local daemon, a terminal chat UI, optional IRC/Telegram/Discord bridges, and a persistent cron scheduler.

## What It Does

- Starts a local HTTP gateway on `addr` (default `127.0.0.1:8042`).
- Hosts both the agent engine and the Hippocampus memory APIs in the same process.
- Persists conversations, channel messages, cron jobs, run history, keys, and object storage state under a local home directory.
- Provides an inline TUI chat client that can launch or reconnect to the daemon automatically.
- Optionally listens on IRC, Telegram, or Discord channels, maps messages into agent conversations, and posts completions back to the same route.
- Exposes agent tools for memory recall, shell execution, notes, todos, file search/read/write/edit, conversation history, skills, and cron management.

## Quick Start

From the repository root:

```bash
cargo run -p anda_bot --
```

This launches the `anda` TUI. On the first run it creates a local home at `~/.anda` by default and writes a starter config to `~/.anda/config.yaml` if one does not exist.

If required config values are missing, the UI stays in setup mode. Edit `config.yaml`, save it, then press Enter in the TUI to reload. You can also type `/reload` once chat is available.

To run only the daemon in the foreground:

```bash
cargo run -p anda_bot -- daemon
```

To stop or restart a background daemon:

```bash
cargo run -p anda_bot -- stop
cargo run -p anda_bot -- restart
```

`stop` and `restart` are implemented for Unix platforms.

## CLI Summary

The `anda` binary accepts an optional `--home` flag:

```bash
cargo run -p anda_bot -- --home /path/to/.anda
```

Commands:

- No subcommand: start the interactive TUI.
- `daemon`: run the daemon in the foreground.
- `stop`: stop a background daemon.
- `restart`: stop and relaunch the background daemon.

## Home Directory Layout

By default Anda Bot stores runtime state in `~/.anda`:

```text
~/.anda/
	config.yaml
	anda-daemon.pid
	db/
	keys/
		anda_bot.key
		user.key
	logs/
		anda-daemon.log
	sandbox/
	skills/
```

Notes:

- `config.yaml` controls the gateway address, model providers, proxy, sandbox flag, and optional channel bridges.
- `keys/` stores the daemon identity key and the local user key.
- `db/` stores AndaDB-backed conversation, memory, channel, and cron state.
- `logs/anda-daemon.log` is used when a daemon is started in the background.
- `skills/` is loaded into the runtime skill manager.
- `sandbox/` is used by the shell tool when `sandbox: true`.

## Configuration

The generated template lives in [assets/config.yaml](assets/config.yaml).

Minimal top-level settings:

```yaml
addr: 127.0.0.1:8042
sandbox: false
# https_proxy: http://127.0.0.1:7890
```

The runtime will refuse to fully start until the active model provider is valid. At minimum, these fields must resolve:

- `model.active`
- `model.providers.<active>.family`
- `model.providers.<active>.model`
- `model.providers.<active>.api_base`
- `model.providers.<active>.api_key`

Example:

```yaml
model:
	active: DeepSeek
	providers:
		DeepSeek:
			family: anthropic
			model: "deepseek-v4-pro"
			api_base: "https://api.deepseek.com/anthropic"
			api_key: "..."
			label: "pro"
			disabled: false

		MiniMax:
			family: anthropic
			model: "MiniMax-M2.7-highspeed"
			api_base: "https://api.minimaxi.com/anthropic/v1"
			api_key: "..."
			label: "flash"
			disabled: false
```

Provider ordering matters only for fallback preference. The active provider is loaded first; other non-disabled providers follow behind it.

## Channel Bridges

IRC, Telegram, Discord, and Lark/Feishu support are configured under `channels.irc`, `channels.telegram`, `channels.discord`, and `channels.lark`.

```yaml
channels:
	irc:
		- id: libera
			server: irc.libera.chat
			port: 6697
			nickname: anda-bot
			username: anda
			channels:
				- "#anda"
			allowed_users:
				- "*"
			verify_tls: true
	discord:
		- id: server
			bot_token: "..."
			guild_id: "123456789012345678"
			allowed_users:
				- "111111111111111111"
			mention_only: true
	lark:
		- id: work
			app_id: "cli_xxx"
			app_secret: "..."
			allowed_users:
				- "ou_xxx"
			mention_only: true
			platform: lark
			receive_mode: websocket
```

Behavior:

- Each IRC entry must include `server` and `nickname`.
- Each Telegram or Discord entry must include `bot_token`.
- Each Lark entry must include `app_id` and `app_secret`; use `platform: feishu` for Feishu endpoints.
- `id` is optional; if omitted, the server name becomes the channel runtime identifier.
- `allowed_users` can restrict who may trigger the bot. Use `"*"` to allow anyone.
- Messages are normalized for IRC output, kept plain-text, and threaded back to the original reply target.
- Discord file attachments are passed to the agent as resources, and outgoing resources are uploaded when possible.
- Lark image, file, and audio messages are passed to the agent as resources; audio uses the existing transcription pipeline when enabled.
- Channel routes are persisted so later completions can continue the same conversation.

## TUI Notes

The TUI is chat-first.

- Press Enter to send a message.
- Press Shift+Enter to insert a newline.
- Use `/new` to start a fresh local conversation.
- Use `/reload` to reload config and reconnect.
- Press Ctrl+C to quit.

When chat is available, the TUI polls conversation state from the local gateway and streams assistant updates inline.

## Runtime Architecture

At a high level the daemon starts four collaborating parts:

1. `brain`: a local Hippocampus space mounted at `/v1/anda_bot/...` for formation, recall, and memory management.
2. `engine`: the Anda agent runtime, served under `/engine/{id}` with tools for shell, files, notes, todos, cron, skills, and memory recall.
3. `cron`: a persistent scheduler that can run shell jobs or submit prompts back into the agent runtime.
4. `channel`: an IRC/Telegram/Discord/Lark runtime that ingests messages, binds them to conversation IDs, and sends completions back through a completion hook.

The daemon stores the current working directory as the agent work directory. File tools always operate relative to that working directory. Shell execution runs there as well unless `sandbox: true`, in which case shell commands are routed into the local sandbox runtime.

## License

Copyright © LDC Labs

Licensed under Apache-2.0. See the repository-level [LICENSE](../LICENSE).
