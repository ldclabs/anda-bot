# Anda Bot

[English](README.md) | [简体中文](README_cn.md)

> 源自熊猫，觉醒为 Anda。

Anda Bot 是一个开源、本地优先的 AI 智能体。它将长期记忆与文件操作、命令执行、浏览器和外部服务连接起来，让你从 Anda 桌面端、Chrome 侧边栏、终端或即时通讯应用中，与同一个智能体协作。

Rust 可执行程序 `anda` 运行本地后台服务，并内嵌 [Anda Brain](https://github.com/ldclabs/anda-brain)。对话、记忆图谱和任务状态保存在本机；模型请求发送给你配置的服务商。

[官网](https://anda.bot) · [使用文档](https://docs.anda.bot/zh-Hans/) · [下载](https://github.com/ldclabs/anda-bot/releases/latest)

## 主要能力

- **跨对话延续上下文。** Brain 将有用的偏好、人物、项目和决策整理为知识图谱。记忆工作区支持查看来源、更正记录、停止使用，以及预览删除范围。
- **持续处理复杂任务。** 智能体可以维护计划、分派子任务、压缩上下文，并围绕明确目标继续工作。运行时技能包含与 Claude Code、Codex 等外部工具协作的工作流。
- **在项目中工作。** Anda 桌面端集成聊天、文件预览、Git 差异与提交、可恢复的工作树、交互终端和内置浏览器。终端客户端可以从当前目录开始工作。
- **连接工具和服务。** 管理本地与远程 MCP 服务、OAuth 登录、工具权限、资源，以及从其他应用导入的配置。其他智能体也可以通过 Anda 自身的 MCP 接口使用它的记忆。
- **自动处理周期性工作。** 定时运行智能体提示词或 shell 命令、查看执行历史，也可以响应支持事件订阅的 MCP 服务。
- **选择合适的交互入口。** 接入 Telegram、微信、Discord、Lark/飞书；通过已配置的模型理解图片、音频和视频附件；单独启用语音转写和语音输出。

## 快速开始

### Anda 桌面端

从 [GitHub Releases](https://github.com/ldclabs/anda-bot/releases/latest) 下载安装包：

| 平台 | 安装包 |
| --- | --- |
| macOS，Apple Silicon | `Anda-mac-arm64.dmg` |
| Windows，x64 | `Anda-win-x64.exe` |

macOS 也可以使用 Homebrew：

```bash
brew install --cask ldclabs/tap/anda-desktop
```

打开应用，按照「连接模型」引导操作：登录 ChatGPT 并选择可用模型，或选择 API 服务商预设并填写密钥。之后可以从「设置 → 通用 → 连接模型」重新打开引导。

桌面端会使用已有的 `anda` 安装，或安装随应用提供的运行时和技能。使用同一个 Anda 数据目录时，桌面端、CLI 和 Chrome 扩展共享后台服务与数据。托盘提供服务管理、登录启动、日志和更新入口。使用发行版无需安装 Rust、Node.js 或 pnpm。

工作台操作、打包和更新方式见[桌面端指南](desktop/README.md)。目前桌面发行包面向 macOS Apple Silicon 和 Windows x64；Intel Mac 用户可以从源码构建。

### 命令行

Homebrew：

```bash
brew install ldclabs/tap/anda
```

macOS 和 Linux：

```bash
curl -fsSL https://raw.githubusercontent.com/ldclabs/anda-bot/main/scripts/install.sh | sh
```

Windows PowerShell：

```powershell
irm https://raw.githubusercontent.com/ldclabs/anda-bot/main/scripts/install.ps1 | iex
```

CLI 发行版支持 macOS arm64、Linux x86_64/arm64 和 Windows x86_64。安装脚本会安装可执行程序与内置技能、更新 `PATH`、注册登录启动并重启后台服务。shell 安装脚本接受 `ANDA_NO_AUTOSTART=1` 和 `ANDA_NO_START=1`；下载到本地的 PowerShell 脚本接受 `-NoAutostart` 和 `-NoStart`。

安装后打开新终端。如果存在多份安装，在 macOS/Linux 上用 `command -v anda`，或在 PowerShell 中用 `Get-Command anda`，确认实际运行的程序。

在无桌面环境的 Linux 上，Anda 需要可用且已解锁的 Secret Service 密钥环。**全新安装**且没有密钥环时，可在启动 Anda 前显式运行 `anda user init-file-keys`，使用文件身份密钥。此命令不用于恢复已有安装，详见[身份配置](docsite/i18n/zh-Hans/docusaurus-plugin-content-docs/current/runtime/channels.mdx)。

```bash
anda
```

该命令打开 TUI，并在需要时启动后台服务。首次启动会创建 `~/.anda/config.yaml`。没有配置模型时，Anda 提供设置入口；完成模型配置后，智能体、Brain、消息渠道和 cron 才会启用。

### 连接模型

可以选择 ChatGPT 登录或模型服务商 API Key。

**ChatGPT 登录**

```bash
anda auth login chatgpt
anda
```

CLI 通过浏览器完成登录，并选择第一个可用模型；添加 `--model <model-slug>` 可以指定模型。用 `anda auth accounts`、`anda auth models <profile-id>` 和 `anda auth use <profile-id> <model-slug>` 管理选择。TUI 中也可以按 Ctrl+G 登录。

ChatGPT 套餐连接使用独立的加密凭据，无需 API Key。启用 ChatGPT 套餐模型期间，后台服务只接受本地所有者的智能体请求，其他可信用户和外部 IM 发送者无法使用。语音和附件处理能力可能需要单独的服务商。无桌面环境下可用 `anda auth login chatgpt --no-browser --port 1455` 配合 SSH 隧道完成登录。

**API 服务商**

编辑生成的 `config.yaml`，或使用桌面端的「设置 → 智能体配置」。[内置配置](anda_bot/assets/config.yaml) 提供了服务商预设。下面是基于当前默认预设的最小示例：

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

将 `model.active` 设为对应服务商条目的模型 ID。`api_key` 留空时，可以读取匹配的环境变量，例如 `DEEPSEEK_API_KEY`、`OPENAI_API_KEY`、`ANTHROPIC_API_KEY` 或 `GEMINI_API_KEY`。配置文件中的密钥优先。环境变量必须对后台服务进程可见；登录时启动的服务不会继承你后来在终端中导出的变量。

修改模型配置后执行：

```bash
anda models reload
```

模型重载会更新智能体的服务商配置。Brain 保持启动时选定的模型，直到执行 `anda restart`。它优先选择带 `brain` 标签的模型，其次是 `memory`，最后回退到当前模型。`image`、`audio` 和 `video` 标签用于选择附件处理模型，只应赋予支持相应输入的模型。

## 日常使用

在项目目录中运行 `anda`，或在桌面端为新对话选择文件夹。会话启动时会读取该工作区根目录下的 `AGENTS.md`，让项目约定随任务一起进入上下文。

| 对话命令 | 用途 |
| --- | --- |
| `/new` | 开始新对话。 |
| `/goal <objective>` | 围绕目标持续工作，由监督智能体检查进度。 |
| `/steer <instruction>` | 调整正在进行的工作。 |
| `/side <question>` 或 `/btw <question>` | 发起独立的临时侧线请求。 |
| `/skill <name> <task>` 或 `$<name> <task>` | 调用运行时技能。 |
| `/stop` | 停止当前工作及其后台任务，保留对话以便继续输入。 |
| `/cancel` | 停止工作并关闭当前对话会话。 |

TUI 中，Enter 发送，Shift+Enter 或 Ctrl+J 换行，Ctrl+U 清空输入，Ctrl+C 退出客户端。退出客户端不会关闭后台服务；停止服务请用 `anda stop`。

不打开 TUI，直接执行一次请求：

```bash
anda agent run --workspace . --prompt "介绍这个项目的结构"
```

命令会等待任务完成，并以 JSON 输出结果。可以用 `--prompt-file` 代替 `--prompt` 从文件读取输入，用 `--output-json` 保存结果，或用 `--wait-timeout-secs 120` 限制等待时间。超时只结束 CLI 的等待，后台任务可能仍在运行。

默认文件工作区是 `~/.anda/workspace`，可通过 `config.yaml` 的 `workspaces` 增加持久工作区。经过本地客户端认证并选择的项目目录也会成为该对话的 shell 工作目录，并按照审批策略开放文件读取和搜索。文件编辑工具仍限于配置的根目录，其他位置的修改通过 shell 审批处理。详见[运行时配置](docsite/i18n/zh-Hans/docusaurus-plugin-content-docs/current/runtime/configuration.mdx)。

### 审批与无人值守任务

交互会话会按照策略，为有风险的 shell 操作和 MCP 动作请求审批。审批卡片展示将执行的内容，并提供允许与拒绝选项。会话内的 shell 审批可以覆盖相同或更小范围的操作；MCP 权限可以按服务或工具设置。

`anda --full-access` 以完全访问模式启动交互会话，在策略允许的范围内跳过审批。**Cron 任务和处于活动状态的目标任务使用完全访问权限。** MCP 事件自动化不会获得这一权限提升，需要审批的操作会被拒绝。显式配置的 MCP `ask` 策略在完全访问模式下也仍然要求审批。这些是本地运行时的控制，shell 命令以你的操作系统用户权限执行。

## 长期记忆

配置好模型后即可使用日常记忆，无需部署独立 Brain 服务或启用可选编译特性。Brain 在后台将对话整理为结构化记忆，召回相关上下文，并持续维护图谱。

可以先告诉 Anda 一条稳定的偏好，等待后台整理，再用 `/new` 开新对话，请它回忆这条偏好。助手回复「记住了」不代表记忆处理已经完成。

```bash
anda memory guide     # 离线入门指南
anda memory           # 连接与处理状态
anda memory activity  # 已记录的处理活动
```

TUI 提供 `/memory` 和 `/memory activity`。桌面端和扩展仪表盘的「记忆」工作区支持查看记录、实体关系、来源引用，以及经过预览确认的更正和删除。状态查询不调用模型；记忆召回和整理可能调用模型。

可以为新对话限制记忆使用：

```bash
anda agent run --memory-mode no-store --prompt "协助完成任务，但不保存长期记忆"
anda agent run --memory-mode off --prompt "只使用这次新对话的信息"
```

`no-store` 允许召回，但不新增 Brain 记忆；`off` 同时禁用读取和写入。这些模式也会阻止尚不支持相应限制的嵌套智能体和持久化工具。聊天历史、文件、日志和服务商处理仍然保留。

可选的持久待办可以请你澄清记忆或响应变化。`anda memory inbox setup` 会先预览配置；按输出中的命令应用并重启后才会启用。自动学习是需要明确配置的独立能力，并不会因为启用了记忆而默认开启。记忆控制、删除范围、待办和可选学习能力见 [Brain 集成指南](docs/brain-integration_cn.md)。

### 与其他智能体共享记忆

为 Claude Code、Codex 或其他本地 MCP 客户端创建独立授权：

```bash
anda memory agents add claude-code --write
anda memory agents
```

第一条命令只展示一次令牌，并打印 Streamable HTTP 接口的连接说明，默认地址为 `http://127.0.0.1:8042/mcp/memory`。省略 `--write` 可创建只读授权。授权包含 `recall_memory`、`search_memory` 和 `read_memory_entity`；可写授权还包含 `remember` 和 `memory_note_status`。笔记会归属于写入它的智能体。

这类授权可以读取所有者的记忆，只应发给可信客户端。接口要求令牌、只接受回环主机名，并拒绝网页来源。用 `anda memory agents revoke <id-or-client>` 撤销访问。

## 集成

### Chrome 侧边栏

[Chrome 扩展](chrome-extension/README.md) 将浏览器连接到后台服务，让 Anda 查看网页、管理标签页并操作网站。使用 Node.js 和 pnpm 从本仓库构建：

```bash
pnpm install --frozen-lockfile --filter ./chrome-extension
pnpm --dir chrome-extension build
anda browser token --days 30
```

在 Chrome 116 或更新版本中打开 `chrome://extensions`，启用开发者模式，选择「加载已解压的扩展程序」，并选中 **`chrome-extension/dist`**。打开侧边栏，在设置中填写命令输出的 Gateway URL 和令牌。如果后台服务尚未运行，用 `anda start` 启动。

桌面端内置浏览器使用独立的用户配置；Chrome 扩展则将 Anda 连接到你的 Chrome 会话。

### MCP 服务

桌面端和扩展仪表盘的 MCP 页面支持添加服务、浏览 MCP Registry、导入已有配置、管理密钥和审阅工具。CLI 提供相同的管理功能：

```bash
anda mcp list
anda mcp add docs --url https://mcp.example.com/mcp
anda mcp import --dry-run
anda mcp reload
```

示例 URL 是占位符，请替换为实际服务地址。本地服务通过 stdio 连接，需要预先安装相应命令或运行时；远程服务通过 HTTP 连接，并可支持 OAuth 登录。配置保存在 `~/.anda/mcp.json`，与 `config.yaml` 分开，支持 `mcpServers` 和 `servers` 两种根键。手动编辑后需要重载。

用 `anda mcp secret set NAME` 保存密钥，再通过 `${secret:NAME}` 引用。MCP 工具受审批策略和定义变更审阅约束。服务还可以提供可附加到消息中的资源，或在工具执行期间请求用户输入。

支持 MCP Events 的服务可以通过 Events 页签或 `anda mcp triggers` 创建事件自动化。这类任务有事件批处理、频率限制和连续失败自动暂停机制。配置、导入、OAuth、权限与事件投递详见 [MCP 配置指南](docsite/i18n/zh-Hans/docusaurus-plugin-content-docs/current/runtime/configuration.mdx)。

### 即时通讯渠道

Telegram、微信、Discord 和 Lark/飞书在 `config.yaml` 的 `channels` 下配置，例如：

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

用 `anda channel list` 查看已配置条目，修改后执行 `anda restart`。已配置的微信条目可以用 `anda channel init wechat` 完成扫码登录。

`allowed_users` 校验平台发送者。可选的 `user` 将渠道绑定到通过 `anda user create <id>` 创建的 Anda 可信用户；省略时使用本地所有者身份。`allow_external_users: true` 允许其他发送者以不可信的 `$external_user` 身份交互，不会将他们视为所有者。回复保留原始渠道和线程/会话路由。配置方法与限制见[渠道和身份指南](docsite/i18n/zh-Hans/docusaurus-plugin-content-docs/current/runtime/channels.mdx)。

### 语音与附件

通过 `transcription` 启用语音输入，通过 `tts` 启用语音回复，两者独立于主聊天模型。客户端也支持附件，并按模型的能力标签路由处理。

在 macOS 和 Windows 上，可以通过 CLI 使用麦克风输入：

```bash
anda voice --record-secs 8 --no-playback
```

去掉 `--no-playback` 可播放语音回复，需要配置 TTS，并在 `PATH` 中提供 `ffplay`、macOS 的 `afplay` 或 SoX 的 `play`。Edge TTS 需要 `edge-tts`。Linux 未启用 CLI 录音。详见[语音配置](docsite/i18n/zh-Hans/docusaurus-plugin-content-docs/current/runtime/configuration.mdx)。

## 技能与定时任务

在桌面端或扩展仪表盘的技能工作区中管理运行时技能。个人技能位于 `~/.anda/skills`，发行版管理的技能位于 `~/.anda/bundled-skills`，也可以发现 `~/.agents/skills` 下的共享技能。仓库的 [skills](skills) 包含研究、文档处理、编程工具和技能编写等工作流；具体技能可能需要额外工具。

可以直接请 Anda 定时执行提示词或 shell 命令，也可以使用桌面端的「自动化」页面。Cron 支持一次性、固定间隔和 cron 表达式调度，提供本地执行历史及暂停/恢复控制。定时任务需要后台服务保持运行。MCP 事件自动化在 MCP 页面管理。

## 服务管理与本地数据

| 命令 | 用途 |
| --- | --- |
| `anda status` | 查看后台服务状态，加 `--json` 输出结构化结果。 |
| `anda start` / `anda stop` / `anda restart` | 管理后台服务。 |
| `anda daemon` | 在前台运行服务。 |
| `anda models reload` | 重载模型服务商配置。 |
| `anda autostart install` / `uninstall` / `status` | 管理登录启动。 |
| `anda update` | 更新脚本安装的发行版；Homebrew 安装使用 `brew upgrade anda`。 |
| `anda update --skills` | 更新内置技能。 |
| `anda validate-config` | 从标准输入校验 YAML，不启动服务或创建数据目录。 |
| `anda --help` | 查看命令；子命令也支持 `--help`。 |

桌面端可通过托盘或设置中的「检查更新」更新；Homebrew 管理的组件使用 Homebrew 升级。内置运行时与平台差异见[桌面端更新说明](desktop/README.md#updates--更新)。

默认网关地址为 `127.0.0.1:8042`。运行时状态位于 `~/.anda`，Windows 对应 `%USERPROFILE%\.anda`：

| 路径 | 内容 |
| --- | --- |
| `config.yaml` | 模型、渠道、工作区、语音和启动设置。 |
| `db/` | 对话、记忆图谱、任务状态和其他持久记录。 |
| `workspace/` | 默认工作文件。 |
| `channels/` | 渠道状态和附件。 |
| `skills/`、`bundled-skills/` | 个人技能和发行版管理的技能。 |
| `mcp.json`、`mcp_secrets.json`、`mcp_state.json` | MCP 配置、本地受保护的密钥，以及连接与审阅状态。 |
| `credentials/`、`chatgpt/` | 加密的可信用户凭据和 ChatGPT 账户状态。 |
| `logs/` | 运行日志。 |

后台服务和所有者的身份密钥通常保存在操作系统安全凭据库中；显式初始化的文件密钥和 Linux 回退密钥使用 `keys/`。桌面端还会在操作系统应用数据目录中保存界面偏好和浏览器配置。本地存储不代表离线推理：已配置的模型服务商和外部服务会接收调用所需的内容。

使用 `anda --home /absolute/path/to/anda-home` 隔离状态。若同时运行多个后台服务，应为它们配置不同的 `addr` 端口。升级前备份数据和身份密钥，旧 Brain 数据库可能需要迁移；兼容性与回滚限制见 [Brain 指南](docs/brain-integration_cn.md#产品-api-与开发依赖)。

## 开发

Rust 工作区包含 `anda_bot` 包和 `anda` 可执行程序。构建需要 **Rust 1.95 或更新版本**、本地 C/C++ 构建工具链，以及 `PATH` 中的 `protoc`。CI 会安装这些前置依赖，各平台配置见[测试工作流](.github/workflows/test.yml)和[发布工作流](.github/workflows/release.yml)。

```bash
git clone https://github.com/ldclabs/anda-bot.git
cd anda-bot
cargo build --locked -p anda_bot --bin anda
cargo run --locked -p anda_bot --bin anda --
```

共享的 Brain/DB/KIP/Core/Engine 依赖由 Cargo 解析，无需在同级目录克隆 `anda-brain`。精确版本记录在 `Cargo.lock` 中。可选的 `learning` 和 `mib` 特性互相独立：前者提供需要配置的学习集成，后者增加隔离的 [MIB 评测宿主](docs/mib-integration_cn.md)。日常记忆不需要它们，编译这些特性也不会自动开启学习。

JavaScript 子项目由根目录的 pnpm workspace 管理。使用 Node.js 22.12+（CI 使用 Node.js 24）和 pnpm：

```bash
pnpm install --frozen-lockfile
pnpm --dir desktop dev
```

| 目录 | 职责 |
| --- | --- |
| `anda_bot/src/` | CLI、后台服务、引擎、Brain 集成、渠道、cron、语音和 TUI。 |
| `anda_bot/assets/` | 默认配置、提示词和运行时示例。 |
| `desktop/` | Electron + Svelte 桌面应用，复用扩展的 UI 组件。 |
| `chrome-extension/` | Svelte + TypeScript Chrome 侧边栏与仪表盘。 |
| `skills/` | 内置运行时技能包。 |
| `docsite/` | Docusaurus 文档和翻译。 |
| `website/` | SvelteKit 官网。 |
| `scripts/` | 安装、签名和发布脚本。 |
| `docs/` | 集成指南、设计文档和实施记录。 |

按修改的组件运行检查。Rust：

```bash
cargo fmt --check
RUST_MIN_STACK=16777216 cargo test --locked -p anda_bot -- --nocapture
cargo clippy --locked --all-targets --all-features
```

`make test` 使用所有特性运行 Rust 工作区测试，并设置相同的测试线程栈大小。直接运行测试命令时保留 `RUST_MIN_STACK=16777216`，以满足内嵌 Brain 调试构建的栈需求。

| 组件 | 检查 |
| --- | --- |
| 桌面端 | `pnpm --dir desktop check`、`test`、`build` |
| Chrome 扩展 | `pnpm --dir chrome-extension check`、`test`、`i18n`、`build` |
| 官网 | `pnpm --dir website check`、`test`、`lint`、`build` |
| 文档站 | `pnpm --dir docsite typecheck`、`build` |

表中每个脚本都使用相同的 `pnpm --dir <directory>` 前缀。仓库约定见 [AGENTS.md](AGENTS.md)，GUI 检查见[桌面端验证说明](desktop/README.md#validation)。

## 许可证

[Apache-2.0](LICENSE)。内置的第三方技能可能包含各自的许可证文件。
