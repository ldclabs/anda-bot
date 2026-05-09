# Anda Bot User Guide

[Project README](../README.md) | [简体中文](../README_cn.md)

> Born of panda. Awakened as Anda.

This page is the practical guide for the `anda` binary. If you are deciding whether to use Anda Bot, start with the [project README](../README.md). If you are ready to run me, configure me, or connect me to your daily tools, you are in the right place.

## Start Me

From the repository root:

```bash
cargo run -p anda_bot --
```

This opens the terminal chat UI and starts or reconnects to the local daemon. On first launch I create `~/.anda/config.yaml` and the runtime directories I need.

If setup is incomplete, the UI will list the missing config fields. Edit `~/.anda/config.yaml`, save it, then press Enter in the UI. Once chat is available, `/reload` also reloads the config.

Use a different home directory for a separate identity, profile, or test environment:

```bash
cargo run -p anda_bot -- --home /path/to/.anda
```

## Configure A Model

The generated template is [assets/config.yaml](assets/config.yaml). The active provider must include `family`, `model`, and `api_base`. Set `api_key` in the file, or leave it empty and export a matching environment variable before starting Anda.

```yaml
model:
  active: "deepseek-v4-pro"
  providers:
    - family: anthropic
      model: "deepseek-v4-pro"
      api_base: "https://api.deepseek.com/anthropic"
      api_key: "YOUR_API_KEY" # optional when DEEPSEEK_API_KEY is set
      labels: ["pro", "hippocampus"]
      disabled: false
```

Supported model key environment variables include `OPENAI_API_KEY`, `ANTHROPIC_API_KEY`, `GEMINI_API_KEY`, `GOOGLE_API_KEY`, `DEEPSEEK_API_KEY`, `MINIMAX_API_KEY`, `MIMO_API_KEY`, `MOONSHOT_API_KEY`, `KIMI_API_KEY`, `BIGMODEL_API_KEY`, and `GLM_API_KEY`. A value in `config.yaml` takes precedence over the environment.

Provider labels help me choose models for different jobs. A provider labeled `hippocampus` is preferred for memory formation and recall support. If no provider has that label, I use the active model.

Useful top-level settings:

| Setting         | What it does                                                    |
| --------------- | --------------------------------------------------------------- |
| `addr`          | Local gateway address, default `127.0.0.1:8042`.                |
| `sandbox`       | Routes shell execution through `~/.anda/sandbox` when true.     |
| `https_proxy`   | Optional proxy for outbound model and channel requests.         |
| `model`         | Model providers used by the agent and memory brain.             |
| `transcription` | Speech-to-text providers for voice input and audio attachments. |
| `tts`           | Text-to-speech providers for spoken playback.                   |
| `channels`      | Optional chat integrations.                                     |

## Chat UI Controls

- Enter sends the message.
- Shift+Enter inserts a newline.
- Ctrl+U clears the input.
- Ctrl+A and Ctrl+E move to the start and end of the input.
- `/reload` reloads `config.yaml` and reconnects.
- `/stop` or `/cancel` interrupts the current response.
- `/steer ...` gives an in-progress response extra guidance.
- Esc shows status.
- Ctrl+C quits.

When a response succeeds, conversation turns are sent to Anda Hippocampus for memory formation. I keep conversation state locally, so later prompts in the same workspace can continue with the right context.

## Teach My Memory

Hippocampus works best when important facts are stated clearly. You can speak naturally:

```text
Remember that this project uses release branches, not trunk-based releases.
When I say staging, I mean the shared QA environment.
My preference changed: use concise answers by default, but include exact commands for deployment issues.
What do you remember about the last incident review?
```

The memory brain stores more than text snippets. It can form entities, relationships, events, preferences, and timelines, then recall them through the `recall_memory` tool when they are useful.

## CLI Commands

