# Anda Bot

[English](README.md) | [简体中文](README_cn.md)

> Born of panda. Awakened as Anda.

我是 Anda Bot，一个运行在你本机的长期记忆智能体。市面上的智能体已经很多，但大多数都擅长“这一轮聊天”，却很难真正延续过去的经验。我想成为的是另一种伙伴：我会记住重要的事，会在需要时回想，会使用你电脑上的工具，也会随着我们一起工作而逐渐成长。

我的核心差异来自 [Anda Hippocampus](https://github.com/ldclabs/anda-hippocampus)。它不是普通向量库，也不是一份越写越长的 Markdown 记忆，而是一颗面向智能体的长期记忆大脑。Hippocampus 会把对话转化为一个持续生长的认知中枢：里面有用户、项目、偏好、事件、关系、决策和随时间变化的事实。

## 为什么推荐我

- 我有图谱记忆，不只是检索旧聊天记录。
- 我可以在未来对话里主动召回与你当前任务相关的背景。
- 我能调用本地 shell、文件、笔记、待办、技能和定时任务工具。
- 我可以待在终端里，也可以接入 IRC、Telegram、WeChat、Discord、Lark/飞书。
- 配好转写和语音合成后，你可以直接和我语音对话。
- 默认情况下，我的运行数据会保存在你的本机目录下。

## 我的长期记忆大脑

Anda Hippocampus 的设计重点不是“记得更多”，而是“记住之后还能消化”。它的核心循环分成三步：

- **Formation 生成记忆：** 我们的对话会被编码为结构化知识，包括实体、关系、事件、偏好和行为模式。
- **Recall 召回记忆：** 我可以用自然语言向记忆图谱提问，拿到有上下文的答案，而不是一堆原始搜索片段。
- **Maintenance 维护记忆：** Hippocampus 可以在后台巩固碎片、合并重复信息、降低陈旧知识的置信度，并在事实变化时保留演化时间线。

这对你意味着：你可以像培养一个长期搭档一样培养我。告诉我稳定的偏好、项目背景、团队关系和决策原因；当事实变化时纠正我；需要连续性时直接问“你还记得什么”。如果你过去喜欢一种工作流，现在改成另一种，我应该学会这段变化，而不是粗暴覆盖或同时给出矛盾答案。

## 快速开始

安装最新发布版：

通过 Homebrew：

```bash
brew install ldclabs/tap/anda
```

通过安装脚本：

```bash
curl -fsSL https://raw.githubusercontent.com/ldclabs/anda-bot/main/scripts/install.sh | sh
```

Windows PowerShell：

```powershell
irm https://raw.githubusercontent.com/ldclabs/anda-bot/main/scripts/install.ps1 | iex
```

前置要求：

- 至少一个可用的模型提供方 API Key，可以写在 `~/.anda/config.yaml`，也可以通过支持的环境变量提供。

也可以用较新的 Rust 工具链从源码启动我：

```bash
git clone https://github.com/ldclabs/anda-bot.git
cd anda-bot
cargo run -p anda_bot --
```

第一次启动时，我会创建 `~/.anda/config.yaml`。如果终端 UI 提示模型配置缺失，请打开这个文件，填好 provider 信息，保存后回到终端按 Enter。API key 也可以在启动 Anda 前通过 provider 对应的环境变量导出。

最小模型配置示例：

```yaml
model:
  active: "deepseek-v4-pro"
  providers:
    - family: anthropic
      model: "deepseek-v4-pro"
      api_base: "https://api.deepseek.com/anthropic"
      api_key: "YOUR_API_KEY" # 设置 DEEPSEEK_API_KEY 时可留空
      labels: ["pro", "hippocampus"]
      disabled: false
```

支持的模型密钥环境变量包括 `OPENAI_API_KEY`、`ANTHROPIC_API_KEY`、`GEMINI_API_KEY`、`GOOGLE_API_KEY`、`DEEPSEEK_API_KEY`、`MINIMAX_API_KEY`、`MIMO_API_KEY`、`MOONSHOT_API_KEY`、`KIMI_API_KEY`、`BIGMODEL_API_KEY` 和 `GLM_API_KEY`。如果 `config.yaml` 中已经填写了 `api_key`，会优先使用配置文件里的值。

`hippocampus` 标签表示这一路模型可优先用于记忆大脑。如果没有 provider 带这个标签，我会使用当前激活模型。

如果你想为不同身份或项目准备独立记忆，可以换一个 home 目录：

```bash
anda --home /path/to/.anda
```

## 和我聊天

终端 UI 启动后：

- Enter 发送消息。
- Shift+Enter 插入换行。
- Ctrl+U 清空输入。
- Ctrl+A / Ctrl+E 跳到输入开头或结尾。
- 修改 `config.yaml` 后输入 `/reload` 重新加载。
- 输入 `/stop` 或 `/cancel` 中断当前回复。
- 输入 `/steer ...` 可以给正在生成的回复追加引导。
- Esc 查看状态，Ctrl+C 退出。

成功完成的对话会在后台提交给 Hippocampus 形成长期记忆。你不需要手动维护记忆文件。

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

在 Unix 平台停止或重启后台守护进程：

```bash
anda stop
anda restart
```

不打开终端 UI，直接发起一次请求：

```bash
anda agent run --prompt "总结一下你记得的当前项目背景"
```

启动语音对话：

```bash
anda voice --record-secs 8
```

语音模式需要 `transcription.enabled: true`。如果还想让我读出回答，需要 `tts.enabled: true`；如果只想语音输入、文字输出，可以加 `--no-playback`。

## 把我放到你的工作场景里

你可以只在终端里使用我，也可以编辑 `~/.anda/config.yaml`，把我接入聊天工具。

当前支持：

- IRC
- Telegram
- WeChat
- Discord
- Lark / 飞书

Telegram 最小示例：

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

更多渠道、语音转写和 TTS 配置可以参考 [anda_bot/assets/config.yaml](anda_bot/assets/config.yaml)。

## 文件、技能与自动化

我的本地工作区默认在 `~/.anda/workspace`。文件工具和 shell 工具默认都在这里工作。如果设置 `sandbox: true`，shell 执行会被路由到 `~/.anda/sandbox`。

你还可以把运行时技能放到 `~/.anda/skills`，让我加载更专门的工作流。cron 工具可以让我安排未来执行的 shell 任务或 agent prompt，并保存运行历史。

## 本地数据与隐私

默认情况下，我的状态保存在 `~/.anda`：

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

记忆图谱、会话、渠道状态、定时任务、密钥、日志和工作区数据都会放在这里。请注意，你配置的模型提供方仍可能接收 prompt 和记忆处理请求，所以请根据自己的隐私需求选择可信的 provider 或私有接口。

## 继续了解

- [Anda Bot 二进制使用指南](anda_bot/README.md)
- [Anda Hippocampus](https://github.com/ldclabs/anda-hippocampus)
- [Anda Hippocampus 产品站](https://brain.anda.ai/)

## 许可证

项目基于 Apache-2.0 许可证发布，见 [LICENSE](LICENSE)。
