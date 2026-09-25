# Brain 运行时集成

Anda Bot 0.13 内嵌基于 KIP 2.0（`cognitive-memory@2.0.0`）的 Brain 0.13，并通过它的 Memory Interface 绑定（`memory_basic`）接入：Formation 窗口是带持久回执的 observe 意图，recall 会等待本对话自己的回执，记忆 attention 有保存的游标。不配置运行时时，普通 Formation、Recall、Maintenance 继续工作。持久待办、独立观察、语义求值、utility、trust 和原生学习分别配置；编译、安装绑定、启用自动运行、机制测试与真实业务收益是不同状态。

## 先使用日常记忆

运行 `anda memory guide` 阅读离线指南，或运行 `anda memory` 检查已启动的服务。这些命令不调用模型；指南也不创建 home 或身份。浏览器工作台默认进入“记忆”，提供示例、连接状态、记录与来源引用；“探索关系”保留原图谱。TUI `/memory` 与裸 `/brain` 显示概览，显式 `/brain status` 仍是技术详情。

告诉 Anda 一条稳定偏好，等后台整理后用 `/new` 开新对话询问。助手说“记住了”不等于保存收据。消息旁状态和 `/memory activity` 只证明对应对话的处理进度，Formation 完成不保证提取了期望的事实。接受结果未知时只对账，不重新发送。只有持久消息映射和内容摘要都匹配，来源引用才提供原消息导航。

所有者可搜索、更正、停用或删除受支持的记录。搜索由用户显式发起，可能调用模型；完整保留 Recall 结果包、警告，默认结果包上限 4,096 token、规划输入上限 32,768 token，不自动重试，也不保证总金额上限。更正和移除先展示绑定修订的预览（原生变更十分钟内有效，误记报告十五分钟），再确认执行。失联后按原操作 ID 查询，不新建操作重发。

更正时先选择是哪一种修改。**是我说错了**（`correct`）在同一时段内取代所有者自己的旧说法，反向更正也是新的条件操作。**情况变了**（`world_change`）从现在起写一条新声明，时序继承结束旧值，旧值在它的时段内仍然成立。**你记错了**（`misrecorded`）是记录修复，绝不当作更正：所有者的报告以 Memory Interface `revise`（`change_kind: "misrecorded"`）按该操作的键发送，Brain 重读原始来源并修复抽取结果；在 Brain 处理完之前变更保持 `committing`（重试会重放同一回执）。无法完成的修复或擦除显示为失败或被阻止，而不是一直等待。

停用将预览中的声明、引用输入和已记录的派生项归档；删除以 Memory Interface `forget`（`mode: "semantic"`）执行：清除该有界原生集合，擦除 Brain 自己引用过这些内容的 Formation 与 Recall 记录，并把 ErasurePlan 报告（`completed`；有存储面无法验证时为 `partial`；被法律保留阻止时为 `blocked`）随确认后的变更保留。旧回执下已由原生产品路径确认的删除只做对账。两者都会持久保存来源排除、拦截在途旧处理，并清空原生及 Bot Notes 和处理历史上下文。相关来源会话及其续接链不再自动贡献新记忆。其他独立记录、Bot 原聊天、文件、日志、备份、已交付模型的上下文及服务商副本仍保留。用于防重放的最小来源标识和摘要仍保留。来源缺口、受保护依赖、保留约束、超大派生集合会使操作被拒绝。直接用不含当前排除元数据的旧备份覆盖数据库，不在此保证内。

新会话可在浏览器输入框的“记忆模式”中选择，也可运行：

```sh
anda agent run --memory-mode no-store --prompt '帮我处理这项任务，本次不保存长期记忆'
anda agent run --memory-mode off --prompt '只使用这个新对话的信息帮我处理任务'
```

模式约束宿主管理的 Brain/Notes，覆盖隐式用户资料初始化和自动 Formation。受限模式会拒绝尚不支持继承策略的子任务、记忆动作、书签及 cron 变更。聊天记录、文件、日志和服务商处理并非无痕。切换模式会创建空白新对话，不复制旧上下文；已有会话保留持久化策略。

