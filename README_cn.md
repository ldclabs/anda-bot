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

先运行 `anda memory guide` 阅读离线指南，再在普通聊天中告诉 Anda 一条偏好。等后台整理后，用 `/new` 开新对话询问。`anda memory` 和 TUI `/memory` 无需调用模型即可查看连接与处理状态；浏览器“记忆”工作区提供记录、可核对的来源、更正及移除预览。助手的口头确认不等于事实已保存。

整理记录按提交时间排序；记忆记录和生效中的订阅支持连续分页，已取消的订阅不占用活动列表配额。Recall 请求收到 HTTP 错误后不会自动重发。

使用 `anda agent run --memory-mode no-store --prompt '…'` 或 `--memory-mode off` 新建受 Brain/Notes 策略约束的会话；聊天历史、文件和服务商处理仍保留。持久确认问题按需启用：`anda memory inbox setup` 先预览配置，再明确应用并重启。详见[使用步骤与范围](docs/brain-integration_cn.md#先使用日常记忆)。

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

如果机器上也装过 Homebrew 版本，请打开新终端后确认 `command -v anda`
指向 `~/.local/bin/anda` 或你的 `ANDA_INSTALL_DIR`；否则较旧的 Homebrew
二进制可能会遮蔽安装脚本安装的新版本。

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

使用 `anda_launcher --home <目录>`（或为 launcher 设置 `ANDA_HOME`）可指定状态目录。
launcher 会将该目录传给 daemon 命令，并保留在登录自启和重启入口中。模型设置只更新
选中的模型，保留其他 provider 和注释，保存前会验证配置。macOS 登录自启开关从下次
登录生效，当前 launcher 会继续运行。托盘状态在空闲时每分钟刷新一次，打开菜单或
完成 daemon 操作时也会异步刷新。

前置要求：

- 至少一个可用的模型提供方 API key。Windows 安装器用户可以在 GUI 向导中填写；CLI 用户可以写在 `~/.anda/config.yaml`，也可以通过支持的环境变量提供。

也可以使用 Rust 1.95 或更新版本从源码编译运行 Anda Bot：

源码构建跟随 KIP 2.0 栈：带 Memory Interface 绑定的 Brain 0.13、Nexus/DB/KIP 0.14 及配套的 Core/Engine 0.16，保持单一 DB/core 类型身份。Brain 0.13 发布之前，`[patch.crates-io]` 指向同级的 `anda-brain` 检出。开发构建在 2.1.0 草案下写入的数据库须先迁移到新库，本构建才能打开；旧库保留只读以便回退。Brain HTTP 接受 `command` 或 `operations` 应用参数（不含 `kip` 字段），返回 KIP 2.0 信封；普通 Bot 工具保留原有 `result`/`error` 响应格式。

开发和测试构建对依赖包启用基础优化，将 macOS 展开表控制在链接器的 16 MiB 限制以内。`anda_bot` 自身仍不启用优化，并保留调试信息和 panic 展开。首次编译依赖会稍慢，后续构建会复用这些产物。

首次使用 KIP 1.x 数据库启动时，会先迁移记忆再开放网关，可能需要数分钟。启动新 daemon 时最多等待十分钟；子进程若退出会及时报错。升级前请备份数据库；迁移后旧任务的原始状态和结果仍保存在 `LegacyRecord` 中。

可选 `mib` feature 提供仅监听本机的评测宿主：`anda mib --model-config /absolute/path/model.json --listen 127.0.0.1:8043`。入口在生产 home/daemon 初始化之前分流，使用隔离的 Brain 运行。Agent 协议执行 Bot 的 runner-managed 业务模式和 MIB 管理的任务工具；独立的记忆后端协议供 MIB 自有同模型 Agent 使用。生命周期、记忆开关和仍不完整的成本计量见 [MIB 接入](docs/mib-integration_cn.md)。

可选 MIB 控制增加强制 Recall 预算（`--recall-max-tokens` 与 `--recall-context-tokens` 同时提供）及评测侧专用的 `learning_audit`。MIB normal/ungated 条件仍需完整独立计量，持久记忆不等于已启用学习组。

```bash
git clone https://github.com/ldclabs/anda-brain.git
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

支持的模型密钥环境变量包括 `OPENAI_API_KEY`、`ANTHROPIC_API_KEY`、`GEMINI_API_KEY`、`GOOGLE_API_KEY`、`DEEPSEEK_API_KEY`、`MINIMAX_API_KEY`、`MIMO_API_KEY`、`MOONSHOT_API_KEY`、`KIMI_API_KEY`、`BIGMODEL_API_KEY` 和 `GLM_API_KEY`。如果 `config.yaml` 中已经填写了 `api_key`，会优先使用配置文件里的值。已识别的 API 地址会优先选择其服务商密钥，再考虑模型名称：OpenRouter、Groq 和 SiliconFlow 分别使用 `OPENROUTER_API_KEY`、`GROQ_API_KEY` 和 `SILICONFLOW_API_KEY`，即使托管的是其他厂商的模型。

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
- 输入 `/stop` 打断当前任务，取消其后台任务和待审批动作，清除活动目标，并让会话回到 idle 状态以接收下一条消息。
- 输入 `/cancel` 停止上述工作，并关闭当前活动会话。
- 输入 `/steer ...` 可以给正在生成的回复追加引导。
- Esc 查看状态，Ctrl+C 退出。

输入会保留文本和代码中的空格，光标移动与删除会将组合 emoji 作为一个字符处理。状态检查、对话轮询和 Brain 请求在后台进行，响应较慢时仍可操作界面并按 Ctrl+C 退出。启动界面会保留已有的终端滚动历史。

### 命令审批

有风险的 Shell 命令和 MCP 服务连接会先弹出审批卡片：

- 输入框为空时，按 `y` 批准，按 `n` 拒绝。
- 输入框中已有内容时，这两个按键会被当作普通输入。此时输入 `y`/`yes` 或 `n`/`no` 再按 Enter 即可回应，也可以按 Ctrl+U 清空输入后继续用单键快捷方式。底部状态栏会提示当前可用的是哪一种。
- 审批卡片 10 分钟后过期，对应的工具调用随之失败。

审批卡片会按终端宽度换行，完整展示命令、详情和选项；状态变化会追加到对话记录。提交错误优先于快捷键帮助显示；文字选项提交失败后会保留草稿，方便修改和重试。

用 `anda --full-access` 启动终端 UI，可以让本次会话跳过审批卡片；开启后状态行会显示 `full-access`。

Cron 定时任务和自主目标模式（`/goal ...`）始终以完全访问权限运行：这些场景没有人在旁边回应卡片，否则任务只能等到卡片过期后失败。 定时任务会等待 Agent 或 Shell 命令实际结束才记录完成，修改计划会保留暂停状态。Cron 工具仅供可信用户使用；从 CLI 创建的任务会保存创建时验证过的工作目录授权，daemon 重启后仍然有效。

成功完成的对话会在后台提交给 Anda Brain 形成长期记忆。用户不需要手动维护记忆文件。Formation 接受 ID 与 Recall 交付收据会持久保留，接受、完成、实际使用分别记录。

可选 `brain.runtime_config`（环境变量 `BRAIN_RUNTIME_CONFIG` 优先）在加载 Space 前安装原生持久待办。浏览器 Brain 页面及 TUI `/brain inbox`、`/brain status` 提供调用者隔离的入口。`learning` 是独立 Cargo feature，语义求值、utility、trust、自动学习均需显式配置及原生授权；现有 IM 发送接口未作为自动动作适配器。配置、收据语义及业务合同见 [Brain 运行时集成](docs/brain-integration_cn.md)。

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

本地构建版本较新时，会保留现有程序、启动器和内置技能。使用 `anda update --force` 可明确安装最新发布版，即使它比本地版本旧。此规则也适用于 `anda update --skills`。

管理后台 daemon：

```bash
anda status
anda start
anda stop
anda restart
anda models reload
anda autostart status
```

自启动注册会保存绝对 home 路径，包括通过相对 `--home` 指定的目录；注册失败会返回错误。指定具体 `addr` 时，CLI 和 Brain 内部连接也使用该地址，只有通配监听地址（`0.0.0.0` 或 `::`）会替换为回环地址。

不打开终端 UI，直接发起一次请求并等待完整结果：

```bash
anda agent run --prompt "总结一下你记得的当前项目背景"
```

`--prompt` 与 `--prompt-file` 必须且只能提供一个。添加 `--wait-timeout-secs 120` 可限制提交请求与等待完成的总时间，默认 `0` 表示不设总超时。超时只停止 CLI 等待，daemon 中的任务可能继续运行。

使用 `anda memory inbox` 读取记忆待办。如果还有下一页，运行输出提示中的 `anda memory inbox --cursor <next_cursor>`；`--json` 在 `result.next_cursor` 中返回游标。

启动语音对话：

```bash
anda voice --record-secs 8
```

语音模式需要 `transcription.enabled: true`。如果还想让我说出回答，需要 `tts.enabled: true`；如果只想语音输入、文字输出，可以加 `--no-playback`。 语音模式会等本轮答复完成后再开始下一轮录音。Ctrl-C 也可停止语音合成和播放。

Google 转写接受不超过 60 秒的 WAV/FLAC 录音，Chrome 扩展会将其他录音格式转换为 WAV。`transcription.initial_prompt` 为 Groq/OpenAI Whisper 提供词汇提示。Edge 合成需要 PATH 中存在 `edge-tts`；`tts.default_format` 仅控制 StepFun，Edge/OpenAI/Google 输出 MP3。详见[语音服务限制与配置](docsite/i18n/zh-Hans/docusaurus-plugin-content-docs/current/runtime/configuration.mdx#语音服务)。

## 多场景集成

Anda Bot 既可以运行在终端中，也可以作为 Chrome 侧边栏插件使用，或通过配置 `~/.anda/config.yaml` 接入各大主流即时通讯频道。

### Chrome 侧边栏

仓库里提供了一个可直接加载的 Chrome 扩展：[chrome-extension](chrome-extension)。它会将 Anda Bot 嵌入 Chrome 的原生侧边栏（Side Panel）中，允许智能体利用专用的浏览器工具读取页面内容并管理标签页，在切换标签页时保持同一个对话会话。

侧边栏与 Dashboard 同步连接设置；切换 daemon 时会清理旧会话和资源缓存。Dashboard 按需加载各工作区。语音录制始终绑定开始录音的标签页。`open_download` 会在文件夹中显示下载文件，由用户点击打开文件。

侧边栏也可以收藏 assistant 消息。收藏会保存在本机 daemon 中，可以放进文件夹，并能从侧边栏或 dashboard 跳回原始对话。

为扩展生成本地 bearer token：

```bash
anda browser token --days 30
```

然后在 `chrome://extensions` 开启开发者模式，加载 [chrome-extension](chrome-extension)，把命令输出的 Gateway URL 和 token 粘贴到侧边栏设置中，就可以在任意网页里开始聊天。

daemon 管理接口要求已配置的可信身份，未登记密钥的有效签名不授予访问权限。浏览器连接会对后续请求重新检查凭据有效期，过期后请生成新 token 并更新扩展设置。会话来源列表和绑定删除仅限调用者自己的会话。

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

OAuth 服务可通过 `connect_mcp_server` 完成授权并保存连接。重新授权会保留工具允许/排除列表、自定义 headers、client ID 和传输设置，并保存更新后的 scopes。浏览器启动器失败时，工具会返回授权 URL，供手动打开。回调使用网关端口及相同 IP 地址族的回环地址；远程或绑定特定网卡的部署，需要将浏览器侧的该回环地址隧道转发到实际监听地址。

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

在 Linux 上，如果没有可用或已解锁的 Secret Service provider，Anda 可以读取**已有的** daemon/owner 私钥文件 `~/.anda/keys/anda_bot.key` 和 `~/.anda/keys/user.key`，并在终端和日志中显示提醒。两个文件必须完整存在：凭据库不可用时不会生成替代身份。已有安装应解锁 provider 或恢复原始密钥；可信用户私钥仍使用从原始 daemon 身份派生的密钥加密保存。

对于**没有操作系统凭据库的全新安装**，请先显式运行 `anda user init-file-keys`，再运行 `anda start`。该命令创建缺失的私钥文件，不覆盖已有文件；若已有本地数据库/凭据数据，或能读取到 keyring 中的身份，则拒绝初始化。不要用它恢复已有安装。导入的 COSE 密钥必须标明 Ed25519 曲线；算法字段存在时必须为 EdDSA，私钥附带的公钥必须与其匹配。原始 32 字节 Base64 密钥继续受支持。

若要使用 Secret Service，请在用户 D-Bus session 中启动并解锁 provider，例如运行 `gnome-keyring-daemon --start --components=secrets`，确认 Anda 能拿到 `DBUS_SESSION_BUS_ADDRESS`，然后重启；KDE 用户也可以解锁 KWallet。已有文件身份会一起迁移到一个 keyring bundle，只有完整保存成功后才删除源文件。

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

频道投递保留 Telegram topic、Discord 线程和 Lark/飞书回复线程。文本分段与附件分别重试，不重发已成功投递的部分。`https_proxy` 同时覆盖 WebSocket 和微信流量。Lark/飞书目前发送附件 HTTP(S) 链接，对本地或二进制附件明确返回不支持。备份数据库时请同时保留频道工作区中的附件文件。

设置 `allow_external_users: true` 后，非 `allowed_users` 的 IM 发送者会以 `$external_user` 身份进入对话。它们可以与机器人交互，但会被视为不可信外部用户，而不是 owner/partner。

MCP 服务参考上面的 `mcp.json` 示例；更多渠道、语音转写和 TTS 配置可以参考 [anda_bot/assets/config.yaml](anda_bot/assets/config.yaml)。

## 文件、技能与自动化

Anda Bot 的本地工作区默认位于 `~/.anda/workspace`。文件与 Shell 工具默认在该目录下执行。交互式 CLI 连接时，会使用本机 owner 身份向 daemon 注册启动目录；该 CLI 会话的原生 Shell 命令从已注册的目录运行。进入 Chrome 扩展中的文件目录频道时，扩展也会注册该目录，此频道的 Shell 命令从该目录启动。其他来源不能只靠请求元数据指定任意 Shell 目录。文件工具仍只能访问已配置的工作区。附件理解也可读取 owner 已注册且授权尚未过期的目录；请求元数据本身不能授权新目录。

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
