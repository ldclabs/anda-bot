# Anda Bot

[English](README.md) | [简体中文](README_cn.md)

> Born of panda. Awakened as Anda.

I am Anda Bot: a local AI agent with a long-term memory brain. Most agents are useful for one conversation and then start over. I am built to remember, recall, use tools on your computer, and keep improving as we work together.

My most important difference is [Anda Hippocampus](https://github.com/ldclabs/anda-hippocampus), the memory engine behind me. Hippocampus turns conversations into a living Cognitive Nexus: a graph of people, projects, preferences, events, decisions, and changing facts. That means I do not just search old text. I can build context, notice relationships, and carry useful history into future conversations.

## Why Use Me

- I remember through a graph brain, not a pile of disconnected chat logs.
- I can recall what matters from past work when you ask, or when it helps the current task.
- I can use local tools for shell commands, files, notes, todos, skills, and scheduled jobs.
- I can live in your terminal, and optionally in IRC, Telegram, WeChat, Discord, or Lark/Feishu.
- I can support voice conversations when transcription and speech output are configured.
- I keep my runtime state under your local home directory by default.

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

- At least one model provider API key.

Or run me from this repository with a recent Rust toolchain:

```bash
git clone https://github.com/ldclabs/anda-bot.git
cd anda-bot
cargo run -p anda_bot --
```

On first launch I create `~/.anda/config.yaml`. If the setup screen says a model field is missing, open that file, fill in your provider details, save it, then press Enter in the terminal UI.

Minimal model configuration:

```yaml
model:
  active: DeepSeek
  providers:
    DeepSeek:
      family: anthropic
      model: "deepseek-v4-pro"
      api_base: "https://api.deepseek.com/anthropic"
      api_key: "YOUR_API_KEY"
      labels: ["pro", "hippocampus"]
      disabled: false
```

The `hippocampus` label lets the memory brain prefer that provider for memory work. If no provider has that label, I use the active model.

Use a separate home directory when you want an isolated profile:

```bash
anda --home /path/to/.anda
```

## Chat With Me

When the terminal UI is running:

- Press Enter to send.
- Press Shift+Enter to insert a newline.
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

You can keep me in the terminal, or connect me to chat channels by editing `~/.anda/config.yaml`.

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
      username: anda_bot
      allowed_users:
        - "*"
      mention_only: false
```

See [anda_bot/assets/config.yaml](anda_bot/assets/config.yaml) for full channel, transcription, and TTS examples.

## Files, Skills, And Automations

My local runtime creates a working area at `~/.anda/workspace`. File and shell tools operate there by default. If `sandbox: true`, shell execution is routed through `~/.anda/sandbox`.

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
