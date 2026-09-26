# Anda Desktop 本地版实施记录

日期：2026-09-26。分支：`codex/desktop-client`。目标平台：本机 macOS Apple Silicon。

## 已交付

- Electron + Svelte 桌面工作台：项目、聊天、搜索、置顶/归档、草稿恢复、审批卡、附件预览、深浅主题和六语言界面。
- 直接复用 Chrome 扩展的 Channel、消息、输入、审批、Memory、Skills、Bookmarks 和配置实现；通过 `UiClient` 上下文和 `ClientPlatform` 隔离桌面与 Chrome API，未复制整套前端。
- 自动化管理接入现有 cron 工具，支持创建、编辑、暂停/恢复、移除和运行记录。
- Main 持有凭证与 WS 连接；preload 只暴露受限 IPC。Svelte 数据跨 IPC 前转为普通快照。应用资源使用受限的 `anda-app://app` 协议。
- 托盘、原生菜单、窗口位置恢复、通知、深链接、项目目录选择、服务重启/停止、录音和打印接入。关闭桌面窗口与停止 daemon 分开。
- 已受理状态不确定的消息记录在本地，禁止自动重发；通道历史在桌面只读，避免改变原发送者或 IM 回复路由。
- Rust 增加桌面能力探测、owner 工作目录授权、配置 revision 冲突检查、owner-only 配置管理、无初始化副作用的 `validate-config` 命令，以及 bundled runtime 的独立更新禁用。
- 离线配置仍通过 Rust parser 校验；备份后原子替换，运行中的 daemon 不允许被离线写入路径绕过。
- macOS ARM64 `.app`、DMG 和 ZIP，包含 release 模式 Rust runtime。安装说明见 [desktop/README.md](../desktop/README.md)。

本机正常启动打包应用后，原生 UI 已显示本地 daemon 连接成功，并加载现有模型和会话列表；没有主动发送模型测试消息。应用和服务保留供用户查看。

## 与设计路线的差异和边界

这次交付以本机可安装使用为边界。下列设计项仍是后续工作，不能视为已实现：

- 保留现有 WS 和会话增量轮询，没有新增 `/ws/app/v1`、服务端事件订阅、Rust→TS 全量协议生成或逐 token 推送。
- 不确定提交使用本地 journal 和明确的用户核对流程；没有宣称服务端 exactly-once。`/side` 在等待响应时允许继续发送前台消息，不把已知在途请求误判为未知提交。标题、项目、归档等桌面导航元数据保存在 Electron profile，正文与记忆继续由 daemon 管理。
- 共享源码暂留在 `chrome-extension`，通过依赖注入复用；没有为目录形式而移动所有文件到 `packages/`。
- 更新采用手动安装新桌面包；没有发布更新 feed、自动迁移旧 launcher 的登录注册或实现在线任务排空升级。使用 bundled runtime 时，升级前先完成任务、停止服务并退出桌面。
- 未包含完整内嵌浏览器、PTY 终端、Git/worktree、远程主机和 Linux 产品化。
- Windows 打包配置已提供，但未在 Windows 设备上验证。麦克风、实际 TTS 音频设备、打印机、多显示器热插拔和系统重启尚未做实机验收。
- 当前 macOS 包为本机开发用途的 ad-hoc 签名包，未做 Developer ID 公证。

## 验证记录

| 检查 | 结果 |
| --- | --- |
| Desktop TypeScript/Svelte check | 0 errors / 0 warnings |
| Desktop 单元测试 | 10 项通过：传输、凭证边界、未知提交、并行侧任务、显式停止、原子存储、协议权限、资源路径、六语言键 |
| Electron 原生 E2E | 通过：启动、真实 IPC/WS、发送、草稿切换、管理页、项目、审批、窄窗口、深色和中文；使用隔离 profile 和 mock daemon |
| Chrome 扩展 check/test/i18n/build | 通过；346 项测试，六语言无缺失键 |
| Rust daemon 单元测试 | 1206 项通过 |
| Rust launcher 单元测试 | 86 项在沙箱内通过；进程组测试报 `restart script never reported its pgid`，沙箱外定向重跑通过 |
| CLI / memory_cli 集成测试 | 4 项通过 |
| Rust release runtime | 构建成功，随应用打包 |
| 应用签名完整性 | `codesign --verify --deep --strict` 通过；应用和 runtime 均为 ARM64 |
| 真机启动 | 正常应用启动后，通过原生 UI 核实已连接本机服务并加载数据 |

第一次脚本启动的真机检查在凭证 helper 处报告 `Anda browser failed. Inspect the daemon log for details.` 并超时；随后通过正常应用启动确认连接成功。没有把该失败脚本记录为通过，也没有从现象推断未经验证的根因。

E2E 截图位于忽略提交的 `desktop/test-results/`，使用合成内容。本次最终安装包位于忽略提交的 `desktop/release/validated/`，避免覆盖用户正在查看的先前预览；正常构建默认输出 `desktop/release/`。真实用户数据、凭证、runtime 二进制和安装产物均不进入 Git。
