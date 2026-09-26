# Anda Desktop 本地版实施记录

当前分支：`codex/desktop-client`。桌面工作台版本：`0.13.1`；下方首版记录对应 `8f04cd7e`，后续交付见文末。

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


## 后续工作台交付（2026-09-26）

本轮沿用 Chrome 扩展的产品能力和共享前端，完成以下增量实现：

- **应用协议**：`/ws/app/v1`、owner 鉴权、持久提交回执、caller 范围的状态通知。通知采用有界合并后的失效提示，客户端读取权威差量；重连重新同步，旧 runtime 回退轮询。请求受理后不随 socket 消失而取消；完成响应（包括 `/side`）恢复到界面后才清除本地回执引用；相同键同文返回已有结果（递归规范化 JSON 键顺序），异文冲突，无法判定的崩溃保留 unknown。
- **类型合同**：使用测试期 `ts-rs` 从 Rust DTO 生成 `desktop/src/shared/app-protocol.ts`，普通测试检查生成结果一致。没有修改现有工具 wire format，也没有改变 Brain/Core/DB 的 patch/type identity。
- **浏览器**：WebContentsView 标签页、导航/查找、独立 cookies、下载、权限处理和 Agent BrowserBridge。页面快照、输入、点击、截图、页面脚本等复用扩展页面函数并由 Electron/CDP 执行。每个桌面聊天有明确 session；断开时不会转到另一个 Chrome session。远端页面没有应用 preload、Node 或 bearer。
- **终端**：独立 utility process 托管 node-pty；xterm 支持多会话、输入、resize、查找及面板恢复。输出有界并通过 ACK 控制渲染推送；退出前确认结束终端。修复 node-pty macOS 预编译 spawn-helper 的执行权限，构建时自动处理。
- **Git**：状态、文本 diff、历史、暂存、取消暂存、提交、创建 worktree、归档和恢复。写入检查仓库状态指纹；归档先创建受引用保护的快照，只清理桌面自己创建的工作树，拒绝有忽略文件、子模块或嵌套仓库的情况。
- **音频**：设备选择、录音电平/时长、回放、转写/TTS 自检和取消。正常聊天 TTS 增加取消播放与停止后续分段合成的能力；真实麦克风和扬声器没有被自动化测试替代。
- **更新**：签名发行配置与 electron-updater 协调器；核对 daemon 的真实运行路径以区分托管/外部安装。90 秒可续期维护租约阻止新任务，保留已授权的取消操作，cron 保留到期任务，IM 按原路回复稍后重试，活动任务完成后才关闭托管 runtime；已有任务的内部 Brain 调用继续通过原始签名身份执行，并检查 Brain 自身的处理状态。下载/安装意图持久化；安装失败尝试恢复，外部 daemon 保持独立。
- **Windows**：新增原生 Windows CI、NSIS 安装/卸载检查、AppUserModelID 及签名参数。已编写代码和工作流，尚无 Windows 执行结果。
- **打包完整性**：runtime 签名完成后再记录 manifest 哈希，随后签名外层应用，避免安装后哈希校验误报。本地 ad-hoc 包使用独立 entitlement 解决 Electron Framework Team ID/library validation 启动问题；正式签名配置使用单独的 entitlement 并要求证书、公证。

### 本轮验证

| 检查 | 已取得的结果 |
| --- | --- |
| Desktop check / 单元测试 | 0 errors / 0 warnings；23 项通过，覆盖回执对账及保留待交付响应、Git 文件保留/分支切换冲突/归档恢复、更新协调顺序及主文档音频权限边界 |
| Rust daemon | 完整 1218 项串行通过，包含应用 WebSocket 回执/通知、JSON 摘要规范化、维护状态和维护期间取消任务测试 |
| Launcher / CLI | 87 项 launcher、2 项 CLI validation、2 项 memory CLI 在沙箱外串行通过 |
| 维护状态 | cron 延迟后仍可运行；外部 IM 维护回复保留 recipient/thread；租约归属和过期测试通过 |
| Chrome 扩展 | check、349 项单元测试、六语言 i18n、build 通过 |
| Native Electron E2E | 隔离 profile 与 mock daemon：聊天、审批、草稿、Git diff、PTY 命令、浏览器提取/输入/点击/截图及页面隔离、合成录音/转写/TTS、窄窗口、主题及中文通过 |
| Cargo metadata | Core/Engine 0.16.1、DB/KIP 0.14.0、Brain 0.13.0 各一套类型来源；仅增加协议生成测试依赖 |
| 打包应用 | 独立 home/profile、daemon 停止状态下，真实 `.app` 启动、runtime 哈希、原生 PTY 和浏览器隔离通过 |
| Windows / 签名发行 / 真实音频 | 未运行；需要 Windows runner/设备、签名身份/更新源、麦克风和听音验收 |

原生 Electron 在命令沙箱内启动失败，GUI 回归改在沙箱外使用隔离目录运行。一次全量测试中的 `tts::edge::tests::timeout_and_cancellation_terminate_child` 失败，沙箱外定向重跑通过，其后另一次并行回归的 launcher staging 测试也出现临时文件不存在；最终在沙箱外串行运行全部 1218 项通过。测试没有访问真实 Anda 数据库，没有向实际 IM 发消息，也没有调用付费模型。浏览器原生视图单独截图到 `10-browser-content.png`；主 renderer 的截图不会包含其他 WebContentsView 的像素。

### 明确保留的边界

- 当前推送的是已保存状态变化。核实 `anda_engine 0.16.1` 的 OpenAI 适配器仍聚合 SSE 后返回 `AgentOutput`；没有新增跨 provider 的逐 token hook。
- 导航元数据仍在桌面 profile；共享源码仍通过平台适配复用，不为目录形式搬迁全部扩展代码。
- 用户终端和浏览器 tabs 随桌面进程退出；持久终端、完整 Chrome 扩展生态、Git 远端凭证/merge/rebase UI、远程主机及 Linux 产品化不在本轮增量中。
- Windows 实机、物理音频/打印/多屏、公开签名、公证及真实更新源上的跨版本升级仍待外部条件验收；本地可构建的代码不等于这些检查已通过。


本轮交付安装包位于 `desktop/release/validated-workbench/`：`Anda-0.13.1-mac-arm64.dmg`、`Anda-0.13.1-mac-arm64.zip`，附 `SHA256SUMS.txt` 和 `verification.json`。最终 `.app` 已通过独立 profile 的打包后启动/PTY/浏览器测试；应用签名完整性和 runtime manifest 哈希通过。本机包为 ad-hoc 签名，公开公证/签名仍未进行。安装前完成现有任务，停止旧版服务并退出旧桌面，再将新应用拖入 Applications。现有真实 daemon 和用户数据未被本轮测试重启或替换。
