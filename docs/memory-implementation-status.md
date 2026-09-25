# 记忆产品化实施记录

依据：[实施方案](memory-implementation-plan.md)。代码基线：Bot `9f64bd7c`、Brain `544acfc`。原生交付提交：Brain `04673a7`（`codex/memory-product`）。实现署名：Codex（OpenAI）。

状态：产品流程与原生联调已落地并完成验证；当前尚未发布。普通记忆不要求 MIB 或运行时配置。付费模型实验、真实业务部署、外部用户可用性试验没有执行，不能当成已验证的效果证据。

## 工作包交付

| 工作包 | 实现 |
| --- | --- |
| W00–W02 | owner 管理 API、逐请求 HTTP/WS 身份校验、独立普通记忆/待办状态、无副作用离线指南、CLI/TUI 概览；配套双语与六语言入口 |
| W03 | 同级 Brain 的原生产品合同及真实 Nexus 夹具；见下方能力矩阵 |
| W04–W06 | 实际持久消息索引、路由与策略 provenance，提交/检索 journal 投影、恢复协调、授权分页、只关联已验证原消息的浏览器状态；unknown 不重发 |
| W07–W08 | Assertion 记录视图、立场和有效范围、修订、来源摘要核对、原消息导航、显式预算搜索、次级图谱入口；旧记录/混合来源不伪造完整性 |
| W09–W10 | 原生及宿主操作账本、不可变预览、条件提交/查询/丢弃、同键异文冲突；更正保持旧声明，反向更正创建新操作；未知结果只查询 |
| W11 | standard/no_store/off 持久会话策略；CLI 和浏览器从空白新会话选择；初始化、恢复、压缩、模型实际工具调用和自动 Formation 受约束；未证明的子任务/书签/cron 派生路径直接拒绝 |
| W12 | 原生停用/删除预览、来源排除、处理 epoch、Notes/缓存清理、已接受工作归属及冷启动恢复；自动检索不能借历史/归档读取重新引入已停用内容 |
| W13 | 原生合同、Bot 管理链路和浏览器组件验证；真实用户可用性试验另列为发布后的验证工作 |
| W14–W15 | 最小 owner inbox 配置事务、摘要 CAS、私有备份和两文件恢复；真实记录订阅/取消；browser pending 与 TUI 稳定回答 outbox、按编号重试 |
| W16–W17 | 隔离 evaluate plan/run/report；冻结可执行文件、模型、提示词、数据及顺序种子；固定四类样例、两 Track/两 arm、串行预算、成对报告、取消清理与缺测费用 |
| W18 | 受支持工作流合同资产、真实原生学习准备度投影、隔离试验与业务应用权限分离、批准/撤销/回退操作说明；没有自动部署真实业务服务 |

## 原生合同与可用范围

用户已授权同时完善同级 `anda-brain`，通过临时本地 patch 联调，原生发布后切回 registry。同级 Brain 当前声明版本 0.12.1；本轮尚未验证 registry 可用性及其合同一致性。Bot 仓库未改外部 MIB 项目和部署资产。

| 门槛 | 本地联调结论 | 证据/边界 |
| --- | --- | --- |
| G1 | 在支持范围内可用 | 原生 Evidence client_key + payload digest 对接 Bot journal 和原消息摘要；来源不完整、混合归属或超大结果不开放精确操作 |
| G2 | 可用 | caller/operation 幂等意图、原生 idempotency key、固定预览与修订、两个已准备预览的冲突、取消等待者后的原生完成、按 ID 对账 |
| G3 | 在声明范围内可用 | 原生 epoch/来源排除先于变更；停用归档、删除清除有界闭包；旧来源重放/旧处理写入被拒；冷启动恢复、保留约束和 Notes 清理夹具通过 |
| G4 | 在明确受限路径中可用 | no_store/off 实际运行 fixture 的禁止写入为零，off 自动读取为零；Note 通过实际模型工具分发仍被拦截；未支持的派生工作不降级为 standard |
| G5 | 记录变化订阅可用 | 真实创建、原生归档取消、重试不重新启用、跨 recipient 拒绝、问题回答同键去重；控制器取消授权仅限该 Watch |
| G6 | 部分计量档 | 固定协议/时间/局部 token 边界，成本保留每 run 最后一份累计快照；未知费用非零，硬金额上限不支持，没有运行付费评测 |
| G7 | 合同和机制夹具可用 | 原生 workflow_http_v1 对独立服务、身份/摘要、校准与开关的验证继续生效；产品 ready 仅指隔离学习流程，不授予业务应用权限 |

