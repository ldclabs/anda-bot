# anda-bot

[English](README.md) | [简体中文](README_CN.md)

anda-bot 是一个拥有长期记忆、可调用本地工具并具备自我进化能力的 AI 智能体，底层由 ANDA Hippocampus 驱动。

当前这个仓库通过 [anda_bot](anda_bot/README.md) 包来暴露该智能体，并产出 `anda` 二进制。

## 项目概览

这个项目把几类能力合并成一个本地运行时：

- 终端内联聊天 UI。
- 本地守护进程与 HTTP 网关。
- 基于 ANDA Hippocampus 的长期记忆服务。
- 基于 anda_engine 的工具化 Agent 运行时。
- 基于 AndaDB 的持久化会话、频道消息与定时任务存储。
- 可选的 IRC 接入与回发。
- 可持久化的 cron 调度器，可执行 shell 任务或 agent prompt。

守护进程会在同一个本地地址上合并两套 HTTP 接口：

- `/engine/{id}`：Agent 运行时接口。
- `/v1/anda_bot/...`：Hippocampus 记忆与空间管理接口。

## 快速开始

前置要求：

- 较新的 Rust 工具链。
- 至少一个可用的模型提供方 API Key。

启动 TUI：

```bash
cargo run -p anda_bot --
```

前台启动守护进程：

```bash
cargo run -p anda_bot -- daemon
```

在 Unix 平台停止或重启后台守护进程：

```bash
cargo run -p anda_bot -- stop
cargo run -p anda_bot -- restart
```

运行时 home 目录默认是 `~/.anda`。可以用 `--home` 覆盖：

```bash
cargo run -p anda_bot -- --home /path/to/.anda
```

## 首次运行行为

第一次启动时，如果缺少 home 目录或配置文件，程序会自动创建，并在 `~/.anda/config.yaml` 写入一份起始模板。

如果当前激活的模型提供方配置不完整，TUI 会停留在 setup 模式并直接列出缺失字段。此时编辑配置文件、保存，然后回到 TUI 按 Enter 重新加载即可。

运行时还会按需创建这些子目录：

- `keys/`：存放 daemon 身份密钥和本地用户密钥。
- `db/`：存放对象存储与数据库状态。
- `logs/`：存放后台 daemon 日志。
- `skills/`：存放运行时加载的技能文件。
- `sandbox/`：在启用沙箱时用于 shell 执行隔离。

## 配置模型

标准模板位于 [anda_bot/assets/config.yaml](anda_bot/assets/config.yaml)。

核心字段如下：

```yaml
addr: 127.0.0.1:8042
sandbox: false
# https_proxy: http://127.0.0.1:7890

model:
	active: DeepSeek
	providers:
		DeepSeek:
			family: anthropic
			model: "deepseek-v4-pro"
			api_base: "https://api.deepseek.com/anthropic"
			api_key: "..."

channels:
	irc: []
	telegram: []
	discord: []
	lark: []
```

激活模型必须能解析到一个未禁用的 provider，并且至少补齐 `family`、`model`、`api_base`、`api_key`。

IRC、Telegram、Discord、Lark/飞书配置都是可选的。启用后，频道消息或私聊会被转换成 agent prompt，对应回复会回发到原来的频道、目标用户或线程。Lark 配置需要 `app_id` 和 `app_secret`，飞书可设置 `platform: feishu`。

## 架构说明

主要实现入口：

- [anda_bot/src/main.rs](anda_bot/src/main.rs)：CLI 入口与命令分发。
- [anda_bot/src/daemon.rs](anda_bot/src/daemon.rs)：运行目录管理、密钥加载、本地数据库启动与服务编排。
- [anda_bot/src/tui](anda_bot/src/tui)：终端内联聊天 UI。
- [anda_bot/src/engine](anda_bot/src/engine)：Agent 运行时、工具注册与会话接口。
- [anda_bot/src/brain](anda_bot/src/brain)：Hippocampus 的 formation / recall 集成。
- [anda_bot/src/channel](anda_bot/src/channel)：IRC、Telegram、Discord、Lark/飞书接入、路由、重试与回发。
- [anda_bot/src/cron](anda_bot/src/cron)：持久化调度器、任务存储与执行历史。
- [anda_bot/src/gateway](anda_bot/src/gateway)：本地 HTTP API 与客户端。

一些重要的运行时行为：

- 会话历史会被持久化，并通过专门的 conversations 工具供 TUI 轮询。
- 引擎会注册长期记忆检索、shell、note、todo、文件读写搜索编辑、skills、cron 等工具。
- 文件工具总是相对 `anda` 启动时的当前工作目录执行。
- shell 默认也在当前工作目录执行；如果 `sandbox: true`，则切换到本地 sandbox 运行时。
- cron 任务既可以执行 shell 命令，也可以把 prompt 再提交给 agent，并保留运行历史。

## 仓库结构

```text
.
├── Cargo.toml
├── README.md
├── README_CN.md
└── anda_bot/
		├── Cargo.toml
		├── README.md
		├── assets/
		└── src/
```

如果你想看更具体的子项目使用说明、配置示例和 TUI 交互细节，请直接阅读 [anda_bot/README.md](anda_bot/README.md)。

## 许可证

项目基于 Apache-2.0 许可证发布，见 [LICENSE](LICENSE)。
