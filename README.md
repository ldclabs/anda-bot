# Anda Bot

[English](README.md) | [简体中文](README_cn.md)

> Born of panda. Awakened as Anda.

I am Anda Bot: an open-source Rust AI agent that runs in your terminal, remembers across sessions, and can keep working on long-horizon goals. I am built to remember, reason, use tools on your computer, coordinate subagents, and keep improving as we work together.

My most important difference is [Anda Hippocampus](https://github.com/ldclabs/anda-hippocampus), the memory engine behind me. Hippocampus turns conversations into a living Cognitive Nexus: a graph of people, projects, preferences, events, decisions, and changing facts. That means I do not just search old text. I can autonomously distill useful knowledge, build context, notice relationships, and carry useful history into future conversations.

## Why Use Me

- I remember through a knowledge graph brain, not a pile of disconnected chat logs.
- I can autonomously learn the useful parts of past work and recall them when they matter.
- I can execute long-horizon reasoning tasks that continue across compacted conversations.
- I am good at using external tools, including Claude Code, Codex, shell commands, files, notes, todos, skills, and scheduled jobs.
- I have a powerful subagents system for delegating, auditing, and coordinating complex work.
- I am written in Rust, open source, and built to run locally in your terminal.
- I can live in your terminal, and optionally in IRC, Telegram, WeChat, Discord, or Lark/Feishu.
- I can support voice conversations when transcription and speech output are configured.
- I keep my runtime state under your local home directory.

## Long-Horizon Work And Subagents

Anda Bot is designed for tasks that need continuity, not just a single answer. A goal can stay active while I inspect progress, compact context, open the next linked conversation, call tools, and continue until there is evidence that the objective is done. Subagents let specialized workers take on focused roles such as implementation, review, research, or supervision, while the main agent keeps the larger plan and memory thread intact.

External coding tools are part of that loop. When a task calls for it, I can work alongside tools such as Claude Code and Codex, use local shell and file tools, load runtime skills, and preserve the important outcomes in Hippocampus for future recall.

## My Memory Brain

Anda Hippocampus is designed for agents that need memory to grow instead of merely accumulate. Its core loop has three parts:

- **Formation:** conversations are encoded into structured memories: entities, relationships, events, preferences, and patterns.
- **Recall:** I can ask the memory graph natural-language questions and receive context-rich answers instead of raw search hits.
- **Maintenance:** Hippocampus can consolidate fragments, merge duplicates, decay stale knowledge, and preserve timelines when facts change.

For you, this means a simple habit works well: tell me the things that should remain true across sessions, correct me when something changes, and ask me what I remember when you want continuity. If you once preferred one workflow and now prefer another, I should learn the evolution instead of blindly overwriting the past.

## Quick Start

Install the latest release:

With Homebrew:

```bash
brew install ldclabs/tap/anda
```

With the install script:

```bash
curl -fsSL https://raw.githubusercontent.com/ldclabs/anda-bot/main/scripts/install.sh | sh
```

On Windows PowerShell:

```powershell
irm https://raw.githubusercontent.com/ldclabs/anda-bot/main/scripts/install.ps1 | iex
```

Requirements:

- At least one model provider API key, either in `~/.anda/config.yaml` or in a supported environment variable.

Or run me from this repository with a recent Rust toolchain:

```bash
git clone https://github.com/ldclabs/anda-bot.git
cd anda-bot
cargo run -p anda_bot --
```

On first launch I create `~/.anda/config.yaml`. If the setup screen says a model field is missing, open that file, fill in your provider details, save it, then press Enter in the terminal UI. For API keys, you can also export a provider environment variable before starting Anda.

Minimal model configuration:

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

The `hippocampus` label lets the memory brain prefer that provider for memory work. If no provider has that label, I use the active model.

Use a separate home directory when you want an isolated profile:

```bash
anda --home /path/to/.anda
```

## Chat With Me

When the terminal UI is running:

- Press Enter to send.
- Press Shift+Enter, or Ctrl+J in terminals that do not report Shift+Enter, to insert a newline.
- Press Up or Down to move through multi-line input.
- Press Ctrl+U to clear the input.
- Press Ctrl+A or Ctrl+E to jump to the start or end of the input.
- Use `/reload` after editing `config.yaml`.
- Use `/stop` or `/cancel` to interrupt the current response.
- Use `/steer ...` to nudge an in-progress response.
- Press Esc to show status, and Ctrl+C to quit.

Successful conversation turns are submitted to Hippocampus for memory formation in the background. You do not need to manage memory files by hand.

Good prompts for long-term memory:

```text
Remember that I prefer concise release notes with a short risk section.
What do you remember about the payment migration project?
I used to use provider A, but now provider B is the default for this workspace.
When we talk about Alice, she means the designer on the mobile team.
```

## Useful Commands

Run Anda Bot:

```bash
anda
```

Update an install-script release to the latest version:

```bash
anda update
```

Stop or restart the background daemon on Unix:

```bash
anda stop
anda restart
```

Send a one-time prompt without opening the terminal UI:

```bash
anda agent run --prompt "Summarize what you remember about my current project"
```

Start a voice conversation:

```bash
anda voice --record-secs 8
```

Voice mode requires `transcription.enabled: true`. Spoken playback also requires `tts.enabled: true`; use `--no-playback` if you only want microphone input and text output.

## Put Me Where You Work

You can keep me in the terminal, open me from Chrome, or connect me to chat channels by editing `~/.anda/config.yaml`.

### Chrome Side Panel

The repository includes an unpacked Chrome extension in [chrome_extension](chrome_extension). It opens Anda in Chrome's native Side Panel and lets the agent inspect pages and manage browser tabs through the `chrome_browser` tool while keeping one stable browser session as you switch tabs.

Generate a local bearer token for the extension:

```bash
anda browser token --days 30
```

Then load [chrome_extension](chrome_extension) from `chrome://extensions` with Developer mode enabled, paste the printed Gateway URL and token into the side panel settings, and start chatting from any webpage.

Supported channel families:

- IRC
- Telegram
- WeChat
- Discord
- Lark / Feishu

Minimal Telegram example:
```yaml
channels:
  telegram:
    - id: personal
      bot_token: "YOUR_TELEGRAM_BOT_TOKEN"
      username: "YOUR_TELEGRAM_BOT_USERNAME"
      allowed_users:
        - "*"
      allow_external_users: false
      mention_only: false
```

Minimal Wechat example:
```yaml
channels:
  wechat:
    - id: personal
      # Optional. When empty, you can run `anda channel init wechat` to initialize, scan QR code, and obtain a token.
      bot_token: ""
      username: anda-wechat
      allowed_users:
        - "*"
      allow_external_users: false
      route_tag:
```

  Set `allow_external_users: true` to accept non-allowlisted IM senders as `$external_user`. They can interact with the bot, but are treated as untrusted and are not the owner/partner.

See [anda_bot/assets/config.yaml](anda_bot/assets/config.yaml) for full channel, transcription, and TTS examples.

## Files, Skills, And Automations

My local runtime creates a working area at `~/.anda/workspace`. File and shell tools operate there by default.

You can also add runtime skills under `~/.anda/skills`. Skills let me load focused instructions and workflows as the system grows. Cron tools let me schedule shell commands or future agent prompts, with run history stored locally.

## Local Data And Privacy

By default I store state under `~/.anda`:

```text
~/.anda/
  config.yaml
  db/
  keys/
  logs/
  channels/
  sandbox/
  skills/
  workspace/
```

The memory graph, conversations, channel state, cron jobs, keys, logs, and workspace data live there. Your configured model provider can still receive prompts and memory-processing requests, so choose providers and API endpoints that match your privacy needs.

## Learn More

- [Anda Bot package guide](anda_bot/README.md)
- [Anda Hippocampus](https://github.com/ldclabs/anda-hippocampus)
- [Anda Hippocampus product site](https://brain.anda.ai/)

## License

Licensed under Apache-2.0. See [LICENSE](LICENSE).
