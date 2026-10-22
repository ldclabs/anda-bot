# Anda Bot

[English](README.md) | [简体中文](README_cn.md)

> Born of panda. Awakened as Anda.

Anda Bot is an open-source, local-first AI agent for your computer. It combines long-term memory with tools for working on files, running commands, using a browser, and connecting to external services. You can work with the same agent from Anda Desktop, a Chrome side panel, the terminal, or messaging apps.

The Rust binary, `anda`, runs a local daemon with an embedded [Anda Brain](https://github.com/ldclabs/anda-brain). Conversations, the memory graph, and task state persist on your machine; model requests go to the providers you configure.

[Website](https://anda.bot) · [Documentation](https://docs.anda.bot) · [Downloads](https://github.com/ldclabs/anda-bot/releases/latest)

## What you can do

- **Carry context across conversations.** Brain organizes useful preferences, people, projects, and decisions into a knowledge graph. The Memory workspace lets you inspect sources, correct records, stop using them, or preview their deletion.
- **Work on extended tasks.** The agent can maintain a plan, delegate to subagents, compact context, and continue toward an explicit goal. Runtime skills include workflows for external tools such as Claude Code and Codex.
- **Work inside a project.** Anda Desktop brings together chat, file previews, Git diffs and commits, recoverable worktrees, interactive terminals, and an embedded browser. The terminal client works from your current directory.
- **Connect tools and services.** Manage local and remote MCP servers, OAuth connections, tool permissions, resources, and imports from other apps. Other agents can also access Anda's memory through its own MCP endpoint.
- **Automate recurring work.** Schedule agent prompts or shell commands, inspect their run history, or react to events from MCP servers that support event subscriptions.
- **Use different interfaces.** Connect Telegram, WeChat, Discord, or Lark/Feishu; inspect image, audio, and video attachments with configured models; enable transcription and speech output separately.

## Quick start

### Anda Desktop

Download an installer from [GitHub Releases](https://github.com/ldclabs/anda-bot/releases/latest):

| Platform | Installer |
| --- | --- |
| macOS, Apple Silicon | `Anda-mac-arm64.dmg` |
| Windows, x64 | `Anda-win-x64.exe` |

On macOS, you can also use Homebrew:

```bash
brew install --cask ldclabs/tap/anda-desktop
```

Open the app and follow **Connect a model**: sign in with ChatGPT and select an available model, or choose an API provider preset and enter its key. You can reopen the guide from **Settings → General → Connect a model**.

Desktop uses an existing `anda` installation or installs its bundled runtime and skills. It shares the daemon and data with the CLI and Chrome extension when they use the same Anda home. The tray manages the service, login startup, logs, and updates. Release packages do not require Rust, Node.js, or pnpm.

See the [desktop guide](desktop/README.md) for workbench controls, packaging, and update behavior. Desktop release packages currently target macOS Apple Silicon and Windows x64; Intel Mac users can build from source.

### Command line

Homebrew:

```bash
brew install ldclabs/tap/anda
```

macOS and Linux:

```bash
curl -fsSL https://raw.githubusercontent.com/ldclabs/anda-bot/main/scripts/install.sh | sh
```

Windows PowerShell:

```powershell
irm https://raw.githubusercontent.com/ldclabs/anda-bot/main/scripts/install.ps1 | iex
```

CLI releases cover macOS arm64, Linux x86_64/arm64, and Windows x86_64. The scripts install the binary and bundled skills, update `PATH`, register login startup, and restart the daemon. The shell installer accepts `ANDA_NO_AUTOSTART=1` and `ANDA_NO_START=1`; the downloaded PowerShell script accepts `-NoAutostart` and `-NoStart`.

Open a new terminal after installation. If multiple installations exist, check `command -v anda` on macOS/Linux or `Get-Command anda` in PowerShell to confirm which one runs.

On a headless Linux machine, Anda needs an available, unlocked Secret Service keyring. For a **new installation** without one, explicitly run `anda user init-file-keys` before starting Anda to use file-based identities. This command is not a recovery procedure for an existing installation; see [identity setup](docsite/docs/runtime/channels.mdx#multiple-users).

```bash
anda
```

This opens the TUI and starts the daemon if necessary. First launch creates `~/.anda/config.yaml`. When no model is configured, Anda exposes setup controls; the agent, Brain, channels, and cron become available after model setup.

### Connect a model

Choose ChatGPT sign-in or a model provider API key.

**ChatGPT sign-in**

```bash
anda auth login chatgpt
anda
```

The CLI signs in through your browser and selects the first available model; add `--model <model-slug>` to choose one explicitly. Use `anda auth accounts`, `anda auth models <profile-id>`, and `anda auth use <profile-id> <model-slug>` to manage the selection. The TUI also offers sign-in with Ctrl+G.

ChatGPT plan connections use their own encrypted credentials, without an API key. While a ChatGPT plan provider is enabled, the daemon accepts only the local owner for agent requests, excluding other trusted users and external IM senders. Speech and attachment-processing capabilities may need separate providers. Use `anda auth login chatgpt --no-browser --port 1455` with an SSH tunnel for headless sign-in.

**API provider**

Edit the generated `config.yaml`, or use **Settings → Agent configuration** in Desktop. The [bundled configuration](anda_bot/assets/config.yaml) contains provider presets. A minimal example using its default preset is:

```yaml
model:
  active: "deepseek-flash"
  providers:
    - family: anthropic
      model: "deepseek-flash"
      api_base: "https://api.deepseek.com/anthropic"
      api_key: "YOUR_API_KEY"
      labels: ["memory"]
      stream: true
      disabled: false
```

Set `model.active` to the provider's model ID. An empty `api_key` can use a matching environment variable, such as `DEEPSEEK_API_KEY`, `OPENAI_API_KEY`, `ANTHROPIC_API_KEY`, or `GEMINI_API_KEY`. A key in the file takes precedence. The variable must be available to the daemon process; a service started at login does not inherit variables exported later in a terminal.

After editing model settings:

```bash
anda models reload
```

Model reload updates the agent's providers. Brain keeps its startup model until `anda restart`. Its model selection prefers the `brain` label, then `memory`, then the active model. The `image`, `audio`, and `video` labels select attachment-processing models; label only models that support those inputs.

## Everyday use

Start `anda` from a project directory, or choose a folder for a new chat in Desktop. A session loads the root `AGENTS.md` from that workspace when it starts, so project conventions can accompany the task.

| Chat command | Purpose |
| --- | --- |
| `/new` | Start a new conversation. |
| `/goal <objective>` | Continue working toward a goal, with a supervisor checking progress. |
| `/steer <instruction>` | Redirect work already in progress. |
| `/side <question>` or `/btw <question>` | Run a separate side request. |
| `/skill <name> <task>` or `$<name> <task>` | Invoke a runtime skill. |
| `/stop` | Stop current work and its background tasks; keep the conversation available. |
| `/cancel` | Stop work and close the active conversation session. |

In the TUI, Enter sends, Shift+Enter or Ctrl+J inserts a newline, Ctrl+U clears the input, and Ctrl+C exits the client. Exiting the client does not shut down the daemon; use `anda stop` for that.

For a single request without the TUI:

```bash
anda agent run --workspace . --prompt "Explain this project's structure"
```

The command waits for completion and prints the result as JSON. Use `--prompt-file` instead of `--prompt` to read a file, `--output-json` to save the result, or `--wait-timeout-secs 120` to bound the wait. A timeout ends the CLI wait; the daemon may still be working.

The default file workspace is `~/.anda/workspace`; configure `workspaces` in `config.yaml` to add persistent roots. A project selected by an authenticated local client also becomes that chat's shell directory and, subject to its approval policy, is available to file reading and searching. File editing tools remain limited to configured roots; changes elsewhere go through shell approval. See [configuration](docsite/docs/runtime/configuration.mdx).

### Approvals and unattended work

Interactive sessions request approval for risky shell operations and MCP actions according to their policy. Cards show what will run and let you approve or deny it. Shell approvals can apply to the same or narrower operations during the session; MCP permissions can be set per server or tool.

`anda --full-access` starts an interactive session with approval prompts bypassed where the policy permits. **Cron jobs and active goals run with full access.** MCP event automations do not receive that elevation: operations that need approval are refused. An explicit MCP `ask` policy still requires approval, including in full-access mode. These controls govern the local runtime; shell commands execute with your OS user's permissions.

## Long-term memory

Ordinary memory works once a model is configured, without a separate Brain server or optional build feature. Brain forms structured memory from conversations in the background, recalls relevant context, and maintains the graph over time.

Try telling Anda a stable preference, allow background processing, then start a new conversation with `/new` and ask it to recall that preference. A reply saying “remembered” is not proof that processing has finished.

```bash
anda memory guide     # Offline introduction
anda memory           # Connection and processing status
anda memory activity  # Recorded processing activity
```

The TUI provides `/memory` and `/memory activity`. Desktop and the extension dashboard provide a **Memory** workspace with records, entity relationships, source quotes, and reviewed corrections/removals. Status checks do not call a model; recall and memory processing can.

For a fresh conversation with restricted memory:

```bash
anda agent run --memory-mode no-store --prompt "Help without saving long-term memory"
anda agent run --memory-mode off --prompt "Use only this new conversation"
```

`no-store` allows recall without adding Brain memory; `off` disables both. The runtime also blocks unsupported nested agents and persistence tools in these modes. Chat history, files, logs, and provider processing still remain.

An optional persistent inbox can ask you to clarify memory or respond to changes. `anda memory inbox setup` previews its configuration; follow the printed apply command and restart to enable it. Automatic learning is a separate, explicitly configured capability, not a default effect of having memory. See [Brain integration](docs/brain-integration.md) for memory controls, deletion scope, inbox setup, and optional learning.

### Share memory with other agents

Give Claude Code, Codex, or another local MCP client its own grant:

```bash
anda memory agents add claude-code --write
anda memory agents
```

The first command prints a token once and connection instructions for the Streamable HTTP endpoint at `http://127.0.0.1:8042/mcp/memory` by default. Omit `--write` for read-only access. Grants can use `recall_memory`, `search_memory`, and `read_memory_entity`; write grants also get `remember` and `memory_note_status`. Notes are attributed to the agent that wrote them.

A grant can read the owner's memory, so share it only with a trusted client. The endpoint requires a token, accepts loopback host names, and rejects browser origins. Revoke access with `anda memory agents revoke <id-or-client>`.

## Integrations

### Chrome side panel

The [Chrome extension](chrome-extension/README.md) connects your browser to the daemon and lets Anda inspect pages, manage tabs, and interact with websites. Build it from this checkout with Node.js and pnpm:

```bash
pnpm install --frozen-lockfile --filter ./chrome-extension
pnpm --dir chrome-extension build
anda browser token --days 30
```

In Chrome 116 or newer, open `chrome://extensions`, enable Developer mode, choose **Load unpacked**, and select **`chrome-extension/dist`**. Open the side panel and paste the printed Gateway URL and token into its settings. Start the daemon with `anda start` if it is not running.

Desktop's embedded browser has its own profile; the extension connects Anda to your Chrome session.

### MCP servers

Use the MCP page in Desktop or the extension dashboard to add servers, browse the MCP Registry, import existing configurations, manage secrets, and review tools. The CLI exposes the same management functions:

```bash
anda mcp list
anda mcp add docs --url https://mcp.example.com/mcp
anda mcp import --dry-run
anda mcp reload
```

The example URL is a placeholder; replace it with your server. Local servers use stdio and require their command/runtime to be installed. Remote servers use HTTP and can support OAuth sign-in. Configuration lives in `~/.anda/mcp.json`, separate from `config.yaml`; both `mcpServers` and `servers` root keys are accepted. Reload after editing it manually.

Store secrets with `anda mcp secret set NAME` and reference them as `${secret:NAME}`. MCP tools have approval policies and definition-change review. Servers may also provide resources to attach to a message or request input during a tool call.

Servers that support MCP Events can drive automations through the Events tab or `anda mcp triggers`. These runs are subject to batching, rate limits, and automatic pause after repeated failures. See the [MCP guide](docsite/docs/runtime/configuration.mdx#mcp-servers) for configuration, imports, OAuth, permissions, and event delivery.

### Messaging channels

Telegram, WeChat, Discord, and Lark/Feishu are configured under `channels` in `config.yaml`. For example:

```yaml
channels:
  telegram:
    - id: personal
      bot_token: "YOUR_TELEGRAM_BOT_TOKEN"
      username: "YOUR_TELEGRAM_BOT_USERNAME"
      allowed_users: ["YOUR_TELEGRAM_USER_ID"]
      allow_external_users: false
      mention_only: false
```

Run `anda channel list` to inspect configured entries, then `anda restart` after changes. A configured WeChat entry can use `anda channel init wechat` for QR login.

`allowed_users` checks platform senders. An optional `user` binds a channel to an Anda trusted user created with `anda user create <id>`; when omitted, it uses the local owner. `allow_external_users: true` admits other senders as untrusted `$external_user`, without treating them as the owner. Replies retain the original channel and thread/session route. See [channels and identity](docsite/docs/runtime/channels.mdx) for setup and restrictions.

### Voice and attachments

Enable `transcription` for speech input and `tts` for spoken replies. These are separate from the main chat model. The clients also support attachments routed to models with the relevant capability labels.

On macOS and Windows, you can use microphone input from the CLI:

```bash
anda voice --record-secs 8 --no-playback
```

Remove `--no-playback` for spoken replies; this requires configured TTS and `ffplay`, macOS `afplay`, or SoX `play` on `PATH`. Edge TTS requires `edge-tts`. CLI recording is not enabled on Linux. See [speech configuration](docsite/docs/runtime/configuration.mdx#speech-providers).

## Skills and scheduled tasks

Manage runtime skills from the Skills workspace in Desktop or the extension dashboard. Personal skills live in `~/.anda/skills`, release-managed skills in `~/.anda/bundled-skills`, and shared skills can be discovered from `~/.agents/skills`. The repository's [skills](skills) include research, document, coding-tool, and skill-authoring workflows; individual skills may require additional tools.

Ask Anda to schedule a prompt or shell command, or use Desktop's **Automations** page. Cron supports one-time, interval, and cron-expression schedules, with local run history and pause/resume controls. Scheduled work requires the daemon to be running. MCP event automations are managed from the MCP page.

## Service management and local data

| Command | Purpose |
| --- | --- |
| `anda status` | Inspect daemon status; add `--json` for structured output. |
| `anda start` / `anda stop` / `anda restart` | Manage the background daemon. |
| `anda daemon` | Run the daemon in the foreground. |
| `anda models reload` | Reload model providers. |
| `anda autostart install` / `uninstall` / `status` | Manage login startup. |
| `anda update` | Update an install-script release. Homebrew installs use `brew upgrade anda`. |
| `anda update --skills` | Refresh bundled skills. |
| `anda validate-config` | Validate YAML from stdin without starting services or creating a home. |
| `anda --help` | List commands; subcommands also support `--help`. |

For Desktop updates, use **Check for updates** in the tray or Settings; Homebrew-managed components are upgraded with Homebrew. See the [desktop update guide](desktop/README.md#updates--更新) for bundled runtime and platform differences.

The default gateway is `127.0.0.1:8042`. Runtime state lives under `~/.anda` (`%USERPROFILE%\.anda` on Windows):

| Path | Contents |
| --- | --- |
| `config.yaml` | Models, channels, workspaces, speech, and startup settings. |
| `db/` | Conversations, memory graph, task state, and other persisted records. |
| `workspace/` | Default working files. |
| `channels/` | Channel-specific state and attachments. |
| `skills/`, `bundled-skills/` | Personal and release-managed skills. |
| `mcp.json`, `mcp_secrets.json`, `mcp_state.json` | MCP configuration, locally protected secrets, and connection/review state. |
| `credentials/`, `chatgpt/` | Encrypted trusted-user credentials and ChatGPT account state. |
| `logs/` | Runtime logs. |

Daemon and owner identity keys normally live in the OS secure credential store; explicitly initialized file keys and the Linux fallback use `keys/`. Desktop also stores UI preferences and its browser profile in the OS application-data directory. Local storage does not mean offline inference: configured model providers and connected services receive the content needed for their calls.

Use `anda --home /absolute/path/to/anda-home` for separate state. If multiple daemons run at once, give them different `addr` ports. Back up data and identities before upgrades; older Brain databases can require migration. The [Brain guide](docs/brain-integration.md#product-api-and-development-dependency) documents compatibility and rollback constraints.

## Development

The Rust workspace contains the `anda_bot` package and the `anda` executable. Build with **Rust 1.95 or newer**, a native C/C++ build toolchain, and `protoc` on `PATH`. CI installs these prerequisites; see the [test workflow](.github/workflows/test.yml) and [release workflow](.github/workflows/release.yml) for platform setup.

```bash
git clone https://github.com/ldclabs/anda-bot.git
cd anda-bot
cargo build --locked -p anda_bot --bin anda
cargo run --locked -p anda_bot --bin anda --
```

The shared Brain/DB/KIP/Core/Engine dependencies resolve through Cargo; no sibling `anda-brain` checkout is required. Exact versions are recorded in `Cargo.lock`. Optional `learning` and `mib` features are independent: the former enables the configured learning integration, while the latter adds an isolated [MIB evaluation host](docs/mib-integration.md). Neither is required for ordinary memory, and compiling them does not enable automatic learning.

JavaScript projects use the root pnpm workspace. Use Node.js 22.12+ (CI uses Node.js 24) and pnpm:

```bash
pnpm install --frozen-lockfile
pnpm --dir desktop dev
```

| Directory | Role |
| --- | --- |
| `anda_bot/src/` | CLI, daemon, engine, Brain integration, channels, cron, speech, and TUI. |
| `anda_bot/assets/` | Default configuration, prompts, and runtime examples. |
| `desktop/` | Electron + Svelte desktop app, reusing extension UI components. |
| `chrome-extension/` | Svelte + TypeScript Chrome side panel and dashboard. |
| `skills/` | Bundled runtime skill packages. |
| `docsite/` | Docusaurus documentation and translations. |
| `website/` | SvelteKit website. |
| `scripts/` | Installation, signing, and release helpers. |
| `docs/` | Integration guides, design documents, and implementation records. |

Run checks for the components you change. Rust:

```bash
cargo fmt --check
RUST_MIN_STACK=16777216 cargo test --locked -p anda_bot -- --nocapture
cargo clippy --locked --all-targets --all-features
```

`make test` runs the Rust workspace with all features and the same enlarged test-thread stack. Keep `RUST_MIN_STACK=16777216` on direct test commands for the embedded Brain's debug builds.

| Component | Checks |
| --- | --- |
| Desktop | `pnpm --dir desktop check`, `test`, `build` |
| Chrome extension | `pnpm --dir chrome-extension check`, `test`, `i18n`, `build` |
| Website | `pnpm --dir website check`, `test`, `lint`, `build` |
| Documentation | `pnpm --dir docsite typecheck`, `build` |

In the table, each script uses the same `pnpm --dir <directory>` prefix. See [AGENTS.md](AGENTS.md) for repository conventions and [desktop validation](desktop/README.md#validation) for GUI checks.

## License

[Apache-2.0](LICENSE). Bundled third-party skills may include their own license files.
