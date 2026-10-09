# Anda Bot

[English](README.md) | [简体中文](README_cn.md)

> Born of panda. Awakened as Anda.

Anda Bot is an open-source Rust AI agent with a Chrome extension, an Electron desktop client, and CLI/TUI access. It runs locally, remembers across sessions, and can work on long-horizon goals. It is built to remember, reason, use tools on your computer, coordinate subagents, and continuously improve through collaboration.

Its primary differentiator is [Anda Brain](https://github.com/ldclabs/anda-brain), the memory engine powering the agent. Brain turns conversations into a living Cognitive Nexus: a graph of people, projects, preferences, events, decisions, and changing facts. Instead of simply searching raw chat logs, Anda Bot can autonomously distill useful knowledge, construct context, identify relationships, and carry historical context forward into future sessions.

## Why Anda Bot

- **Graph-based Long-term Memory:** Remembers through a knowledge graph (Anda Brain), rather than a disjointed pile of chat logs.
- **Autonomous Learning:** Discovers and distills key insights from past work, recalling them contextually when needed.
- **Long-Horizon Execution:** Capable of running reasoning tasks that persist and continue across compacted conversations.
- **Rich Tool Integration:** Out-of-the-box support for external tools (e.g., Claude Code, Codex), shell commands, files, skills, and cron jobs.
- **Subagents Coordination:** A robust system for delegating, auditing, and coordinating specialized tasks among subagents.
- **Rust & Local-First:** Written in Rust, fully open-source, and optimized to run locally in the terminal.
- **Multi-Channel Runtime:** Operates in the terminal and can be optionally connected to Telegram, WeChat, Discord, and Lark/Feishu.
- **Voice Support:** Supports speech-to-text input and text-to-speech output when configured.
- **Self-Contained State:** Keeps all configuration and runtime files under a local home directory.

## Long-Horizon Work And Subagents

Anda Bot is designed for tasks that require long-term continuity, going beyond simple single-turn question-answering. A goal can remain active as the agent inspects progress, compacts context, links across conversational threads, invokes tools, and executes until the objective is verified as complete. The subagent system allows specialized workers to take on focused roles (e.g., implementation, review, research, or supervision) while the main agent maintains the overarching plan and memory thread.

External coding tools are fully integrated into this execution loop. When needed, Anda Bot can collaborate with tools like Claude Code and Codex, execute local shell/file commands, load custom runtime skills, and persist critical outcomes in Anda Brain for future reference.

## Memory & Knowledge Graph

Start with `anda memory guide` (offline), then tell Anda a preference in an ordinary chat. After background processing, open a new conversation with `/new` and ask about it. `anda memory` and TUI `/memory` show connection and processing status without model calls; the browser **Memory** workspace shows records, verified sources and reviewable corrections/removals. A chat acknowledgement is not proof of a saved fact.

Memory activity is listed by submission time. Record lists and active subscriptions continue across pages; cancelled subscriptions do not consume the active list limit. Recall requests are not automatically replayed after an HTTP error.

Use `anda agent run --memory-mode no-store --prompt '…'` or `--memory-mode off` to start a fresh conversation with enforced Brain restrictions. Chat history, files and provider processing remain. Persistent questions are optional: `anda memory inbox setup` previews the configuration before an explicit apply and restart. See [the memory guide and scope](docs/brain-integration.md#start-with-ordinary-memory).

Anda Brain is designed for agents that need memory to grow instead of merely accumulate. Its core loop has three parts:

- **Formation:** Conversations are encoded into structured memory fragments (entities, relationships, events, preferences, and patterns).
- **Recall:** The agent queries the memory graph via natural language, receiving context-rich answers instead of flat keyword search hits.
- **Maintenance:** Brain can consolidate fragments, merge duplicates, decay stale knowledge, and track timelines as facts evolve.

This allows users to establish a natural feedback loop: state facts or preferences that should persist across sessions, correct the agent when things change, or query what the agent remembers. If a workflow preference evolves over time, the system learns the transition rather than blindly overwriting past knowledge.

## Quick Start

### Anda Desktop

Anda Desktop is the chat window, workbench and tray for your local Anda. Download it from the [latest release](https://github.com/ldclabs/anda-bot/releases/latest):

- macOS Apple Silicon: `Anda-mac-arm64.dmg`
- Windows x64: `Anda-win-x64.exe`
- Homebrew: `brew install --cask ldclabs/tap/anda-desktop` (installs the `anda` formula too)

New macOS CLI and desktop releases are available for Apple Silicon only. Intel Mac users can build from source.

The desktop, your terminal and the Chrome extension share one `anda` and one daemon. On start, Anda Desktop keeps an existing install (from the install script, `ANDA_INSTALL_DIR` or Homebrew), or installs its bundled `anda` and curated skills where the install scripts put them: `~/.local/bin` on macOS, `%LOCALAPPDATA%\Programs\AndaBot` on Windows. A newer CLI is never downgraded, and Homebrew installs are updated with `brew upgrade`. The model connection guide includes a launch-at-login option; the tray then keeps the service running while the window is closed.

When no model is configured, the desktop automatically opens **Connect a model**. Sign in with ChatGPT and confirm a model, or choose a preset from the bundled `config.yaml` and enter its API key. Anda saves and loads the default model before returning to chat. **Set up later** keeps a reminder in chat; reopen the guide from **Settings → General → Connect a model**. Existing model configurations are reused.

The desktop tray replaces the retired Anda Bot menu bar launcher. Installing Anda Desktop, rerunning an install script or updating an old launcher removes the launcher's login entry, app bundle and shortcuts, and keeps the daemon starting at login if the launcher did.

**Check for updates** in the tray and Settings checks both `anda` and the desktop app. Installing `anda` waits for active tasks to finish, stops the service during installation, then starts it again. A failed check, such as while offline, keeps an already downloaded `anda` release in the tray. The check continues to the desktop app even if the `anda` update is installed, postponed or fails; desktop updates (signed release builds) restart only the app without stopping the service. The tray keeps **Check for updates** alongside the individual install actions, and also restarts the service, copies a Chrome extension token and opens the logs.

Signed desktop releases automatically check for app updates one minute after launch, then every six hours after each check finishes, even with the window closed or the service stopped. New versions appear in the tray; downloads and installation still require confirmation. Checks with no update or a network failure stay quiet.

The desktop client shares chat, memory, skills, bookmarks and configuration components with the Chrome extension. The workbench includes server-pushed chat updates with durable submission receipts, an isolated embedded browser with agent tools, interactive terminals, Git changes/commits and recoverable worktrees, and an audio self-test page. Chrome automation remains available through the extension. Config editing is owner-only and rejects stale saves; `anda validate-config` validates YAML from stdin without initializing a home or daemon.

Release packages are signed and notarized when the release environment provides credentials; otherwise they are unsigned and have no in-app update feed. To build a local installer, run `pnpm install --filter @anda/desktop...` and `pnpm --dir desktop package`; see [desktop installation, development and validation](desktop/README.md) and the [implementation and validation record](docs/desktop-client-implementation.md).

### Command line

Install the latest `anda` release without the desktop:

With Homebrew:

```bash
brew install ldclabs/tap/anda
```

macOS and Linux with the install script:

```bash
curl -fsSL https://raw.githubusercontent.com/ldclabs/anda-bot/main/scripts/install.sh | sh
```

If you also have a Homebrew install, open a new terminal and verify
`command -v anda` points at `~/.local/bin/anda` or your `ANDA_INSTALL_DIR`;
otherwise an older Homebrew binary may shadow the updated install-script binary.

Windows with PowerShell:

```powershell
irm https://raw.githubusercontent.com/ldclabs/anda-bot/main/scripts/install.ps1 | iex
```

The install scripts install `anda` and the curated skills, add it to `PATH`,
register the daemon to start at login with `anda autostart install` and restart
it. On Windows that login entry is a per-user `Run` value that runs
`anda start` (a console window appears briefly); Anda Desktop's login start has
no window. For PowerShell, use `-NoAutostart` or `-NoStart` to opt out; for the
shell installer, set `ANDA_NO_AUTOSTART=1` or `ANDA_NO_START=1`.

Requirements:

- At least one model provider API key. Enter it in the desktop **Connect a model** guide or under
  **Settings → Agent configuration**, or put it in `~/.anda/config.yaml` or a
  supported environment variable.

Or run Anda Bot from this repository with Rust 1.95 or newer:

Source builds track the KIP 2.0 stack: Brain 0.13 with its Memory Interface binding, Nexus/DB/KIP 0.14 and the matching Core/Engine 0.16, keeping one DB/core type identity. Brain 0.13.2 and the shared stack resolve from crates.io by default; the commented `[patch.crates-io]` entries are available for sibling-checkout development. A database that a development build wrote under the 2.1.0 draft must be migrated to a new database before this build opens it; keep the old one read-only for rollback. Brain HTTP accepts `command` or `operations` application arguments (no `kip` field) and returns KIP 2.0 envelopes; ordinary Bot tool responses retain their existing `result`/`error` format.

Development and test builds use basic optimization for dependencies to keep macOS unwind tables below the linker's 16 MiB limit. `anda_bot` itself remains unoptimized, with debug information and panic unwinding preserved. The first dependency build takes longer; subsequent builds reuse those artifacts.

Run `make test` for the full Rust suite with all features. It defaults to `RUST_MIN_STACK=16777216` (16 MiB per test thread), matching CI, because the embedded Brain's unoptimized async frames can overflow the default test-thread stack. An explicitly supplied `RUST_MIN_STACK` overrides this default. When invoking Cargo directly, use `RUST_MIN_STACK=16777216 cargo test --workspace --all-features`.

The first start with a KIP 1.x database migrates stored memory before the gateway becomes ready and can take several minutes. Commands starting a new daemon wait up to ten minutes and report an exited child promptly. Back up the database before upgrading; original task statuses and results remain available in `LegacyRecord` after migration.

The optional `mib` feature adds a loopback-only evaluation host: `anda mib --model-config /absolute/path/model.json --listen 127.0.0.1:8043`. It starts before production home/daemon initialization and uses isolated Brain runs. The agent protocol executes Bot's runner-managed business profile with MIB-owned task tools; a separate memory-backend protocol supports MIB's evaluator-owned same-model agent. See [MIB integration](docs/mib-integration.md) for lifecycle guarantees, memory controls and incomplete cost accounting.

The optional MIB host controls add forced Recall budgets (`--recall-max-tokens` plus `--recall-context-tokens`) and an evaluator-only `learning_audit`. MIB normal/ungated conditions still require complete independent instrumentation; persistent memory does not imply an enabled learning condition.

```bash
git clone https://github.com/ldclabs/anda-brain.git
git clone https://github.com/ldclabs/anda-bot.git
cd anda-bot
cargo run -p anda_bot --
```

On first launch, the daemon creates `~/.anda/config.yaml`. If the setup screen indicates a missing model configuration, open this file, specify the provider details, save it, then refresh models from Anda Desktop or the browser side panel, or run `anda models reload`. For API keys, you can also export a provider environment variable before starting Anda.

You can also connect an eligible ChatGPT Plus / Pro plan without an API key:

```bash
anda auth login chatgpt
anda auth accounts
anda auth models <profile-id>
anda auth use <profile-id> <model-slug>
anda auth logout <profile-id>
```

In Anda Desktop, use **Connect a model → Continue with ChatGPT**, then confirm the selected model. The full account controls remain under **Settings → Agent configuration → Models**. In the extension, open **Models** on the configuration page or the model section in side-panel settings. Click **Continue with ChatGPT**, complete browser authorization, then choose **Use selected model**. Press **Ctrl+G** in the TUI to sign in. CLI/TUI sign-in selects the first available model; pass `--model <slug>` to the CLI to choose another. Without a configured model, the daemon serves only authenticated setup controls; Brain, channels and cron start after model selection. All three clients share account and refresh state under the same `ANDA_HOME`. Credentials are encrypted in `chatgpt/accounts.cose`, never stored in config.yaml or browser storage.

Changing a ChatGPT plan model waits until running tasks and subagents are idle. ChatGPT plan providers are owner-only: while they are enabled, other users and external IM requests are rejected. Audio, video and transcription require separate providers. A usage limit never silently switches billing to an API key; use [ChatGPT usage settings](https://chatgpt.com/settings/usage). Existing `.codex/auth.json` configurations remain compatible, but new connections should use this dedicated sign-in flow.

For a headless or SSH host, run `anda auth login chatgpt --no-browser --port 1455` and forward `127.0.0.1:1455` from the browser's machine to the daemon host. To move a session, run `anda auth export <profile-id> --output /absolute/path/session.json` (disconnects the source), transfer the owner-only file securely over SSH, and run `anda auth import /absolute/path/session.json` on the destination. Import removes the transfer file and preserves the destination host ID. Activate a model with `anda auth use <profile-id> <model-slug>`. Never refresh the same session on both hosts.

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

Supported model key environment variables include `OPENAI_API_KEY`, `ANTHROPIC_API_KEY`, `GEMINI_API_KEY`, `GOOGLE_API_KEY`, `DEEPSEEK_API_KEY`, `MINIMAX_API_KEY`, `MIMO_API_KEY`, `MOONSHOT_API_KEY`, `KIMI_API_KEY`, `BIGMODEL_API_KEY`, and `GLM_API_KEY`. A value in `config.yaml` takes precedence over the environment. Known API endpoints select their own provider key before considering the model name: OpenRouter, Groq and SiliconFlow use `OPENROUTER_API_KEY`, `GROQ_API_KEY` and `SILICONFLOW_API_KEY`, including when hosting another vendor's model.

The `brain` label designates the preferred provider for memory processing. If no provider has this label, the active model is used.

The `image`, `audio` and `video` labels pick the providers that inspect those attachments. When the active model also has the `image` label, it receives PNG, JPEG, GIF and WebP attachments of up to 5 MiB directly with your message, for the task that message starts; remove the label from a model that cannot read images.

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
- Use `anda models reload`, or the refresh models button in Anda Desktop or the browser side panel, after editing model providers in `config.yaml`. The Brain keeps the model it started with until the daemon restarts.
- Use `/reload` after changing daemon settings that still require a restart.
- Use `/stop` to interrupt the current task, cancel its background work and pending approvals, clear the active goal, and leave the conversation idle for your next message.
- Use `/cancel` to stop the same work and close the active conversation session.
- Use `/steer ...` to nudge an in-progress response.
- Press Esc to show status, and Ctrl+C to quit.

Input preserves spaces in text and code. Cursor movement and deletion treat combined emoji as one character. Status checks, conversation polling and Brain requests run in the background, so slow responses leave the UI responsive and Ctrl+C available. Opening the UI preserves existing terminal scrollback. `/new` clears the screen and scrollback for the new conversation. Reconnecting keeps your draft, and replaces the transcript only when it restores an active conversation. In a terminal that implements the [Program Status Protocol](https://www.superlogical.com/rex/docs/build/program-status) (OSC 7501), the tab or session list shows whether Anda is working, waiting for your approval, answer or sign-in, done, or failed; other terminals ignore it.

### Command Approvals

Risky shell commands and MCP server connections raise an approval card before they run:

- With an empty input box, press `y` to approve or `n` to deny.
- If the input box already has text, those keys go to the input instead. Type `y`/`yes` or `n`/`no` and press Enter to answer, or press Ctrl+U to clear the input and use the single-key shortcuts again. The footer always shows which of the two is currently active.
- Approval cards expire after 10 minutes, and the tool call then fails.

Approval cards show the full command, details and choices, wrapping to the terminal width. A command that asks for its own working directory shows it on the card, and one outside the active workspace always needs approval. Status changes append a result to the transcript. Submission errors take priority over shortcut help; a failed text-choice submission keeps the draft for editing and retrying.

Start the terminal UI with `anda --full-access` to skip the cards for that session; the status line shows `full-access` while it is on.

Cron jobs and autonomous goal mode (`/goal ...`) always run with full access, because nobody is present to answer a card and the task would otherwise stall until the card expires.

Successful conversation turns are submitted to Anda Brain for memory formation in the background. Users do not need to manage memory files manually. Accepted Formation IDs and Recall delivery receipts are retained separately from completion and actual memory use.

Optional `brain.runtime_config` (overridden by `BRAIN_RUNTIME_CONFIG`) installs the native persistent inbox before Space loading. The browser Brain view and TUI `/brain inbox` / `/brain status` expose caller-scoped work. `learning` is a separate Cargo feature; semantic, utility, trust and automatic learning require explicit configuration and native authorization. Current IM transports are not automatic action adapters. See [Brain runtime integration](docs/brain-integration.md) for configuration, receipt semantics and learning service contracts.

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

Homebrew installs refuse `anda update`; use `brew upgrade anda`. Anda Desktop runs the same update from its tray and Settings.

Install this `anda` at the shared CLI location, as Anda Desktop does with its bundled copy (never downgrades; `--dir` picks another directory):

```bash
anda install
```

A newer local build is kept, including its bundled skills. Use `anda update --force` to explicitly install the latest release even if it is older. The same rule applies to `anda update --skills`.

Manage the background daemon:

```bash
anda status
anda start
anda stop
anda restart
anda models reload
anda autostart status
```

Autostart registration stores an absolute home directory, including when `--home` is relative. Registration failures are reported. A specific `addr` is also used for local CLI/Brain connections; only wildcard listener addresses (`0.0.0.0` or `::`) are replaced with loopback addresses.

Send a one-time prompt and wait for the complete result without opening the terminal UI:

```bash
anda agent run --prompt "Summarize what you remember about my current project"
```

`--prompt` and `--prompt-file` are mutually exclusive; one is required. Add `--wait-timeout-secs 120` to bound submission and completion polling together. The default `0` disables the overall timeout. A timeout stops the CLI wait; the daemon task may continue.

Read the memory inbox with `anda memory inbox`. If another page is available, follow the printed `anda memory inbox --cursor <next_cursor>` command; `--json` includes the cursor in `result.next_cursor`.

Start a voice conversation:

```bash
anda voice --record-secs 8
```

Voice mode requires `transcription.enabled: true`. Spoken playback also requires `tts.enabled: true` and an audio player on PATH (`ffplay`, macOS `afplay`, or SoX `play`), checked before the first recording; use `--no-playback` if you only want microphone input and text output. Voice mode waits for the current answer to finish before starting the next recording; a failed turn is reported and the next recording starts. Ctrl-C also stops speech synthesis and playback.

Google transcription accepts WAV/FLAC recordings up to 60 seconds (the Chrome extension converts other recording formats to WAV). `transcription.initial_prompt` supplies Groq/OpenAI Whisper vocabulary hints. Edge synthesis requires `edge-tts` on PATH; `tts.default_format` applies to StepFun, while Edge/OpenAI/Google output MP3. See [speech provider limits and configuration](docsite/docs/runtime/configuration.mdx#speech-providers).

## Integrations

Anda Bot can be used directly in the terminal, opened as a Chrome extension, or connected to various external messaging channels by editing `~/.anda/config.yaml`.

### Chrome Side Panel

The repository includes an unpacked Chrome extension in [chrome-extension](chrome-extension). It opens Anda in Chrome's native Side Panel and lets the agent inspect pages and manage browser tabs through split browser tools while keeping one stable browser session as you switch tabs.

Connection settings synchronize across the Side Panel and Dashboard, and changing the daemon clears cached conversation/resource data. The Dashboard loads its workspaces on demand. Voice recording remains attached to the tab where it started. `open_download` reveals a file in its folder; the user opens the file by clicking it.

The side panel can also bookmark assistant messages. Bookmarks are saved in the local daemon, can be organized into folders, and can jump back to the original conversation from the side panel or dashboard.

Generate a local bearer token for the extension:

```bash
anda browser token --days 30
```

Then load [chrome-extension](chrome-extension) from `chrome://extensions` with Developer mode enabled, paste the printed Gateway URL and token into the side panel settings, and start chatting from any webpage.

Daemon management requires a configured trusted identity; a valid signature from an unregistered key grants no access. Browser connections recheck credential expiry for subsequent requests. Generate a new token and update the extension when its token expires. Conversation source listings and binding deletion are limited to the caller's own conversations.

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

For OAuth servers, `connect_mcp_server` runs authorization and persists the connection. Reauthorization preserves tool allowlists/denylists, custom headers, client ID and transport settings, and saves updated scopes. If the browser opener fails, the tool returns the authorization URL for manual opening. The callback uses the gateway's port and loopback IP family; remote or specific-interface deployments need a tunnel from that browser-side loopback address to the actual listener.

Supported channel families:

- Telegram
- WeChat
- Discord
- Lark / Feishu

Multiple trusted users can share one daemon and the same Anda agent. Create a user key, then set a channel entry's `user` to the matching id. If `user` is omitted, channel messages run as the local owner identity stored in the OS secure credential store. Daemon controls (configuration, model reload and switching, update installation, shutdown) remain reserved for the local owner.

```bash
anda user create alice
anda user list
```

The command writes the new public key under top-level `users` and saves the matching private key in the local encrypted credential store under `~/.anda/credentials/`. The credential file contains an encrypted COSE Key, with its encryption key derived from the local daemon identity secret. Use `anda user export` below when you explicitly need a file key.

On Linux, if no Secret Service provider is available or unlocked, Anda can read **existing** daemon/owner key files at `~/.anda/keys/anda_bot.key` and `~/.anda/keys/user.key`, and prints/logs a warning. Both files must be present: an unavailable keyring never causes replacement identities to be generated. For an existing installation, unlock the provider or restore the original keys. Trusted-user private keys remain encrypted with a key derived from the original daemon identity.

For a **new installation without an OS keyring**, explicitly run `anda user init-file-keys` before `anda start`. This creates missing private key files without overwriting existing files, and refuses initialization if local database/credential data or accessible keyring identities already exist. Do not use it to recover an existing installation. Imported COSE keys must identify the Ed25519 curve; an algorithm, when present, must be EdDSA, and a private key’s public component must match its secret. Raw 32-byte Base64 keys remain supported.

To use Secret Service, start and unlock a provider in a user D-Bus session, for example `gnome-keyring-daemon --start --components=secrets`, make sure `DBUS_SESSION_BUS_ADDRESS` is set for Anda, then restart it. KDE users can unlock KWallet instead. Existing file identities are migrated together to one keyring bundle; the source files are removed only after the bundle is saved successfully.

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

Channel delivery preserves Telegram topics, Discord threads and Lark/Feishu reply threads. Text and attachments retry independently without replaying successfully delivered parts. `https_proxy` also covers WebSocket traffic; WeChat does not need a proxy and connects without it. Lark/Feishu links HTTP(S) attachments and uploads local or binary attachments as image or file messages. Keep channel workspace attachment files with database backups.

Set `allow_external_users: true` to accept non-allowlisted IM senders as `$external_user`. They can interact with the bot, but are treated as untrusted and are not the owner/partner.

See the `mcp.json` example above for MCP servers, and [anda_bot/assets/config.yaml](anda_bot/assets/config.yaml) for channel, transcription, and TTS examples.

## Files, Skills, And Automations

The local runtime creates a workspace directory at `~/.anda/workspace`. File and shell tools operate in this folder by default. A shell command still running after a short foreground wait (10 seconds by default, up to 30) keeps running in the background for up to 24 hours: its output is reported to the conversation, the agent can poll or stop it with `shell_session`, and `/stop` cancels it. When the interactive CLI connects, it registers its launch directory with the daemon using the local owner's credentials; native shell commands for that CLI session run from the registered directory. The Chrome extension and Anda Desktop likewise register a directory-backed chat when you enter it, so its shell commands start in that directory. The daemon keeps these registrations in memory for up to 24 hours and forgets them when it restarts, so each client registers the directory again with every message it sends. Other sources cannot select an arbitrary shell directory through request metadata alone. Filesystem tools remain limited to configured workspaces. Attachment understanding can also read the owner's registered directories while their grants remain valid; request metadata alone cannot authorize a new directory.

For a one-shot request, `anda agent run --workspace . --prompt "Summarize this project"` registers the directory before submitting the prompt and uses its `cli:<path>` source by default. Relative `--workspace` paths resolve from the CLI's current directory. Explicit `source` and `workspace` values in `--meta` still take precedence; filesystem tools remain limited to configured workspaces.

Custom runtime skills can be added under `~/.anda/skills`. Release-managed skills are installed in `~/.anda/bundled-skills`, and shared cross-agent skills from `~/.agents/skills` can be imported into the personal library via the Dashboard. Integrated cron capabilities enable scheduling shell commands or automated agent prompts, with execution histories stored locally. Cron runs remain active until the agent or shell command finishes, and schedule edits preserve paused state. Cron tools are available only to trusted users. CLI-created jobs retain the working-directory authorization validated at creation, including across daemon restarts.

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