更正使用撤回旧 Assertion、新建 Proposition/Assertion 和更正 Activity。Nexus 只允许同一 Proposition 内的 supersession，不能用修改 Concept 名称或跨 Proposition supersession 篡改旧事实。

移除范围是预览中的声明、引用输入和记录的反向依赖，最多 128 个原生元素。Concept 级联、未知来源、保留约束等拒绝执行。管理界面需要核对闭包内所有来源归属，不能只验证选中的第一条记录。用于防复活的最小来源标识和摘要会保留；源会话/续接链停止自动贡献，处理 Notes 与历史上下文重置。原聊天、文件、日志、备份、其他独立记录、已交付上下文和服务商副本不在擦除范围内。旧备份不能覆盖当前来源排除信息。

## 接口与存储

产品前缀为 `/daemon/memory/v1`，使用 Bot `ToolResponse`。WS `memory_*` 接口与 HTTP 复用服务；请求上限 64 KiB，搜索文本上限 8 KiB UTF-8。普通列表默认 20、最大 50。新产品 ID 为字符串。来源引用明确标注截短，不切割 Recall 包。搜索的服务端传输显式关闭重放，即使收到 503/504 也不会暗中再调用模型。

宿主 journal 保留 `bot-brain/v1/`：Formation/Recall 来源、changes、inbox-setup、record-watch 意图和 Notes epoch。`bot_memory_activity_v1` 只是可重建索引，不是任务真相。ObjectStore 不保证列举顺序，因此恢复按有界批次重扫与幂等修补，checkpoint 只记录完整扫描结果。终端回答保存在 home 下的私有 `memory-inbox-outbox`；不写入 bearer。

原生 `memory-product/v1` 保存条件操作、已接受状态、source exclusions 和 epoch。原生任务由 DurableTasks 管理，宿主变更由 TaskTracker 管理；客户端断线不取消已接受写入。关闭等待持久操作完成或保留可对账状态。

## 验证记录

验证均使用现有离线依赖与本地合成夹具，没有真实模型费用或生产配置变更：

- Bot 默认主程序：1,072 项通过；全功能主程序：1,095 项通过、1 个既有手动 HTTP fixture 忽略。`memory_` 针对性回归和 2 项 CLI integration tests 通过，包含真实模型工具分发、压缩策略继承、操作对账、无重试搜索、评测截止清理与截断日志重建。
- 启动器：75 项在沙箱内通过；进程组测试因 `zsh: operation not permitted: ps` 失败，已单独在所需权限下通过，没有修改启动器代码。
- Brain 初始交付时的独立验证：默认精简库 330 项通过；全功能 524 库测试、17 binary 测试、28 独立集成测试通过。
- Browser：312 项通过，check 无错误/警告，六语言无缺失键，生产构建通过。
- Docsite：typecheck 及 en/zh-Hans/es/fr/ru/ar 构建全部通过；本地 Docusaurus 更新检查提示权限不足不影响构建产物。
- CLI 离线 guide 的无 home/身份副作用测试通过。
- 在明确标识的合成组件页面检查桌面与 390px 窄屏、设置预览/重启提示、显式搜索、更正编辑/范围/确认/丢弃、订阅与取消。临时页面及开发服务已清理。没有把 localhost 组件夹具当作真实 Chrome 扩展安装验收。
- Brain 全功能 Clippy、Wiki-only/MCP-only 编译检查通过；Bot 全 targets/全 features Clippy 及两边格式检查通过。
- Cargo metadata 确认 Brain 0.12.1 为唯一 local path 包，Nexus 0.13.4、DB 0.13.2、KIP 0.13.1、Core/Engine 0.16 各只有一个 registry 包身份。

2026-09-23 复核修复：原生收据确认但返回失败时，Bot 仍完成 Notes 对账与删除预览清理；未建立的变更草稿可重新预览或丢弃；来源跳转按指定会话加载原消息；搜索保留超限与结果未知的错误码。回归覆盖已确认原生变更的宿主恢复、HTTP/WS 搜索错误及跨独立会话的来源定位。本轮未重跑同级 Brain 0.12.1 的独立测试套件；Bot 使用当前本地 patch 完成全功能联调。

## Memory Interface 接入（2026-09-25）

依据：Brain 的 Memory Interface 开发计划 A1–A7。依赖升级到 KIP 2.0 栈（`anda_kip =0.14.0`、Nexus/DB 0.14、Core/Engine 0.16 来自 crates.io；Brain 0.13.0 发布前以同级 `anda-brain` 的 path patch 构建）；`cargo metadata` 确认每个 anda crate 只有一个包身份。实现署名：Claude（Anthropic）。

