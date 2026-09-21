# Brain 运行时集成

Anda Bot 0.13 内嵌 Brain 0.12。不配置运行时时，普通 Formation、Recall、Maintenance 继续工作。持久待办、独立观察、语义求值、utility、trust 和原生学习分别配置；编译、安装绑定、启用自动运行、机制测试与真实业务收益是不同状态。

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

模型通过 `tools_select` 发现 `brain_attention`、`brain_respond`、`brain_runtime_status`。回答工具严格 schema 要求同时传 `answer` 和 `statement`，未使用的字段为 null，校验后转换成 Brain 原生请求。外部不可信 IM 用户不能使用这些工具。HTTP/WS 转发调用者自己的 bearer；Engine 工具从真实认证上下文映射显式配置的原生主体。匿名、local、public 模式不绕过运行时认证；用户队列及审计清单相互隔离。

分页读取不领取工作，`complete` 只表示分页结束。cursor 过期或失效时从首页刷新。回答 URL 使用返回的十六进制 `id`，不是 `wake/v1/...`。浏览器先保存事件键和正文再发送，丢失确认或重载后可原样重试；TUI 重试须保留相同事件键和正文。同键异文返回冲突。澄清回答不授予权限，自述不是独立 Outcome。

`attention_inbox_v1` 使用 Brain 原生持久投递和 Attempt 身份，重启后原生记录仍是工作真相。Bot 只保存图谱外的会话、渠道、接收者、路由关联，保留 `(reply_target, thread)`。现有 IM 渠道没有启用自动 Brain 分派：`send -> Result<()>` 无法证明投递或权威 NotStarted。不支持的 adapter 在启动时拒绝，普通 IM 聊天沿用既有权限和路由。不会把 Watch 包装成无人值守 cron 或 full-access goal。

## Recall 与 Formation 收据

Brain HTTP KIP 使用应用参数：`command`，或带 `execution` 的 `operations`，以及 `parameters`、`read`、顶层 `dry_run`。响应仍为 KIP 2.0，同时检查请求级和 operation 级状态。原生 Rust 信封在统一边界转换；无法表达的请求元数据、preconditions、deadline、extensions 明确拒绝，不静默丢弃。

`recall_memory` 支持可空 `budget`，包含 Brain 固定 tokenizer、`max_tokens`、`context_tokens`。null 保持兼容。保留完整记忆包和不足状态，不通过 artifacts 或收据追加记忆。失败有明确错误标记，保留直接及嵌套计量，嵌套值不重复加总；缺失账单仍是不完整。

现有 DB 对象存储中的 `bot-brain/v1/` 宿主日志保留 Recall 交付身份、Brain conversation/receipt、Bot conversation/turn 及可明确对应的模型 tool-call ID。并行重复参数无法区分时不猜测关联。收据仅证明交付；普通检索不改变 utility、trust 或执行权限，legacy trace 不证明贡献。

Formation 在发送前持久化窗口，接受后保留 Brain conversation ID。`/brain formation <id>` 查询该任务的 submitted/working/completed/failed 状态，不用全局 high-water mark 推断完成。明确拒绝保留退避重试；失联或中断导致接受情况未知时，阻止该窗口盲重发并保留对账材料。服务器没有提交幂等键，因此不宣称 Formation 恰好一次。

## 独立观察与可选校准

`/outcomes` 原生路由及 typed Rust client 供独立认证的 instrument 使用，不注册普通模型工具，也不由 completion hook 替代观察者。原生 Action context/Decision 保留真实 `used_refs`、`applied_revisions` 和 Recall receipt；观察者通过 `OutcomeInput.utility` 提交原生单项贡献 witness。交付、使用、贡献可区分，无法归因的普通任务只保留审计，不伪造 trial 或涨分。

启动配置 `semantic`、`utility`、`trust` 直接交给 Brain 校验和执行，其模型、endpoint、tokenizer、身份、校准合同由部署固定。utility 保持 Concept 范围；trust 要求二元事实证据及精确 actor/predicate/context。提案及治理沿用原生 Rust 能力，没有模型 trust setter，bootstrap 不授予 `manage_trust`。自动应用须显式校准和启用。未知测量不记零，机制测试不证明学习收益。

## 学习与 MIB

独立 feature `learning = ["anda_brain/learning"]` 不改变 `mib` 含义。以 `--features learning` 编译后，可通过同一运行时配置安装 `workflow_http_v1`。[学习示例](../anda_bot/assets/brain-learning.example.json) 默认关闭所有自动开关及校准批准；部署前替换身份、端点、固定摘要和校准材料，数值仅是模板，不是生产校准默认值。

原生适配器仅支持 `tool_workflow.precondition.v1`，要求真实 inspect/prepare/commit、隔离 reset、稳定请求、当前 fence/期限、状态查询、取消、完整日志及测量预算。启动探测独立 executor/observer，冻结 cohort 来自注册 source。复用 Brain 的 Decision→Attempt→独立 Outcome→Trial/Evaluation、归档、复核、安全撤销；不把任意 Bot shell 或 IM 工具自动提升为学习执行器。

MIB 宿主继续如实声明 `persistent`/`no_memory`、`learning=false`、`independent_observer=false` 和计量不完整。启用 Cargo feature 不改变实验条件。normal/ungated 实测还需要实际业务服务、独立 instrument、完整计量、批准的冻结计划及明确费用预算。本次集成不安装生产凭据、不发送通知、不运行付费模型试验、不修改跨仓库 MIB。参见 [MIB 宿主](mib-integration_cn.md)及 [Brain 业务合同](https://github.com/ldclabs/anda-brain/blob/main/anda_brain/LEARNING_RUNTIME_cn.md)。