## 产品 API 与开发依赖

`/daemon/memory/v1` 是使用 Bot `ToolResponse` 的所有者管理入口，与原生 KIP、模型工具分开。HTTP 和 WebSocket 每次请求都重新验证原凭证。概览、记录、搜索、变更预览/提交/查询/丢弃、待办设置与记录订阅复用同一服务。活动查询还支持已认证调用者自己拥有的指定会话，不暴露其他记录或计数。产品响应的标识统一为字符串。

活动分页按提交时间和稳定的 journal 键排序，旧索引重建后也保持该顺序。升级后若已保存的游标被拒绝，请从首页刷新。启动与低频对账负责修复索引，日常轮询只读取变化和未完成记录。来源校验缓存仅在同一调用者的单次请求内复用。

记录分页在达到响应大小预算前停止，返回指向首条尚未返回记录的游标。`response_size_limit` 表示还有后续记录，不代表来源缺失。较长引用会标记 `text_truncated`，记录身份和原始声明标签保持不变。订阅列表接受可选的 `cursor` 和 `limit`（1–50），以绑定调用者的游标返回生效订阅，已取消历史不占配额。浏览器读完所有页面后才启用新建订阅。

普通和结构化 Recall 共用可复用、禁用自动重试的 HTTP 传输。失败响应可能发生在模型任务已接受之后，因此重新搜索必须显式发起。变更确认保留版本冲突、过期等具体错误；接受结果不明时，仍核对原生回执后才报告成功。

0.14 版 DB/KIP/Nexus 栈及配套的 Core/Engine 0.16 来自 crates.io；Brain 0.13 发布之前，`[patch.crates-io]` 从同级 `anda-brain` 检出构建它。`Cargo.lock` 中每个 crate 只有一个类型身份。0.12.0 的数据库是 KIP 1.x，首次启动时自动迁移。开发构建在 2.1.0 草案（Brain 0.12.1 / Nexus 0.13.4）下写入的数据库不能被原地打开，需先迁移到新库（见 Brain 的草案 Space 迁移），旧库保留只读以便回退。软件回滚不能简单覆盖旧数据库而丢失当前来源排除信息。

## 需要跨会话确认时，再设置待办

日常记忆不需要待办。在“记忆”中点击“设置记忆待办”，或运行 `anda memory inbox setup` 预览仅面向所有者的配置。使用 `--apply` 确认所展示的摘要；向导保存私密备份，检查原配置摘要，再写入最小受管运行时文件。环境覆盖和自定义运行时文件会保留，需明确手动合并。运行 `anda restart` 后才生效，再回来检查可访问性。回滚配置文件不等于撤销原生授权。

设置后，“这条记忆变化时向我确认”创建真实、收件人拥有的 Watch。控制器只获得该新 Watch 的归档权限，用于取消，无需扩大到任意记录的管理权限。取消会归档原生 Watch，重试和重启都不重新启用它，也不恢复已撤销的取消权限。取消未来观察不会撤回已经投递的问题。确认问题与投递收据是不同的原生待办项。

TUI 使用 `/memory inbox`、`/memory next`、`/memory answer <编号> <正文>`、`/memory retry <编号>`。终端和浏览器都在发送前保存原正文与事件键；回复丢失后沿用原数据重试，不允许悄悄替换待发正文。acknowledged 仅表示回答已被接受，不表示业务任务已成功。

使用 `anda memory inbox` 读取待办，按输出提示运行 `anda memory inbox --cursor <next_cursor>` 读取下一页。使用 `--json` 时，游标位于 `result.next_cursor`。


## 启动配置

在 `~/.anda/config.yaml`（或选定 home）中添加：

```yaml
brain:
  runtime_config: brain-runtime.yaml
```

相对路径基于配置目录解析，环境变量 `BRAIN_RUNTIME_CONFIG` 优先于 YAML，支持绝对路径。留空保持普通记忆行为，修改后重启。配置是可信部署输入，模型、普通请求和记忆内容不能安装身份、地址或凭据。

