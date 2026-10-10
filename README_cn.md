# Anda Bot

[English](README.md) | [简体中文](README_cn.md)

> Born of panda. Awakened as Anda.

Anda Bot 是一个基于 Rust 编写、开源、本地运行的 AI 智能体，提供 Chrome 扩展、Electron 桌面客户端和 CLI/TUI 入口。它具备长期记忆、长程推理、本地工具调用、Subagents 协同调度等能力，并能在与用户的协作中持续学习与成长。

其核心差异在于背后的记忆引擎 [Anda Brain](https://github.com/ldclabs/anda-brain)。Anda Brain 会将对话转化为一个持续生长的认知图谱（Cognitive Nexus），包含用户、项目、偏好、事件、关系、决策以及不断演变的事实。这意味着 Anda Bot 不仅仅是检索历史文本，而是能够自主提炼有价值的知识、构建上下文、建立关联，并将有用的历史背景带入未来的对话中。

## 核心特性

- **图谱化长期记忆**：基于知识图谱记忆大脑（Anda Brain），而非零散的聊天日志。
- **自主学习与召回**：能从过去的工作中自动提炼关键信息，并在未来对话中主动召回相关背景。
- **长程推理任务**：能够执行跨越多轮对话和复杂上下文的长周期推理任务。
- **丰富的工具生态**：擅长调用外部工具（如 Claude Code、Codex）、本地 Shell、文件系统、自定义技能以及定时任务。
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

使用 `anda agent run --memory-mode no-store --prompt '…'` 或 `--memory-mode off` 新建受 Brain 策略约束的会话；聊天历史、文件和服务商处理仍保留。持久确认问题按需启用：`anda memory inbox setup` 先预览配置，再明确应用并重启。详见[使用步骤与范围](docs/brain-integration_cn.md#先使用日常记忆)。

Anda Brain 的核心设计理念是让记忆有机生长，而非简单地堆积数据。其核心循环包含三个阶段：

- **Formation（生成记忆）**：对话内容被编码为结构化知识，如实体、关系、事件、偏好和行为模式。
- **Recall（召回记忆）**：支持使用自然语言向记忆图谱提问，并获取包含丰富上下文的关联知识，而非单纯的关键词匹配。
- **Maintenance（记忆维护）**：在后台合并重复信息、巩固记忆碎片、降低过时事实的置信度，并在事实演变时保留时间线。

这为用户提供了一种自然且具有连续性的互动体验：只需告知智能体需要跨会话保留的偏好、项目背景或决策依据，在事实变化时进行纠正，或在需要时直接询问智能体“你还记得什么”。若用户的工作偏好发生演变，系统会记录并学习这种演进过程，而不是简单地覆盖历史或给出矛盾的回复。

## 快速开始

### Anda 桌面端

Anda 桌面端是本地 Anda 的聊天窗口、工作台和托盘。请从 [latest release](https://github.com/ldclabs/anda-bot/releases/latest) 下载：

- macOS Apple Silicon：`Anda-mac-arm64.dmg`
- Windows x64：`Anda-win-x64.exe`
- Homebrew：`brew install --cask ldclabs/tap/anda-desktop`（会同时安装 `anda` formula）

新的 macOS 命令行和桌面发布包仅提供 Apple Silicon 版本。Intel Mac 用户可从源码构建。

桌面端、终端和 Chrome 扩展共用同一个 `anda` 和同一个 daemon。桌面端启动时会沿用已有安装（安装脚本、`ANDA_INSTALL_DIR` 或 Homebrew 安装的 anda）；如果没有，就把内置的 `anda` 和精选技能安装到安装脚本使用的位置：macOS 为 `~/.local/bin`，Windows 为 `%LOCALAPPDATA%\Programs\AndaBot`。较新的 CLI 不会被降级，Homebrew 安装通过 `brew upgrade` 更新。模型连接引导中可以选择是否登录时启动 Anda；关闭窗口后，托盘会让服务继续运行。

尚未配置模型时，桌面端会自动打开“连接模型”引导。使用 ChatGPT 登录并确认模型，或从内置 `config.yaml` 的预置模型中选择一项、填写 API Key。Anda 保存并加载默认模型后，即可返回聊天。选择“稍后设置”后，聊天页会保留提示，也可以从**设置 → 通用 → 连接模型**重新打开。已有的模型配置会直接沿用。

桌面端托盘取代了已停用的 Anda Bot 菜单栏启动器。安装 Anda 桌面端、重新运行安装脚本或更新旧启动器时，都会移除启动器的登录项、app 包和快捷方式；如果原来启动器会在登录时启动，daemon 仍会在登录时启动。

托盘和设置中的**检查更新**会检查 `anda` 和桌面客户端两个组件。每个版本同时发布两者，桌面安装包内置同一版本的 `anda`：如果共享的 `anda` 由桌面端安装，正式签名版的桌面更新就是完整的更新，只下载一次，确认一次即可安装。服务会暂停新任务、等待活动任务结束后停止；客户端重启后用新的 `anda` 启动服务。使用 Homebrew 或在设置中指定的 `anda` 时，两者分别更新：安装 `anda` 更新会等待活动任务结束，在安装期间停止服务，完成后再启动；随后的桌面更新只重启客户端。检查失败（例如离线）时，已下载的 `anda` 版本仍保留在托盘中。托盘始终保留**检查更新**及安装入口，还可以重启服务、复制 Chrome 扩展令牌和打开日志。

正式签名版桌面客户端会在启动 1 分钟后自动检查应用更新，之后每次检查完成后间隔 6 小时再检查，即使关闭窗口或停止服务也会继续。发现新版本后，托盘和侧边栏底部会显示**下载更新**；下载完成后，同一位置改为**重启以完成更新**。没有更新或网络检查失败时保持安静。

桌面端与 Chrome 扩展共享聊天、记忆、技能、书签和配置组件。工作台支持服务端推送和持久提交回执、独立内嵌浏览器与 Agent 操作、交互终端、Git 改动/提交及可恢复工作树，并提供音频自检页。Chrome 浏览器自动化仍可使用扩展。配置管理仅限 owner，保存支持修订校验，避免覆盖其他客户端的新配置；`anda validate-config` 从 stdin 验证 YAML，不初始化 home 或 daemon。

发布环境提供签名凭证时，发布包会签名并公证；否则为未签名包，没有应用内更新通道。如需构建本机安装包，运行 `pnpm install --filter @anda/desktop...` 和 `pnpm --dir desktop package`；详见[桌面安装、开发和验证说明](desktop/README.md)与[实施与验证记录](docs/desktop-client-implementation.md)。

### 命令行

不安装桌面端，只安装最新的 `anda` 发布版：

通过 Homebrew：

```bash
brew install ldclabs/tap/anda
```

macOS 和 Linux 通过安装脚本：

```bash
curl -fsSL https://raw.githubusercontent.com/ldclabs/anda-bot/main/scripts/install.sh | sh
```

如果机器上也装过 Homebrew 版本，请打开新终端后确认 `command -v anda`
指向 `~/.local/bin/anda` 或你的 `ANDA_INSTALL_DIR`；否则较旧的 Homebrew
二进制可能会遮蔽安装脚本安装的新版本。

Windows 通过 PowerShell：

```powershell
irm https://raw.githubusercontent.com/ldclabs/anda-bot/main/scripts/install.ps1 | iex
```

安装脚本会安装 `anda` 和精选技能，将其加入 `PATH`，用 `anda autostart install`
注册 daemon 登录自启，并重启 daemon。Windows 上的登录项是当前用户的 `Run`
注册表值，登录时运行 `anda start`（会短暂出现一个控制台窗口）；Anda 桌面端的登录启动
不会出现窗口。PowerShell 安装器可以用 `-NoAutostart` 或 `-NoStart` 退出默认行为；
shell 安装器可以设置 `ANDA_NO_AUTOSTART=1` 或 `ANDA_NO_START=1`。

前置要求：

- 至少一个可用的模型提供方 API key。可以在桌面端的“连接模型”引导或**设置 → Agent 配置**中填写，
  也可以写在 `~/.anda/config.yaml`，或通过支持的环境变量提供。

也可以使用 Rust 1.95 或更新版本从源码编译运行 Anda Bot：

源码构建跟随 KIP 2.0 栈：带 Memory Interface 绑定的 Brain 0.13、Nexus/DB/KIP 0.14 及配套的 Core/Engine 0.16，保持单一 DB/core 类型身份。Brain 0.13.2 及共享栈默认从 crates.io 解析；`[patch.crates-io]` 中的注释项供同级检出开发使用。开发构建在 2.1.0 草案下写入的数据库须先迁移到新库，本构建才能打开；旧库保留只读以便回退。Brain HTTP 接受 `command` 或 `operations` 应用参数（不含 `kip` 字段），返回 KIP 2.0 信封；普通 Bot 工具保留原有 `result`/`error` 响应格式。

开发和测试构建对依赖包启用基础优化，将 macOS 展开表控制在链接器的 16 MiB 限制以内。`anda_bot` 自身仍不启用优化，并保留调试信息和 panic 展开。首次编译依赖会稍慢，后续构建会复用这些产物。

运行 `make test` 可执行启用全部 feature 的完整 Rust 测试。它默认设置 `RUST_MIN_STACK=16777216`（每个测试线程 16 MiB），与 CI 保持一致，因为内嵌 Brain 未优化的异步栈帧可能超出默认测试线程栈。显式指定的 `RUST_MIN_STACK` 可覆盖此默认值。直接调用 Cargo 时，请使用 `RUST_MIN_STACK=16777216 cargo test --workspace --all-features`。

首次使用 KIP 1.x 数据库启动时，会先迁移记忆再开放网关，可能需要数分钟。启动新 daemon 时最多等待十分钟；子进程若退出会及时报错。升级前请备份数据库；迁移后旧任务的原始状态和结果仍保存在 `LegacyRecord` 中。

可选 `mib` feature 提供仅监听本机的评测宿主：`anda mib --model-config /absolute/path/model.json --listen 127.0.0.1:8043`。入口在生产 home/daemon 初始化之前分流，使用隔离的 Brain 运行。Agent 协议执行 Bot 的 runner-managed 业务模式和 MIB 管理的任务工具；独立的记忆后端协议供 MIB 自有同模型 Agent 使用。生命周期、记忆开关和仍不完整的成本计量见 [MIB 接入](docs/mib-integration_cn.md)。

可选 MIB 控制增加强制 Recall 预算（`--recall-max-tokens` 与 `--recall-context-tokens` 同时提供）及评测侧专用的 `learning_audit`。MIB normal/ungated 条件仍需完整独立计量，持久记忆不等于已启用学习组。

```bash
git clone https://github.com/ldclabs/anda-brain.git
git clone https://github.com/ldclabs/anda-bot.git
cd anda-bot
cargo run -p anda_bot --
```

首次启动时，daemon 会自动创建 `~/.anda/config.yaml`。如果界面提示模型配置缺失，请打开该文件，填写 provider 详情，保存后在 Anda 桌面端或浏览器侧边栏点击刷新模型，或运行 `anda models reload`。对于 API Key，也可以在启动 Anda 之前导出对应的环境变量。

也可以直接使用符合条件的 ChatGPT Plus / Pro 套餐，无需 API Key：

```bash
anda auth login chatgpt
anda auth accounts
anda auth models <profile-id>
anda auth use <profile-id> <model-slug>
anda auth logout <profile-id>
```

在 Anda 桌面端选择**连接模型 → 使用 ChatGPT 登录**，然后确认所选模型。完整账号管理仍位于**设置 → Agent 配置 → 模型**；在扩展中打开配置页的“模型”分区，或侧边栏设置中的模型区域。点击 **Continue with ChatGPT**，完成浏览器授权，再选择“使用所选模型”。TUI 中可按 **Ctrl+G** 登录；CLI/TUI 默认选用账号模型目录中的首个模型，CLI 可通过 `--model <slug>` 指定。模型未配置时 daemon 只运行经过身份验证的设置服务；选择模型后才启动 Brain、渠道和 cron。三端共享同一 `ANDA_HOME` 下的账号和刷新状态，凭证加密保存于 `chatgpt/accounts.cose`，不写入配置文件或浏览器存储。

切换 ChatGPT 套餐模型需要先等待运行中的任务和子代理空闲。ChatGPT 套餐 provider 仅供本地 owner 使用；启用此类 provider 时，其他用户及外部 IM 请求会被拒绝。音视频和转录仍需独立 provider。额度耗尽不会自动切换到收费 API key，可前往 [ChatGPT 用量设置](https://chatgpt.com/settings/usage) 管理。旧版 `.codex/auth.json` 配置仍兼容，但新接入应使用上述正式登录流程。

无浏览器或 SSH 环境可运行 `anda auth login chatgpt --no-browser --port 1455`，并将浏览器所在机器的 `127.0.0.1:1455` 转发到运行 daemon 的机器。会话迁移使用 `anda auth export <profile-id> --output /absolute/path/session.json`（断开源机器），通过 SSH 传输并保持文件权限为 `0600`，在目标机器运行 `anda auth import /absolute/path/session.json`。导入后转移文件会被删除，目标机器保留自己的 host ID，使用 `anda auth use <profile-id> <model-slug>` 激活模型。不要让两台机器同时刷新同一份会话。

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

`image`、`audio`、`video` 标签指定理解对应附件的 provider。当前激活的模型也带有 `image` 标签时，消息中 5 MiB 以内的 PNG、JPEG、GIF 和 WebP 图片会直接随消息发给它，在该消息开启的任务内有效；不能读图的模型不要加这个标签。

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
- 修改 `config.yaml` 中的模型 provider 后，可以运行 `anda models reload`，或在 Anda 桌面端 / 浏览器侧边栏点击刷新模型。Brain 会继续使用启动时的模型，直到 daemon 重启。
- 修改仍需要重启的 daemon 设置后，再输入 `/reload`。
- 输入 `/stop` 打断当前任务，取消其后台任务和待审批动作，清除活动目标，并让会话回到 idle 状态以接收下一条消息。
- 输入 `/cancel` 停止上述工作，并关闭当前活动会话。
- 输入 `/steer ...` 可以给正在生成的回复追加引导。
- Esc 查看状态，Ctrl+C 退出。

输入会保留文本和代码中的空格，光标移动与删除会将组合 emoji 作为一个字符处理。状态检查、对话轮询和 Brain 请求在后台进行，响应较慢时仍可操作界面并按 Ctrl+C 退出。启动界面会保留已有的终端滚动历史。`/new` 会清空屏幕和滚动历史后开始新对话。重新连接会保留输入草稿，只有恢复到进行中的对话时才替换已显示的记录。在支持 [Program Status Protocol](https://www.superlogical.com/rex/docs/build/program-status)（OSC 7501）的终端中，标签页或会话列表会显示 Anda 正在工作、等待你的审批、回答或登录、已完成还是失败；其他终端会忽略它。

### 命令审批

有风险的 Shell 命令、MCP 服务连接，以及需要你同意的 MCP 工具调用，会先弹出审批卡片：

- 输入框为空时，按 `y` 批准，按 `n` 拒绝。MCP 工具的审批卡片还提供 `a`（始终允许）：批准本次调用，之后不再询问这个工具。
- 输入框中已有内容时，这两个按键会被当作普通输入。此时输入 `y`/`yes` 或 `n`/`no` 再按 Enter 即可回应，也可以按 Ctrl+U 清空输入后继续用单键快捷方式。底部状态栏会提示当前可用的是哪一种。
- 审批卡片 10 分钟后过期，对应的工具调用随之失败。

审批卡片会按终端宽度换行，完整展示命令、详情和选项；命令指定了工作目录时卡片会一并显示，目录在当前工作区之外则总是需要审批。状态变化会追加到对话记录。提交错误优先于快捷键帮助显示；文字选项提交失败后会保留草稿，方便修改和重试。

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

Homebrew 安装会拒绝 `anda update`，请使用 `brew upgrade anda`。Anda 桌面端的托盘和设置页执行的也是同一个更新。

把当前 `anda` 安装到共享的 CLI 位置（Anda 桌面端就是这样安装内置副本的；不会降级，`--dir` 可指定其他目录）：

```bash
anda install
```

本地构建版本较新时，会保留现有程序和内置技能。使用 `anda update --force` 可明确安装最新发布版，即使它比本地版本旧。此规则也适用于 `anda update --skills`。

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

语音模式需要 `transcription.enabled: true`。如果还想让我说出回答，需要 `tts.enabled: true`，并且 PATH 中有音频播放器（`ffplay`、macOS 的 `afplay` 或 SoX 的 `play`），首次录音前会检查；如果只想语音输入、文字输出，可以加 `--no-playback`。 语音模式会等本轮答复完成后再开始下一轮录音；某一轮失败时会提示错误，然后继续下一轮录音。Ctrl-C 也可停止语音合成和播放。

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
`~/.anda/mcp.json`，然后运行 `anda mcp reload`（或重启 daemon）。`mcp.json` 同时支持 `mcpServers` 和
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
        "Authorization": "Bearer ${secret:MCP_REMOTE_TOKEN}"
      }
    }
  }
}
```

配置字符串支持 `$VAR` 和 `${VAR}` 环境变量展开，变量可能未设置时可写
`${VAR:-default}`。`ANDA_HOME` 和 `ANDA_WORKSPACE` 是内置变量；未配置 `cwd` 时，
stdio 服务默认在第一个 Anda workspace 中启动。`${secret:NAME}` 展开为保存在
`mcp.json` 之外的密钥，存放在仅所有者可读的 `~/.anda/mcp_secrets.json` 中；用
`anda mcp secret set NAME`（不回显地读取输入，也可以从管道读取）或在 MCP 页面设置。
引用的密钥未设置时，该服务会被跳过，设置后自动启动；删除服务时，只有它使用的密钥也会
一并删除。

每个条目单独加载，MCP 不会阻止 daemon 启动：无法使用的条目（环境变量未设置、
不支持的 `type`，如已弃用的 `sse`、重复的 id）会被跳过，并在 daemon 日志中记录
警告。服务在 daemon 启动后于后台连接；如需在 daemon 报告就绪前完成工具发现，
可为条目设置 `"startup": "eager"`。stdio 服务与 shell 工具使用相同的扩展
`PATH`，即使 daemon 作为登录服务运行也能找到 `npx` 和 `uvx`；登录服务看不到
shell 配置文件中导出的变量。

条目还可以调整服务的运行方式：`timeouts`，单位为秒（`setup_secs` 默认 90、
`list_secs` 30、`request_secs` 180、`call_secs` 600）；`concurrency`（默认
`serial`，可选 `read_only_parallel` 或 `parallel`）；`limits.output_text_bytes`
（智能体从一次结果中得到的文本，默认 32768）；`lifecycle`、`tasks`，以及本地服务的
`inherit_env`。本地服务默认获得 daemon 的全部环境变量；设为 `"inherit_env": false`
后，只获得平台必需的变量（`PATH`、`HOME` 等）和它自己的 `env`。可以在 MCP 页面或用
`anda mcp options` 修改。

在终端中用 `anda mcp` 管理服务：

```bash
anda mcp list                                         # 每个服务的状态、工具数和最近错误
anda mcp add context7 -- npx -y @upstash/context7-mcp
anda mcp add docs --url https://docs.example.com/mcp --header 'Authorization: Bearer ${DOCS_TOKEN}'
anda mcp add-json notes '{"type":"http","url":"https://notes.example.com/mcp"}'
anda mcp login linear --url https://mcp.linear.app/mcp  # OAuth 登录
anda mcp tool github delete_repository --hide         # 不让智能体使用某个工具
anda mcp secret set MCP_REMOTE_TOKEN                  # 另有 secret list、secret unset
anda mcp disable context7                             # 另有 enable、remove、reconnect、logout
anda mcp options github --call-timeout 900            # 高级设置；用 `default` 清除某项
anda mcp import --dry-run                             # 其他应用里配置的服务
anda mcp import github notes=work-notes --from cursor # 导入它们；不写名称则导入全部新服务
```

Anda Desktop 和扩展仪表盘（`#mcp`）提供 MCP 页面，功能与 CLI 相同：每个服务的状态、
最近错误和说明，工具及其审批策略和审查后的变化，登录，密钥（只写：保存后不再显示），
高级设置，以及“添加”菜单：填 URL、命令，或粘贴其他应用的 JSON，保存前先测试连接，
并把其中的令牌存为密钥；从其他应用导入；浏览官方 MCP Registry。在 Anda Desktop 中，
`anda://mcp/install?name=<id>&config=<base64url JSON>` 链接会打开“添加”对话框，
填入该配置并提示你先检查；在你点击添加之前不会保存任何内容。

`anda mcp import` 和页面上的导入会读取 Claude Desktop、Claude Code（`~/.claude.json`）、
Cursor、VS Code（以及 GitHub Copilot 的 CLI）、Windsurf 和 Codex（`~/.codex/config.toml`）
的 MCP 设置，以及 Claude Code 记录的项目、当前目录或 daemon 工作区中的项目文件（`.mcp.json`、
`.cursor/mcp.json`、`.vscode/mcp.json`）。这些文件只会被读取。
每个服务都会显示导入后的结果：名称已被占用的会换一个名称导入，Anda 已经在运行的会被
略过。导入时，请求头的值、令牌和密钥会转存为密钥（`--keep-plaintext` 则保留在
`mcp.json` 中）；VS Code 的输入项和 daemon 没有的环境变量会变成待设置的密钥，导入时会
询问它们的值；本地服务会设为 `"inherit_env": false`。

MCP Registry 由 daemon 搜索，使用 daemon 的代理设置。安装服务时默认写入它的远程端点，
因为这不会在你的电脑上运行任何程序；也可以选择它的 npm、PyPI、OCI（Docker）或 NuGet
包，作为本地命令运行，且不获得 daemon 的全部环境变量。你为密钥字段填写的值会存入密钥库。
Registry 只说明服务已经发布，并不代表它安全：在你放行之前，它的工具运行前仍会先询问。

daemon 运行时，修改立即生效，连接失败的服务会在后台重试；daemon 未运行时，命令
只修改 `mcp.json`，在 daemon 启动时生效。`list` 也会列出被跳过的条目及原因。修改
会保留 Anda 不读取的字段和 `${VAR}` 引用；`remove` 还会删除该服务已保存的登录凭据。

每次调用 MCP 工具都要经过审批检查。在默认的 `auto` 策略下，没有完全访问权限的会话
调用服务未标为只读的工具前会先询问；定时任务和目标模式以完全访问权限运行。可以为
服务或其中某个工具设置策略：

```bash
anda mcp approval github allow                          # github 的工具都不再询问
anda mcp approval github ask --tool merge_pull_request  # 总是询问，即使拥有完全访问权限
anda mcp review github                                  # 确认审查后发生变化的工具
```

在 MCP 工具的审批卡片上选择“始终允许”，会把该工具设为 `allow`。
Anda 会记住服务第一次提供的每个工具定义：之后发生变化的工具或新增的工具，即使在
`allow` 下也会重新询问，直到你批准它的一次调用，或用 `anda mcp review` 确认
（`anda mcp diff <id> <tool>` 显示具体变化）。服务给模型的说明也会这样记录，
变化后 MCP 页面会提示。外部 IM 用户的请求只有在执行
`anda mcp external-users <id> on` 之后才能使用该服务，而且不能调用需要审批的工具。
为其他智能体编写的技能可以用 `mcp__<server>__<tool>` 指代工具。

支持 MCP Events 的服务会上报事件，例如新的 issue 或新的评论。自动化会在事件到来时
按你的指令运行智能体，回复出现在你的一个对话中，或回到创建它的 IM 聊天里。可以在
MCP 页面服务的“事件”标签页、用 `anda mcp triggers add`，或让智能体
（`create_event_trigger`，会先请求你的批准）创建自动化：

```bash
anda mcp events github                                # 服务上报哪些事件
anda mcp triggers add github issue.opened --args '{"repo":"owner/name"}' \
    --instructions "给每个新 issue 加标签并写一段摘要"
anda mcp triggers                                     # 另有：get、pause、resume、delete
```

在自动化的批处理窗口（默认 30 秒）内到达的事件会合并到一次运行中处理。事件数据来自
服务，属于不可信内容，所以这些运行不像定时任务那样拥有完全访问权限：只能使用只读工具
和设为 `allow` 的工具，需要审批的操作一律拒绝。运行次数超过每小时上限（默认 12 次）
或连续失败五次的自动化会自行暂停。Anda 通过推送或轮询接收事件。只通过 webhook 投递
的事件需要 dMsg 代为接收：在 `mcp.json` 中加入 dMsg 并设置
`"events": {"webhook_ingress": true}`，Anda 会为每个这样的自动化在 dMsg 上创建端点，
并让服务向它投递。

智能体也可以在对话中调用 `add_mcp_server` 连接新的 MCP 服务。`persist: false`
表示只对当前 daemon 生效；`persist: true` 会把服务写回 `~/.anda/mcp.json`，
重启后继续保留。它的服务字段与一条 `mcp.json` 配置保持一致：`type`、
`command`、`args`、`env`、`cwd`、`url`、`headers`、`enabled`、`include` 和
`exclude`，另外再加 tool 专用的 `id` 和 `persist`。`manage_mcp_server` 让智能体
查看各服务的状态和最近错误，并重连某个服务；启用、停用、删除或登出服务前会先请求
你的批准。

OAuth 服务可通过 `connect_mcp_server` 完成授权并保存连接。重新授权会保留工具允许/排除列表、自定义 headers、client ID 和传输设置，并保存更新后的 scopes。浏览器启动器失败时，工具会返回授权 URL，供手动打开。回调使用网关端口及相同 IP 地址族的回环地址；远程或绑定特定网卡的部署，需要将浏览器侧的该回环地址隧道转发到实际监听地址。

当前支持：

- Telegram
- WeChat
- Discord
- Lark / 飞书

多个可信用户可以共享同一个 daemon 和同一个 Anda agent。先创建用户 key，然后在 channel 条目的 `user` 中引用对应 id。未配置 `user` 时，channel 消息仍以操作系统安全凭证库里的本地 owner 身份运行。daemon 管理操作（修改配置、重载与切换模型、安装更新、关停）仍只允许本地 owner 执行。

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

频道投递保留 Telegram topic、Discord 线程和 Lark/飞书回复线程。文本分段与附件分别重试，不重发已成功投递的部分。`https_proxy` 同时覆盖 WebSocket 流量；微信不需要代理，不经过它。Lark/飞书对 HTTP(S) 附件发送链接，本地或二进制附件则上传为图片或文件消息。备份数据库时请同时保留频道工作区中的附件文件。

设置 `allow_external_users: true` 后，非 `allowed_users` 的 IM 发送者会以 `$external_user` 身份进入对话。它们可以与机器人交互，但会被视为不可信外部用户，而不是 owner/partner。

MCP 服务参考上面的 `mcp.json` 示例；更多渠道、语音转写和 TTS 配置可以参考 [anda_bot/assets/config.yaml](anda_bot/assets/config.yaml)。

## 文件、技能与自动化

Anda Bot 的本地工作区默认位于 `~/.anda/workspace`。文件与 Shell 工具默认在该目录下执行。Shell 命令在短暂的前台等待（默认 10 秒，最多 30 秒）后仍未结束时，会转入后台继续运行，最长 24 小时：输出会回报到对话中，agent 可以用 `shell_session` 查询或停止它，`/stop` 会取消它。交互式 CLI 连接时，会使用本机 owner 身份向 daemon 注册启动目录；该 CLI 会话的原生 Shell 命令从已注册的目录运行。进入 Chrome 扩展或 Anda 桌面端中的文件目录会话时，客户端也会注册该目录，此会话的 Shell 命令从该目录启动。daemon 只在内存中保留这些注册，最长 24 小时，重启后即失效，因此各客户端每次发送消息时都会重新注册该目录。其他来源不能只靠请求元数据指定任意 Shell 目录。文件工具仍只能访问已配置的工作区。附件理解也可读取 owner 已注册且授权尚未过期的目录；请求元数据本身不能授权新目录。

单次请求可使用 `anda agent run --workspace . --prompt "总结这个项目"`，CLI 会在提交提示词前注册目录，并默认使用对应的 `cli:<path>` source。`--workspace` 的相对路径基于 CLI 当前目录解析。`--meta` 中显式指定的 `source`、`workspace` 仍然优先；文件工具仍受已配置工作区限制。

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
