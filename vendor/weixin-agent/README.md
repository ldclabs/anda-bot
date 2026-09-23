# Weixin Agent SDK for Rust

[![Crates.io](https://img.shields.io/crates/v/weixin-agent.svg)](https://crates.io/crates/weixin-agent)
[![docs.rs](https://docs.rs/weixin-agent/badge.svg)](https://docs.rs/weixin-agent)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-%3E%3D1.85.0-orange.svg)](https://www.rust-lang.org)

微信 iLink AI Bot 协议的 Rust SDK 实现，基于 [`@tencent-weixin/openclaw-weixin`](https://www.npmjs.com/package/@tencent-weixin/openclaw-weixin) v2.4.3 协议层等价移植。

本 SDK 是纯协议层实现，**不耦合 OpenClaw**，可用于自定义 Agent 接入微信 ClawBot 使用。

## 功能特性

- iLink Bot API 全端点封装（getUpdates / sendMessage / getUploadUrl / getConfig / sendTyping / notifyStart / notifyStop）
- 长轮询消息循环（自动退避重连、Session 过期处理、动态超时调整）
- CDN 文件上传/下载（AES-128-ECB 加解密、自动重试）
- 消息收发（文本、图片、视频、文件、语音，含引用消息解析）
- 出站文本 Markdown 过滤（`StreamingMarkdownFilter`，默认开启，可配置关闭）
- QR 码登录 API 封装（含配对码验证流程）
- 连接生命周期通知（notifyStart / notifyStop）
- 纯协议 SDK — 不管理状态持久化，由调用方自行决定存储策略
- 统一 async/await（基于 tokio + rustls）

## 协议版本

| 参考实现 | 版本 | 说明 |
|---------|------|------|
| [`@tencent-weixin/openclaw-weixin`](https://www.npmjs.com/package/@tencent-weixin/openclaw-weixin) | 2.4.3 | 协议层等价移植（不包含 OpenClaw 插件框架部分） |

## 快速开始

添加依赖：

```toml
[dependencies]
weixin-agent = { git = "https://github.com/spensercai/weixin-agent-sdk-rs" }
tokio = { version = "1", features = ["full"] }
async-trait = "0.1"
```

最小示例：

```rust
use async_trait::async_trait;
use weixin_agent::{WeixinClient, WeixinConfig, MessageHandler, MessageContext, Result};

struct EchoBot;

#[async_trait]
impl MessageHandler for EchoBot {
    async fn on_message(&self, ctx: &MessageContext) -> Result<()> {
        if let Some(text) = &ctx.body {
            ctx.reply_text(text).await?;
        }
        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let config = WeixinConfig::builder()
        .token("your-bot-token")
        .build()?;

    WeixinClient::builder(config)
        .on_message(EchoBot)
        .build()?
        .start(None)
        .await
}
```

## SDK 与应用层的职责边界

本 SDK 只负责协议通信，不负责应用层逻辑：

| 职责 | SDK | 应用层 |
|------|:---:|:------:|
| HTTP API 封装 | ✅ | |
| 长轮询 + 重连 | ✅ | |
| CDN 上传/下载/加密 | ✅ | |
| 消息解析/构建 | ✅ | |
| 出站 Markdown 过滤 | ✅ | |
| QR 码 API 调用 | ✅ | |
| 连接生命周期通知 | ✅ | |
| Context Token 内存管理 | ✅ | |
| sync_buf 持久化 | | ✅ |
| 账号凭证存储 | | ✅ |
| 权限白名单 | | ✅ |
| 斜杠命令 | | ✅ |

`sync_buf` 通过 `MessageHandler::on_sync_buf_updated()` 回调通知，调用方自行持久化。Context Token 提供 `export_all()` / `import()` 接口供调用方备份恢复。

## 核心 API

### WeixinConfig

```rust
let config = WeixinConfig::builder()
    .token("your-bot-token")
    .bot_agent("my-app/1.0")       // 可选，默认 "weixin-agent-rs"
    .markdown_filter(false)         // 可选，默认 true（开启出站 markdown 过滤）
    .base_url("https://custom.example.com/")  // 可选
    .build()?;
```

### MessageHandler trait

```rust
#[async_trait]
pub trait MessageHandler: Send + Sync {
    /// 处理收到的消息
    async fn on_message(&self, ctx: &MessageContext) -> Result<()>;

    /// sync_buf 更新回调 — 在此持久化
    async fn on_sync_buf_updated(&self, _sync_buf: &str) -> Result<()> { Ok(()) }

    /// 启动前回调
    async fn on_start(&self) -> Result<()> { Ok(()) }

    /// 关闭前回调
    async fn on_shutdown(&self) -> Result<()> { Ok(()) }
}
```

### MessageContext

```rust
impl MessageContext {
    pub async fn reply_text(&self, text: &str) -> Result<SendResult>;
    pub async fn reply_media(&self, file_path: &Path) -> Result<SendResult>;
    pub async fn download_media(&self, media: &MediaInfo, dest: &Path) -> Result<PathBuf>;
    pub async fn send_typing(&self) -> Result<()>;
    pub async fn cancel_typing(&self) -> Result<()>;
}
```

### QR 码登录

在创建 `WeixinClient` 之前，可通过 `StandaloneQrLogin` 独立完成 QR 码登录获取 token：

```rust
use weixin_agent::{StandaloneQrLogin, WeixinConfig, LoginStatus};

let config = WeixinConfig::builder().token("").build()?;
let qr = StandaloneQrLogin::new(&config);
let session = qr.start(None, &[]).await?;
println!("请扫描二维码: {}", session.qrcode_img_content);

loop {
    match qr.poll_status(&session, None).await? {
        LoginStatus::Confirmed { bot_token, base_url, .. } => {
            // 保存 token，用 token 创建 WeixinClient
            break;
        }
        LoginStatus::NeedVerifyCode => {
            // 提示用户输入手机上显示的验证码
            // 下次 poll_status 时传入 Some("1234")
        }
        LoginStatus::Expired => { /* 重新获取 QR 码 */ }
        _ => tokio::time::sleep(Duration::from_secs(2)).await,
    }
}
```

已有 `WeixinClient` 实例时也可通过 `client.qr_login()` 获取 QR 登录 API：

```rust
let qr = client.qr_login();
let session = qr.start(None, &[]).await?;
// ... 同上
```

### Markdown 过滤

SDK 默认对出站文本应用 `StreamingMarkdownFilter`，过滤微信不支持的 Markdown 语法：

```rust
use weixin_agent::{StreamingMarkdownFilter, filter_markdown};

// 一次性过滤
let filtered = filter_markdown("**粗体** *中文斜体* ![img](url)");
// → "**粗体** 中文斜体 "（保留粗体，去除 CJK 斜体标记，移除图片）

// 流式过滤（适用于 LLM 流式输出）
let mut f = StreamingMarkdownFilter::new();
let out1 = f.feed("**hello** ");
let out2 = f.feed("*world*");
let out3 = f.flush();
```

通过 `.markdown_filter(false)` 关闭：

```rust
let config = WeixinConfig::builder()
    .token("tok")
    .markdown_filter(false)
    .build()?;
```

### 主动发送消息

```rust
client.send_text("user_id", "hello", Some("context_token")).await?;
client.send_media("user_id", Path::new("/path/to/file.jpg"), None).await?;
```

## 项目结构

```
src/
├── lib.rs              # 公共 API 导出
├── client.rs           # WeixinClient + Builder
├── config.rs           # WeixinConfig（协议级配置）
├── error.rs            # 统一错误类型
├── types.rs            # 协议类型定义
├── api/                # iLink Bot HTTP API
│   ├── client.rs       # HTTP 客户端（含 notifyStart/notifyStop）
│   ├── session_guard.rs # Session 暂停/冷却
│   └── config_cache.rs # typing_ticket 缓存
├── monitor/            # 长轮询消息循环
├── messaging/          # 消息解析/构建/发送
│   ├── markdown_filter.rs # 出站 Markdown 过滤器
│   └── ...
├── cdn/                # CDN 上传/下载 + AES-ECB
├── qr_login/           # QR 码登录 API（含配对码验证）
├── media/              # MIME 类型检测
└── util/               # 日志脱敏 / ID 生成
```

## 文档

- [整体架构](docs/architecture.md)
- [长轮询生命周期](docs/poll-lifecycle.md)
- [CDN 上传流程](docs/cdn-upload.md)

## 与 Node.js 版本的设计差异

| Node.js (openclaw-weixin) | Rust (weixin-agent) | 说明 |
|---|---|---|
| OpenClaw 插件框架 | 独立 SDK | 不耦合宿主框架 |
| 文件系统持久化 | 回调 + export/import | 调用方决定存储策略 |
| 内置斜杠命令 | 不包含 | 应用层自行实现 |
| 内置账号管理 | 不包含 | 应用层自行实现 |
| 类/回调函数 | Trait + Builder | Rust 惯用模式 |
| 自定义 JSON logger | tracing | Rust 生态标准 |
| native-tls (OpenSSL) | rustls | 纯 Rust TLS，无系统依赖 |

## 环境要求

- Rust ≥ 1.85.0（edition 2024）
- tokio 异步运行时

## License

MIT