复制[本地 inbox 示例](../anda_bot/assets/brain-runtime.example.yaml)，将 CWT subject 替换为实际 owner 身份，保存到仓库外。Brain 在首次加载 Space 前验证全部配置；未知适配器、缺少秘密、主体冲突会使启动失败。bootstrap 不会恢复已撤销 grant、重新 arm Watch、授予 `manage_trust` 或批准 Skill 执行。控制器、接收者、观察者、治理者是不同角色，Bot 的通配 CWT 不自动获得这些原生身份。

复用现有后台生命周期和持久工作目录，保持每 shard 单一所有者及关闭时写入排空。学习 HTTP 服务必须预先启动：加载 Space 时会探测 executor 能力和独立 observer 身份，不能将启动回调配置为此时尚未监听的 Bot 网关。

## 待办与身份

旧版导出的浏览器令牌缺少 Brain audience，需要运行 `anda browser token` 重新导出并更新扩展设置。新令牌限定 `anda_bot`，保留原有 subject、scope 和有效期上限。

浏览器 Brain 页面提供“记忆待办”。TUI 使用 `/brain inbox`、`/brain status`、`/brain next <cursor>`；回答用 `/brain answer <id> <event_key> <answer>`，陈述用 `/brain statement <id> <event_key> <statement>`，帮助为 `/brain help`。

模型通过 `tools_select` 发现 `brain_attention`、`brain_respond`、`brain_runtime_status`、`brain_feedback`。无论是否配置待办，`brain_attention` 都会附带 `memory_attention`：调用者上次查看以来提起的到期承诺和已触发的 Watch，游标按调用者保存。`brain_runtime_status` 附带 Brain 声明的 Memory Interface 描述符。`brain_feedback` 把模型对某个决定或尝试的自述记录为带归属的 agent 证据（`feedback`），它不是评分、结果或已验证事实，受限记忆模式下不可用。回答工具严格 schema 要求同时传 `answer` 和 `statement`，未使用的字段为 null，校验后转换成 Brain 原生请求。外部不可信 IM 用户不能使用这些工具。HTTP/WS 转发调用者自己的 bearer；Engine 工具从真实认证上下文映射显式配置的原生主体。匿名、local、public 模式不绕过运行时认证；用户队列及审计清单相互隔离。

分页读取不领取工作，`complete` 只表示分页结束。cursor 过期或失效时从首页刷新。回答 URL 使用返回的十六进制 `id`，不是 `wake/v1/...`。浏览器先保存事件键和正文再发送，丢失确认或重载后可原样重试；TUI 重试须保留相同事件键和正文。同键异文返回冲突。澄清回答不授予权限，自述不是独立 Outcome。

`attention_inbox_v1` 使用 Brain 原生持久投递和 Attempt 身份，重启后原生记录仍是工作真相。Bot 只保存图谱外的会话、渠道、接收者、路由关联，保留 `(reply_target, thread)`。现有 IM 渠道没有启用自动 Brain 分派：`send -> Result<()>` 无法证明投递或权威 NotStarted。不支持的 adapter 在启动时拒绝，普通 IM 聊天沿用既有权限和路由。不会把 Watch 包装成无人值守 cron 或 full-access goal。

## Recall 与 Formation 收据

Brain HTTP KIP 使用应用参数：`command`，或带 `execution` 的 `operations`，以及 `parameters`、`read`、顶层 `dry_run`。响应仍为 KIP 2.0，同时检查请求级和 operation 级状态。原生 Rust 信封在统一边界转换；无法表达的请求元数据、preconditions、deadline、extensions 明确拒绝，不静默丢弃。

`recall_memory` 支持可空 `budget`，包含 Brain 固定 tokenizer、`max_tokens`、`context_tokens`。null 保持兼容。保留完整记忆包和不足状态，不通过 artifacts 或收据追加记忆。失败有明确错误标记，保留直接及嵌套计量，嵌套值不重复加总；缺失账单仍是不完整。