| 项 | 实现 |
| --- | --- |
| A1 | 内嵌 Brain 的描述符（`memory_basic`）进入 `brain_runtime_status` 的 `memory_interface` 与产品概览能力 `memory_interface`；没有内嵌宿主的 HTTP 客户端沿用旧 Formation/Recall 路径 |
| A2 | Formation 窗口以 Bot 产品来源身份和 Formation 上下文受信暂存（Brain `stage_host_memory_source`），再以 `observe` 发送；键 = 对话/窗口/尝试序号，SourceOrder 流 = Bot 对话；窗口记录保存 `receipt_ref`、`attempt` 与 Brain conversation，状态跟随回执阶段；中断后同键重放，变长窗口或明确拒绝换下一个尝试键 |
| A3 | 每个 Bot 对话一个 `MemorySession`（`bot-brain/v1/memory-session/...`）保存未完成回执；`recall_memory` 召回前等待本对话回执最多 20 秒，未完成/失败的回执写入说明，成功召回后才确认；新会话以 attention 简报开始（外部 IM 发送者不注入） |
| A4 | `brain_attention` 附带按调用者保存游标的 `memory_attention`（`commitment_due`、`watch_fired`），不依赖待办配置；条目不授予权限，不转 cron/goal，推送通道不在范围内 |
| A5 | 更正三选一：`correct`（原生 supersession）、`world_change`（原生新断言 + 时序继承）、`misrecorded`（`revise` 记录修复，保持 `committing` 直到处理完）；删除走 `forget`（semantic），视图保留 ErasurePlan 报告；新增模型工具 `brain_feedback`（自述证据，外部 IM 与受限模式不可用）；浏览器更正对话框与六语言文案同步 |
| A6 | 集成测试：窗口回执、中断后同键重放、变长窗口换尝试键、召回屏障、attention 游标跨 journal 重启与调用者隔离、会话简报、feedback 与状态描述符；误记修复的回执重放、删除的擦除报告与原生预览释放 |
| A7 | `brain-integration(_cn).md`、本记录、README 对、docsite 页面、CHANGELOG |

已知边界：
- 会话简报只读 attention，不运行需要查询与 Recall 模型调用的 `resume`。
- `/daemon/memory/v1/search` 不属于任何 Bot 对话，不带 `after` 屏障。
- 由 2.1.0 草案栈写入的现有数据库不能被 0.14 栈原地打开；切换前须先迁移到新库并保留旧库只读（KIP 同步任务 §12，切换需所有者确认）。
- 真实模型下误记修复与 Formation 的行为效果未在本仓库验证。

## 发行与回滚

1. 原生合同合入/发布后，将 Bot 的 `anda_brain` registry 约束更新到实际发布版本，再移除临时 `[patch.crates-io]` 中的 Brain 路径项。
2. 更新 Cargo.lock，用 `cargo metadata --filter-platform aarch64-apple-darwin` 核对 DB、KIP、Nexus、Core、Engine 均只有一个包身份。不要为这次接口另开图谱或混用 sibling DB 类型。
3. 重跑产品合同、默认/功能构建、浏览器与 CLI 检查。不能仅移除 patch 后仍保留未发布接口调用。
4. 软件版本回退保留新的操作账本和 source exclusions；回滚配置文件不会撤销已经安装的原生授权。业务部署须先停止新调度、撤销原生执行权限，再按原 Attempt 身份对账并执行安全恢复。

真实服务商成本校准、业务部署批准和外部可用性试验需要独立的实际配置与证据；本实现不自动开启它们。

署名：Codex（OpenAI）。

## 复核命令

```sh
cargo fmt --check
env -u LIBRARY_PATH RUST_MIN_STACK=16777216 cargo test --offline -p anda_bot --bin anda
env -u LIBRARY_PATH RUST_MIN_STACK=16777216 cargo test --offline -p anda_bot --all-features
env -u LIBRARY_PATH RUST_MIN_STACK=16777216 cargo test --offline -p anda_bot --all-features memory_
env -u LIBRARY_PATH RUST_MIN_STACK=16777216 cargo clippy --offline -p anda_bot --all-targets --all-features
pnpm --dir chrome-extension check
pnpm --dir chrome-extension test
pnpm --dir chrome-extension i18n
pnpm --dir chrome-extension build
pnpm --dir docsite typecheck
pnpm --dir docsite build
```

同级 Brain 对 `--lib` 默认、`--all-features`、Wiki-only/MCP-only 和全功能 Clippy 分别验证。启动器进程组测试需要允许 `ps`；受限沙箱失败时保留其错误并在所需权限下单独复核。