| Command                                             | Use it when                                           |
| --------------------------------------------------- | ----------------------------------------------------- |
| `cargo run -p anda_bot --`                          | You want the interactive terminal UI.                 |
| `cargo run -p anda_bot -- update`                   | You want to update an install-script release.         |
| `cargo run -p anda_bot -- daemon`                   | You want the daemon in the foreground.                |
| `cargo run -p anda_bot -- stop`                     | You want to stop a background daemon on Unix.         |
| `cargo run -p anda_bot -- restart`                  | You changed config or want a fresh daemon on Unix.    |
| `cargo run -p anda_bot -- agent run --prompt "..."` | You want a single prompt without opening the UI.      |
| `cargo run -p anda_bot -- voice --record-secs 8`    | You want microphone input and optional spoken output. |

Voice mode requires `transcription.enabled: true`. Playback requires `tts.enabled: true`; add `--no-playback` to keep voice input but print text output only.

## Connect Chat Channels

Configure channels under `channels` in `~/.anda/config.yaml`.

Supported channel families:

- IRC
- Telegram
- WeChat
- Discord
- Lark / Feishu

Minimal examples:

```yaml
channels:
  telegram:
    - id: personal
      bot_token: "YOUR_TELEGRAM_BOT_TOKEN"
      username: anda_bot
      allowed_users:
        - "*"
      mention_only: false

  wechat:
    - id: personal
      username: anda-wechat
      allowed_users:
        - "*"

  lark:
    - id: work
      app_id: "cli_xxx"
      app_secret: "YOUR_APP_SECRET"
      platform: feishu
      receive_mode: websocket
      mention_only: true
```

Channel notes:

- `allowed_users` restricts who can trigger me. Use `"*"` only when that is acceptable.
- `allow_external_users: true` lets non-allowlisted IM senders talk to me as `$external_user`; they are treated as untrusted and are not the owner/partner.
- Telegram and Discord require `bot_token`.
- WeChat can use a saved token or QR login when `bot_token` is empty.
- Lark and Feishu require `app_id` and `app_secret`; use `platform: feishu` for Feishu endpoints.
- Audio attachments can be transcribed when transcription is enabled.
- Channel routes are persisted, so replies continue in the original thread, room, or recipient when possible.

## Local Files, Skills, And Cron

My file and shell tools use `~/.anda/workspace` as the default working area. This keeps local agent work separate from the repository checkout and your home directory.

Runtime skills live in `~/.anda/skills`. Put skill folders or skill documents there when you want me to learn specialized workflows.

Cron tools let me create, list, manage, and inspect scheduled jobs. A job can run a shell command or submit a future prompt back to the agent. Run history is persisted locally.

## Home Directory Layout

```text
~/.anda/
  config.yaml
  anda-daemon.pid
  channels/
  db/
  keys/
    anda_bot.key
    user.key
  logs/
  sandbox/
  skills/
  workspace/
```

What lives there:

- `config.yaml` controls models, proxy, channels, voice, and sandboxing.
- `db/` stores memory, conversations, channels, cron jobs, and object state.
- `keys/` stores local signing keys for the daemon and user.
- `logs/` stores daemon and CLI logs.
- `channels/` stores channel-specific runtime state.
- `sandbox/` is used for shell isolation when enabled.
- `skills/` is loaded by the skill manager.
- `workspace/` is the default file and shell workspace.

## Troubleshooting

- **The UI stays in setup mode:** fill every field it lists in `~/.anda/config.yaml`, save, then press Enter.
- **The daemon is unreachable:** run `cargo run -p anda_bot -- restart` on Unix, or start `cargo run -p anda_bot -- daemon` in a separate terminal.
- **Voice mode fails:** enable and configure `transcription`; also enable `tts` unless you pass `--no-playback`.
- **A channel ignores messages:** check `allowed_users`, `mention_only`, and the channel credentials.
- **A command touches the wrong files:** check `~/.anda/workspace`; that is my default working area.

## License

Copyright © LDC Labs

Licensed under Apache-2.0. See the repository-level [LICENSE](../LICENSE).