`recall_memory` 召回前最多等待 20 秒，等本对话自己尚未完成的 Formation 回执（Memory Interface 处理屏障）。仍在处理或已失败的回执会在答案前以说明列出（预算结果包保持不变）；只有成功的召回已计入的回执才从本对话的会话中清除。新会话以所有者上次会话以来提起的记忆 attention 作为系统上下文中的数据段开始，外部 IM 发送者不会收到。这是不调用模型的 `attention` 读取：完整的 `resume` 召回需要查询并运行 Recall 模型，因此不会在每个会话自动执行。

现有 DB 对象存储中的 `bot-brain/v1/` 宿主日志保留 Recall 交付身份、Brain conversation/receipt、Bot conversation/turn 及可明确对应的模型 tool-call ID。并行重复参数无法区分时不猜测关联。收据仅证明交付；普通检索不改变 utility、trust 或执行权限，legacy trace 不证明贡献。

Formation 在发送前持久化窗口。每个窗口先以 Bot 自己的产品来源身份（产品删除因此仍覆盖它）连同 Formation 上下文暂存为来源，再作为 Memory Interface `observe` 发送：幂等键包含对话、窗口和尝试序号，来源顺序流为该 Bot 对话。窗口记录保存回执及其启动的 Brain conversation，状态跟随回执阶段（recorded → accepted，processed/available → completed，failed → failed）。中断的提交按同一键重发并重放同一回执，不会把窗口整理两次；中断后变长的窗口或明确的拒绝进入下一个尝试序号的键。接受后的原生 Formation 失败仍由 Brain 重试。`/brain formation <id>` 仍查询指定的 Brain conversation，不用全局 high-water mark 推断完成。没有内嵌宿主（仅 HTTP 客户端）时沿用原来的直接 Formation 路径。

## 独立观察与可选校准

`/outcomes` 原生路由及 typed Rust client 供独立认证的 instrument 使用，不注册普通模型工具，也不由 completion hook 替代观察者。原生 Action context/Decision 保留真实 `used_refs`、`applied_revisions` 和 Recall receipt；观察者通过 `OutcomeInput.utility` 提交原生单项贡献 witness。交付、使用、贡献可区分，无法归因的普通任务只保留审计，不伪造 trial 或涨分。

启动配置 `semantic`、`utility`、`trust` 直接交给 Brain 校验和执行，其模型、endpoint、tokenizer、身份、校准合同由部署固定。utility 保持 Concept 范围；trust 要求二元事实证据及精确 actor/predicate/context。提案及治理沿用原生 Rust 能力，没有模型 trust setter，bootstrap 不授予 `manage_trust`。自动应用须显式校准和启用。未知测量不记零，机制测试不证明学习收益。

## 学习与 MIB

独立 feature `learning = ["anda_brain/learning"]` 不改变 `mib` 含义。以 `--features learning` 编译后，可通过同一运行时配置安装 `workflow_http_v1`。[学习示例](../anda_bot/assets/brain-learning.example.json) 默认关闭所有自动开关及校准批准；部署前替换身份、端点、固定摘要和校准材料，数值仅是模板，不是生产校准默认值。

原生适配器仅支持 `tool_workflow.precondition.v1`，要求真实 inspect/prepare/commit、隔离 reset、稳定请求、当前 fence/期限、状态查询、取消、完整日志及测量预算。启动探测独立 executor/observer，冻结 cohort 来自注册 source。复用 Brain 的 Decision→Attempt→独立 Outcome→Trial/Evaluation、归档、复核、安全撤销；不把任意 Bot shell 或 IM 工具自动提升为学习执行器。

MIB 宿主继续如实声明 `persistent`/`no_memory`、`learning=false`、`independent_observer=false` 和计量不完整。启用 Cargo feature 不改变实验条件。normal/ungated 实测还需要实际业务服务、独立 instrument、完整计量、批准的冻结计划及明确费用预算。本次集成不安装生产凭据、不发送通知、不运行付费模型试验、不修改跨仓库 MIB。参见 [MIB 宿主](mib-integration_cn.md)及 [Brain 业务合同](https://github.com/ldclabs/anda-brain/blob/main/anda_brain/LEARNING_RUNTIME_cn.md)。
