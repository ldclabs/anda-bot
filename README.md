# Anda Bot

[English](README.md) | [简体中文](README_cn.md)

> Born of panda. Awakened as Anda.

Anda Bot is an open-source Rust AI agent that runs in your terminal, remembers across sessions, and can work on long-horizon goals. It is built to remember, reason, use tools on your computer, coordinate subagents, and continuously improve through collaboration.

Its primary differentiator is [Anda Brain](https://github.com/ldclabs/anda-brain), the memory engine powering the agent. Brain turns conversations into a living Cognitive Nexus: a graph of people, projects, preferences, events, decisions, and changing facts. Instead of simply searching raw chat logs, Anda Bot can autonomously distill useful knowledge, construct context, identify relationships, and carry historical context forward into future sessions.

## Why Anda Bot

- **Graph-based Long-term Memory:** Remembers through a knowledge graph (Anda Brain), rather than a disjointed pile of chat logs.
- **Autonomous Learning:** Discovers and distills key insights from past work, recalling them contextually when needed.
- **Long-Horizon Execution:** Capable of running reasoning tasks that persist and continue across compacted conversations.
- **Rich Tool Integration:** Out-of-the-box support for external tools (e.g., Claude Code, Codex), shell commands, files, notes, tasks, skills, and cron jobs.
- **Subagents Coordination:** A robust system for delegating, auditing, and coordinating specialized tasks among subagents.
- **Rust & Local-First:** Written in Rust, fully open-source, and optimized to run locally in the terminal.
- **Multi-Channel Runtime:** Operates in the terminal and can be optionally connected to Telegram, WeChat, Discord, and Lark/Feishu.
- **Voice Support:** Supports speech-to-text input and text-to-speech output when configured.
- **Self-Contained State:** Keeps all configuration and runtime files under a local home directory.

## Long-Horizon Work And Subagents

Anda Bot is designed for tasks that require long-term continuity, going beyond simple single-turn question-answering. A goal can remain active as the agent inspects progress, compacts context, links across conversational threads, invokes tools, and executes until the objective is verified as complete. The subagent system allows specialized workers to take on focused roles (e.g., implementation, review, research, or supervision) while the main agent maintains the overarching plan and memory thread.

External coding tools are fully integrated into this execution loop. When needed, Anda Bot can collaborate with tools like Claude Code and Codex, execute local shell/file commands, load custom runtime skills, and persist critical outcomes in Anda Brain for future reference.

## Memory & Knowledge Graph

Anda Brain is designed for agents that need memory to grow instead of merely accumulate. Its core loop has three parts:

- **Formation:** Conversations are encoded into structured memory fragments (entities, relationships, events, preferences, and patterns).
- **Recall:** The agent queries the memory graph via natural language, receiving context-rich answers instead of flat keyword search hits.
- **Maintenance:** Brain can consolidate fragments, merge duplicates, decay stale knowledge, and track timelines as facts evolve.

This allows users to establish a natural feedback loop: state facts or preferences that should persist across sessions, correct the agent when things change, or query what the agent remembers. If a workflow preference evolves over time, the system learns the transition rather than blindly overwriting past knowledge.

## Quick Start

Install the latest release:

With Homebrew:

```bash
brew install ldclabs/tap/anda
```

On macOS, the Homebrew formula installs both `anda` and `anda_launcher`. Run
`anda_launcher` once to start the menu bar launcher and refresh
`~/Applications/Anda Bot.app`.

macOS and Linux with the install script:

```bash
curl -fsSL https://raw.githubusercontent.com/ldclabs/anda-bot/main/scripts/install.sh | sh
```

Windows users should download `AndaBotSetup-windows-x86_64.exe` from the
[latest release](https://github.com/ldclabs/anda-bot/releases/latest) and
double-click it. The installer places Anda under
`%LOCALAPPDATA%\Programs\AndaBot`, installs curated skills, creates Start Menu
and desktop shortcuts, registers the tray launcher to start at login, starts
the launcher, lets you configure provider/API key/model in a GUI wizard, and
checks for downloaded updates that can be installed with a restart prompt.

Advanced users and CI can still use the PowerShell path:

```powershell
irm https://raw.githubusercontent.com/ldclabs/anda-bot/main/scripts/install.ps1 | iex
```

The macOS shell installer also installs `~/Applications/Anda Bot.app`,
registers the menu bar launcher at login, and starts it immediately; the
launcher starts the daemon after setup. It can also check for updates from the
menu bar and prompt to install and restart after an update is downloaded. Linux
shell installs register daemon autostart directly. For PowerShell, use
`-NoAutostart` or `-NoStart` to opt out; for the shell installer, set
`ANDA_NO_AUTOSTART=1` or `ANDA_NO_START=1`.

Requirements:

- At least one model provider API key. Windows installer users can enter it in
  the setup wizard; CLI users can put it in `~/.anda/config.yaml` or a supported
  environment variable.

Or run Anda Bot from this repository with a recent Rust toolchain:

```bash
git clone https://github.com/ldclabs/anda-bot.git
cd anda-bot
cargo run -p anda_bot --
```

On first launch, the daemon creates `~/.anda/config.yaml`. If the setup screen indicates a missing model configuration, open this file, specify the provider details, save it, then refresh models from the launcher or browser side panel, or run `anda models reload`. For API keys, you can also export a provider environment variable before starting Anda.

Minimal model configuration:

```yaml
model:
  active: "deepseek-v4-pro"
  providers:
    - family: anthropic
      model: "deepseek-v4-pro"
      api_base: "https://api.deepseek.com/anthropic"
      api_key: "YOUR_API_KEY" # optional when DEEPSEEK_API_KEY is set
      labels: ["pro", "brain"]
      disabled: false
```

Supported model key environment variables include `OPENAI_API_KEY`, `ANTHROPIC_API_KEY`, `GEMINI_API_KEY`, `GOOGLE_API_KEY`, `DEEPSEEK_API_KEY`, `MINIMAX_API_KEY`, `MIMO_API_KEY`, `MOONSHOT_API_KEY`, `KIMI_API_KEY`, `BIGMODEL_API_KEY`, and `GLM_API_KEY`. A value in `config.yaml` takes precedence over the environment.

The `brain` label designates the preferred provider for memory processing. If no provider has this label, the active model is used.

Use a separate home directory when you want an isolated profile:

```bash
anda --home /path/to/.anda
```

## Interacting with the Agent

When the terminal UI is running:

- Press Enter to send.
- Press Shift+Enter, or Ctrl+J in terminals that do not report Shift+Enter, to insert a newline.
- Press Up or Down to move through multi-line input.
- Press Ctrl+U to clear the input.
- Press Ctrl+A or Ctrl+E to jump to the start or end of the input.
- Use `anda models reload`, or the launcher/browser refresh models button, after editing model providers in `config.yaml`.
- Use `/reload` after changing daemon settings that still require a restart.
- Use `/stop` to stop the current task and leave the conversation idle.
- Use `/cancel` to exit the active conversation task.
- Use `/steer ...` to nudge an in-progress response.
- Press Esc to show status, and Ctrl+C to quit.

Successful conversation turns are submitted to Anda Brain for memory formation in the background. Users do not need to manage memory files manually.

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

Manage the background daemon:

```bash
anda status
anda start
anda stop
anda restart
anda models reload
anda autostart status
```

Send a one-time prompt and wait for the complete result without opening the terminal UI:

```bash
anda agent run --prompt "Summarize what you remember about my current project"
```

Start a voice conversation:

```bash
anda voice --record-secs 8
```

Voice mode requires `transcription.enabled: true`. Spoken playback also requires `tts.enabled: true`; use `--no-playback` if you only want microphone input and text output.

## Integrations

Anda Bot can be used directly in the terminal, opened as a Chrome extension, or connected to various external messaging channels by editing `~/.anda/config.yaml`.

### Chrome Side Panel

The repository includes an unpacked Chrome extension in [chrome-extension](chrome-extension). It opens Anda in Chrome's native Side Panel and lets the agent inspect pages and manage browser tabs through split browser tools while keeping one stable browser session as you switch tabs.

The side panel can also bookmark assistant messages. Bookmarks are saved in the local daemon, can be organized into folders, and can jump back to the original conversation from the side panel or dashboard.

Generate a local bearer token for the extension:

```bash
anda browser token --days 30
```

Then load [chrome-extension](chrome-extension) from `chrome://extensions` with Developer mode enabled, paste the printed Gateway URL and token into the side panel settings, and start chatting from any webpage.

### MCP Servers

Anda Bot can connect to MCP servers and expose their tools to the agent. Put a
portable MCP configuration in `~/.anda/mcp.json`, then restart the daemon.
`mcp.json` accepts both `mcpServers` and `servers` as the root key so you can
paste configs from other MCP-compatible tools with minimal changes.

```json
{
  "mcpServers": {
    "filesystem": {
      "type": "stdio",
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-filesystem", "$ANDA_WORKSPACE"]
    },
    "remote": {
      "type": "http",
      "url": "https://mcp.example.com/mcp",
      "headers": {
        "Authorization": "Bearer ${MCP_REMOTE_TOKEN}"
      }
    }
  }
}
```

Configured strings support `$VAR` and `${VAR}` environment expansion. `ANDA_HOME`
and `ANDA_WORKSPACE` are built in, and stdio servers default to the first Anda
workspace as their working directory.

The agent can also connect a new MCP server during a conversation by calling
`add_mcp_server`. Use `persist: false` for the current daemon only, or
`persist: true` to write the server to `~/.anda/mcp.json` for future restarts.
Its server fields mirror one `mcp.json` entry: `type`, `command`, `args`,
`env`, `cwd`, `url`, `headers`, `enabled`, `include`, and `exclude`, plus the
tool-only `id` and `persist` fields.

Supported channel families:

- Telegram
- WeChat
- Discord
- Lark / Feishu

Multiple trusted users can share one daemon and the same Anda agent. Create a user key, then set a channel entry's `user` to the matching id. If `user` is omitted, channel messages run as the local owner identity stored in the OS secure credential store.

```bash
anda user create alice
anda user list
```

The command writes the new public key under top-level `users` and saves the matching private key in the local encrypted credential store under `~/.anda/credentials/`. The credential file contains an encrypted COSE Key, with its encryption key derived from the local daemon identity secret. Use `anda user export` below when you explicitly need a file key.

On Linux, if no Secret Service provider is available or unlocked, Anda falls back to private daemon/owner key files under `~/.anda/keys/` and prints/logs a warning. Trusted-user private keys remain encrypted in the local credential store as long as the daemon identity secret can be loaded. To use Secret Service for daemon/owner identities, start and unlock a provider in a user D-Bus session, for example `gnome-keyring-daemon --start --components=secrets`, make sure `DBUS_SESSION_BUS_ADDRESS` is set for the Anda process, then restart Anda. KDE users can unlock KWallet instead.

To export an existing identity private key to a file, use `anda user export`. The identity can be `daemon`, `owner`, `default`, or a trusted user id:

```bash
anda user export daemon --key-path ./anda-daemon.key
anda user export owner --key-path ./anda-owner.key
anda user export alice --key-path ./alice.key
```

Keep exported private key files out of source control and shared folders.

The config still looks like this:

```yaml
users:
  - id: alice
    pubkey: "ALICE_ED25519_PUBLIC_KEY"
  - id: ops
    pubkey: "OPS_ED25519_PUBLIC_KEY"
```

Minimal Telegram example:

```yaml
channels:
  telegram:
    - id: personal
      user: alice
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
      user: alice
      # Optional. When empty, you can run `anda channel init wechat` to initialize, scan QR code, and obtain a token.
      bot_token: ""
      username: anda-wechat
      allowed_users:
        - "*"
      allow_external_users: false
      route_tag:
```

`allowed_users` still checks the platform sender, such as a Telegram account, WeChat `wxid`, Discord user id, or Lark open id. `user` chooses the trusted Anda caller that owns the resulting conversations, resources, and memory context.

Set `allow_external_users: true` to accept non-allowlisted IM senders as `$external_user`. They can interact with the bot, but are treated as untrusted and are not the owner/partner.

See the `mcp.json` example above for MCP servers, and [anda_bot/assets/config.yaml](anda_bot/assets/config.yaml) for channel, transcription, and TTS examples.

## Files, Skills, And Automations

The local runtime creates a workspace directory at `~/.anda/workspace`. File and shell tools operate in this folder by default.

Custom runtime skills can be added under `~/.anda/skills`. Release-managed skills are installed in `~/.anda/bundled-skills`, and shared cross-agent skills from `~/.agents/skills` can be imported into the personal library via the Dashboard. Integrated cron capabilities enable scheduling shell commands or automated agent prompts, with execution histories stored locally.

## Local Data And Privacy

By default, Anda Bot stores all state and configuration under `~/.anda`:

```text
~/.anda/
  config.yaml
  credentials/ # local encrypted trusted-user credentials
  db/
  keys/ # explicit file keys or Linux Secret Service fallback keys
  logs/
  channels/
  bundled-skills/
  sandbox/
  skills/
  skills-manifest.json
  skill-backups/
  skill-trash/
  workspace/
```

The memory graph, conversations, channel state, cron jobs, logs, personal skills, bundled skills, and workspace data live there. Daemon and owner identity private keys live in the OS secure credential store by default, while trusted-user private keys live in the local encrypted credential store under `~/.anda/credentials/`. Explicitly exported keys and Linux Secret Service fallback keys may exist under `~/.anda/keys/`. Your configured model providers will receive prompts and memory-processing requests, so choose providers and API endpoints that match your privacy preferences.

## Learn More

- [Anda Bot package guide](anda_bot/README.md)
- [Anda Brain](https://github.com/ldclabs/anda-brain)
- [Anda Brain product site](https://brain.anda.ai/)

## License

Licensed under Apache-2.0. See [LICENSE](LICENSE).
