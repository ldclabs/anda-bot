# Anda Bot

[English](README.md) | [简体中文](README_cn.md)

> Born of panda. Awakened as Anda.

Anda Bot 是一个基于 Rust 编写、开源、运行在本地终端的 AI 智能体。它具备长期记忆、长程推理、本地工具调用、Subagents 协同调度等能力，并能在与用户的协作中持续学习与成长。

其核心差异在于背后的记忆引擎 [Anda Brain](https://github.com/ldclabs/anda-brain)。Anda Brain 会将对话转化为一个持续生长的认知图谱（Cognitive Nexus），包含用户、项目、偏好、事件、关系、决策以及不断演变的事实。这意味着 Anda Bot 不仅仅是检索历史文本，而是能够自主提炼有价值的知识、构建上下文、建立关联，并将有用的历史背景带入未来的对话中。

## 核心特性

- **图谱化长期记忆**：基于知识图谱记忆大脑（Anda Brain），而非零散的聊天日志。
- **自主学习与召回**：能从过去的工作中自动提炼关键信息，并在未来对话中主动召回相关背景。
- **长程推理任务**：能够执行跨越多轮对话和复杂上下文的长周期推理任务。
- **丰富的工具生态**：擅长调用外部工具（如 Claude Code、Codex）、本地 Shell、文件系统、笔记、待办事项、自定义技能以及定时任务。
- **Subagents 协同系统**：支持将复杂任务拆解并分发给多个专门的子智能体（如实现、审查、研究、监督等角色）协同推进。
- **Rust & 本地优先**：基于 Rust 构建，完全开源，优先运行在用户本地终端。
- **多渠道接入**：既可作为终端 TUI 运行，也支持接入 Telegram、WeChat、Discord、Lark/飞书。
- **语音对话支持**：配置好语音转写和合成服务后，即可支持无缝的语音交互。
- **数据隐私可控**：所有的运行状态和数据默认保存在本地用户目录下。

## 长程任务与 Subagents

Anda Bot 专为需要连续性的复杂目标而设计，而非简单的单轮问答。一个任务目标可以保持长期活跃：智能体会自动检查进度、压缩上下文、跨越关联会话、调用工具，并持续推进直至有明确证据表明目标已达成。Subagents 协同机制允许将特定工作（如代码实现、质量审查、资料研究、运行监督）分配给专属子智能体，而主智能体则维护全局计划和记忆线索。

外部编码工具是该执行闭环的重要组成部分。在需要时，Anda Bot 可以协同 Claude Code、Codex等工具，调用本地 Shell 和文件工具，加载运行时技能（Skills），并将关键成果沉淀到 Anda Brain 中以供后续召回。

## 记忆与认知大脑

Anda Brain 的核心设计理念是让记忆有机生长，而非简单地堆积数据。其核心循环包含三个阶段：

- **Formation（生成记忆）**：对话内容被编码为结构化知识，如实体、关系、事件、偏好和行为模式。
- **Recall（召回记忆）**：支持使用自然语言向记忆图谱提问，并获取包含丰富上下文的关联知识，而非单纯的关键词匹配。
- **Maintenance（记忆维护）**：在后台合并重复信息、巩固记忆碎片、降低过时事实的置信度，并在事实演变时保留时间线。

这为用户提供了一种自然且具有连续性的互动体验：只需告知智能体需要跨会话保留的偏好、项目背景或决策依据，在事实变化时进行纠正，或在需要时直接询问智能体“你还记得什么”。若用户的工作偏好发生演变，系统会记录并学习这种演进过程，而不是简单地覆盖历史或给出矛盾的回复。

## 快速开始

安装最新发布版：

通过 Homebrew：

```bash
brew install ldclabs/tap/anda
```

在 macOS 上，Homebrew formula 会同时安装 `anda` 和 `anda_launcher`。运行一次
`anda_launcher` 即可启动菜单栏 launcher，并刷新 `~/Applications/Anda Bot.app`。

macOS 和 Linux 通过安装脚本：

```bash
curl -fsSL https://raw.githubusercontent.com/ldclabs/anda-bot/main/scripts/install.sh | sh
```

Windows 普通用户请从
[latest release](https://github.com/ldclabs/anda-bot/releases/latest) 下载
`AndaBotSetup-windows-x86_64.exe`，然后双击安装。安装器会把 Anda 安装到
`%LOCALAPPDATA%\Programs\AndaBot`，安装内置 skills，创建开始菜单和桌面快捷方式，
注册托盘 launcher 登录自启，安装完成后立即启动 launcher，并在 GUI 向导中完成
provider/API key/model 配置；launcher 还会自动检查并下载更新，下载完成后提示安装
并重启。

高级用户和 CI 仍可使用 PowerShell 路径：

```powershell
irm https://raw.githubusercontent.com/ldclabs/anda-bot/main/scripts/install.ps1 | iex
```

macOS shell 安装器也会安装 `~/Applications/Anda Bot.app`，为菜单栏 launcher 注册登录自启，并立即启动 launcher；launcher 会在完成配置后启动 daemon，也可以从菜单栏检查更新，并在更新下载完成后提示安装并重启。Linux shell 安装仍直接注册 daemon 自启。PowerShell 安装器可以用 `-NoAutostart` 或 `-NoStart` 退出默认行为；shell 安装器可以设置 `ANDA_NO_AUTOSTART=1` 或 `ANDA_NO_START=1`。

前置要求：

- 至少一个可用的模型提供方 API key。Windows 安装器用户可以在 GUI 向导中填写；CLI 用户可以写在 `~/.anda/config.yaml`，也可以通过支持的环境变量提供。

也可以使用较新的 Rust 工具链从源码编译运行 Anda Bot：

```bash
git clone https://github.com/ldclabs/anda-bot.git
cd anda-bot
cargo run -p anda_bot --
```

首次启动时，daemon 会自动创建 `~/.anda/config.yaml`。如果界面提示模型配置缺失，请打开该文件，填写 provider 详情，保存后在 launcher 或浏览器侧边栏点击刷新模型，或运行 `anda models reload`。对于 API Key，也可以在启动 Anda 之前导出对应的环境变量。

最小模型配置示例：

```yaml
model:
  active: "deepseek-v4-pro"
  providers:
    - family: anthropic
      model: "deepseek-v4-pro"
      api_base: "https://api.deepseek.com/anthropic"
      api_key: "YOUR_API_KEY" # 设置 DEEPSEEK_API_KEY 时可留空
      labels: ["pro", "brain"]
      disabled: false
```

支持的模型密钥环境变量包括 `OPENAI_API_KEY`、`ANTHROPIC_API_KEY`、`GEMINI_API_KEY`、`GOOGLE_API_KEY`、`DEEPSEEK_API_KEY`、`MINIMAX_API_KEY`、`MIMO_API_KEY`、`MOONSHOT_API_KEY`、`KIMI_API_KEY`、`BIGMODEL_API_KEY` 和 `GLM_API_KEY`。如果 `config.yaml` 中已经填写了 `api_key`，会优先使用配置文件里的值。

`brain` 标签表示该模型配置将优先用于记忆大脑的生成与处理。若无 provider 携带该标签，则默认使用当前激活的模型。

如果你想为不同身份或项目准备独立记忆，可以换一个 home 目录：

```bash
anda --home /path/to/.anda
```

## 交互与快捷键

终端 UI 启动后：

- Enter 发送消息。
- Shift+Enter 插入换行；如果终端不支持区分 Shift+Enter，可以用 Ctrl+J。
- 上/下方向键在多行输入中移动光标。
- Ctrl+U 清空输入。
- Ctrl+A / Ctrl+E 跳到输入开头或结尾。
- 修改 `config.yaml` 中的模型 provider 后，可以运行 `anda models reload`，或在 launcher / 浏览器侧边栏点击刷新模型。
- 修改仍需要重启的 daemon 设置后，再输入 `/reload`。
- 输入 `/stop` 停止当前任务，并让会话回到 idle 状态。
- 输入 `/cancel` 退出当前活动会话任务。
- 输入 `/steer ...` 可以给正在生成的回复追加引导。
- Esc 查看状态，Ctrl+C 退出。

成功完成的对话会在后台提交给 Anda Brain 形成长期记忆。用户不需要手动维护记忆文件。

适合长期记忆的说法：

```text
记住：我喜欢简短的发布说明，但要保留风险段落。
你还记得支付迁移项目的背景吗？
我以前默认用 provider A，现在这个 workspace 默认用 provider B。
以后我们提到 Alice，指的是移动端团队的设计师。
```

## 常用命令

运行 Anda Bot：

```bash
anda
```

把安装脚本安装的发布版更新到最新版本：

```bash
anda update
```

管理后台 daemon：

```bash
anda status
anda start
anda stop
anda restart
anda models reload
anda autostart status
```

不打开终端 UI，直接发起一次请求并等待完整结果：

```bash
anda agent run --prompt "总结一下你记得的当前项目背景"
```

启动语音对话：

```bash
anda voice --record-secs 8
```

语音模式需要 `transcription.enabled: true`。如果还想让我说出回答，需要 `tts.enabled: true`；如果只想语音输入、文字输出，可以加 `--no-playback`。

## 多场景集成

Anda Bot 既可以运行在终端中，也可以作为 Chrome 侧边栏插件使用，或通过配置 `~/.anda/config.yaml` 接入各大主流即时通讯频道。

### Chrome 侧边栏

仓库里提供了一个可直接加载的 Chrome 扩展：[chrome-extension](chrome-extension)。它会将 Anda Bot 嵌入 Chrome 的原生侧边栏（Side Panel）中，允许智能体利用专用的浏览器工具读取页面内容并管理标签页，在切换标签页时保持同一个对话会话。

侧边栏也可以收藏 assistant 消息。收藏会保存在本机 daemon 中，可以放进文件夹，并能从侧边栏或 dashboard 跳回原始对话。

为扩展生成本地 bearer token：

```bash
anda browser token --days 30
```

然后在 `chrome://extensions` 开启开发者模式，加载 [chrome-extension](chrome-extension)，把命令输出的 Gateway URL 和 token 粘贴到侧边栏设置中，就可以在任意网页里开始聊天。

### MCP 服务

Anda Bot 可以连接 MCP 服务，并把远端工具暴露给 agent。把可移植 MCP 配置放到
`~/.anda/mcp.json`，然后重启 daemon。`mcp.json` 同时支持 `mcpServers` 和
`servers` 两种 root key，方便直接粘贴其它 MCP 工具里的配置。

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

配置字符串支持 `$VAR` 和 `${VAR}` 环境变量展开。`ANDA_HOME` 和
`ANDA_WORKSPACE` 是内置变量；未配置 `cwd` 时，stdio 服务默认在第一个 Anda
workspace 中启动。

智能体也可以在对话中调用 `add_mcp_server` 连接新的 MCP 服务。`persist: false`
表示只对当前 daemon 生效；`persist: true` 会把服务写回 `~/.anda/mcp.json`，
重启后继续保留。它的服务字段与一条 `mcp.json` 配置保持一致：`type`、
`command`、`args`、`env`、`cwd`、`url`、`headers`、`enabled`、`include` 和
`exclude`，另外再加 tool 专用的 `id` 和 `persist`。

当前支持：

- Telegram
- WeChat
- Discord
- Lark / 飞书

多个可信用户可以共享同一个 daemon 和同一个 Anda agent。先创建用户 key，然后在 channel 条目的 `user` 中引用对应 id。未配置 `user` 时，channel 消息仍以操作系统安全凭证库里的本地 owner 身份运行。

```bash
anda user create alice
anda user list
```

命令会把新用户的公钥写入顶层 `users`，并把匹配的私钥保存到 `~/.anda/credentials/` 下的本地加密凭证库。凭证文件里是加密的 COSE Key，加密密钥从本地 daemon 身份密钥派生。如果明确需要文件 key，请使用下面的 `anda user export`。

在 Linux 上，如果没有可用或已解锁的 Secret Service provider，Anda 会把 daemon/owner 身份 fallback 到 `~/.anda/keys/` 下的私钥文件，并在终端和日志中显示提醒。只要 daemon 身份密钥能被加载，可信用户私钥仍会保存在本地加密凭证库中。若要为 daemon/owner 身份使用 Secret Service，请在用户 D-Bus session 中启动并解锁 provider，例如运行 `gnome-keyring-daemon --start --components=secrets`，确认 Anda 进程能拿到 `DBUS_SESSION_BUS_ADDRESS`，然后重启 Anda；KDE 用户也可以解锁 KWallet。

如果需要把已有身份私钥导出到文件，使用 `anda user export`。身份可以是 `daemon`、`owner`、`default` 或可信用户 id：

```bash
anda user export daemon --key-path ./anda-daemon.key
anda user export owner --key-path ./anda-owner.key
anda user export alice --key-path ./alice.key
```

导出的私钥文件不要放进代码仓库或共享目录。

配置仍是这样：

```yaml
users:
  - id: alice
    pubkey: "ALICE_ED25519_PUBLIC_KEY"
  - id: ops
    pubkey: "OPS_ED25519_PUBLIC_KEY"
```

Telegram 最小示例：

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

微信最小示例：

```yaml
channels:
  wechat:
    - id: personal
      user: alice
      # 可选，留空时可通过运行 anda channel init wechat 命令初始化，扫码登录获得 token
      bot_token: ""
      username: anda-wechat
      allowed_users:
        - "*"
      allow_external_users: false
      route_tag:
```

`allowed_users` 仍然用于校验平台发送者，例如 Telegram 账号、微信 `wxid`、Discord 用户 id 或 Lark open id。`user` 决定这条 channel 消息以哪个可信 Anda caller 身份创建会话、资源和记忆上下文。

设置 `allow_external_users: true` 后，非 `allowed_users` 的 IM 发送者会以 `$external_user` 身份进入对话。它们可以与机器人交互，但会被视为不可信外部用户，而不是 owner/partner。

MCP 服务参考上面的 `mcp.json` 示例；更多渠道、语音转写和 TTS 配置可以参考 [anda_bot/assets/config.yaml](anda_bot/assets/config.yaml)。

## 文件、技能与自动化

Anda Bot 的本地工作区默认位于 `~/.anda/workspace`。文件与 Shell 工具默认在该目录下执行。

用户可将自定义运行时技能放入 `~/.anda/skills`。发布版内置技能会安装到 `~/.anda/bundled-skills`，而在 `~/.agents/skills` 下的跨 Agent 技能可在 Dashboard 中导入到个人库。内置的 Cron 任务调度器支持安排未来的 Shell 任务或 Agent 提示词，并保留运行历史。

## 本地数据与隐私

默认情况下，Anda Bot 的所有状态和配置均保存在 `~/.anda` 中：

```text
~/.anda/
  config.yaml
  credentials/ # 本地加密可信用户凭证
  db/
  keys/ # 显式导出的文件 key 或 Linux Secret Service fallback key
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

记忆图谱、会话、渠道状态、定时任务、日志、个人 Skills、内置 Skills 和工作区数据都会放在这里。daemon 和 owner 身份私钥默认保存在操作系统安全凭证库，可信用户私钥保存在 `~/.anda/credentials/` 下的本地加密凭证库；显式导出的文件 key 和 Linux Secret Service fallback key 可能位于 `~/.anda/keys/`。请注意，配置的模型提供方（Model Provider）仍会接收 prompt 和记忆处理请求，建议根据数据隐私需求选择合适的提供商或部署私有大模型接口。

## 继续了解

- [Anda Bot 源码](anda_bot/README.md)
- [Anda Brain 源码](https://github.com/ldclabs/anda-brain)
- [Anda Brain 产品站](https://brain.anda.ai/)

## 许可证

项目基于 Apache-2.0 许可证发布，见 [LICENSE](LICENSE)。
