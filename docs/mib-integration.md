# MIB evaluation host / MIB 评测宿主

Build with `env -u LIBRARY_PATH cargo build -p anda_bot --features mib --bin anda`.
Run `anda mib --model-config /absolute/path/model.json --api-key-env MIB_MODEL_API_KEY --listen 127.0.0.1:8043 --memory-mode persistent`.
`model.json` is an Anda Engine `ModelConfig` object; use an empty `api_key` with the named environment variable. `--home` is rejected. Normal production home, identity, IM, cron, browser, automatic updates and session recovery are never initialized by this entry point.

使用 `mib` feature 编译后，通过上述命令启动。模型配置采用 Anda Engine `ModelConfig`，凭证可由指定环境变量提供；不要把真实凭证提交到仓库。入口拒绝 `--home`，不会启动生产 home、身份、IM、cron、浏览器、自动更新和桌面会话恢复。

- Track B: `/mib-agent/v0.1/{operation}`, protocol `mib-agent/0.1`, implements describe/reset/observe/respond/act/maintain/session_boundary/close. Bot reuses its existing system instruction renderer and executes a bounded business model step; MIB executes the supplied native task tool calls. This profile does not claim coverage of the complete desktop daemon.
- Track A: `/mib-memory/v0.1/{operation}`, protocol `mib-memory-backend/0.1`, implements describe/reset/observe/retrieve/maintain/session_boundary/close. MIB keeps its own business agent, model, prompt, tools and sampling. Retrieve returns Brain Recall context, not a final task answer or source graph citation.
- Agent 与 memory backend 使用不同 run 命名空间。Observe 等待真实 Formation 完成，Maintenance 失败或超时使运行无效；Completed 只说明模型流程结束。重复请求返回原结果，内容冲突拒绝，已有 run_id 不允许 reset 重用。HTTP 断连后已接受工作仍由宿主管理，close 中断等待并清理，空闲期限会回收运行。
- `--memory-mode no-memory` skips long-term observation and recall, while preserving current-task tool replies until the task ends. Persistent mode retains graph and Notes while clearing transient context at session boundaries. No ungated learning condition is advertised yet.
- 所有成功响应 `body.costs` 是累计 run 快照，`cost_scope` 为 `cumulative_run`；每 run 取最后一份，不能逐响应求和。未知 tokens/耗时保留 null，provider 内部重试和 observer 仍可能缺测，`accounting_complete=false`。业务输出上限不等于全部模型工作的总预算。
- Idempotency is in-process. The descriptor exposes a host epoch. Process restart requires a fresh evaluation; it cannot resume old in-memory runs. Limits: 8 active runs, 1024 total run records, 10,000 request receipts/32 MiB per run. Capacity exhaustion is explicit. Default operation/idle deadlines: 180/900 seconds.

Detailed implementation, migration and evidence: [Brain P2 plan](https://github.com/ldclabs/anda-brain/blob/main/anda_brain/README.md#mib-integration). Source builds require the sibling `anda`, `anda-db` and `anda-brain` path patches already declared in the workspace.

验证：`env -u LIBRARY_PATH RUST_MIN_STACK=16777216 cargo test -p anda_bot --features mib mib::`。The ignored `serve_local_transport_fixture` test is an explicit local model fixture for HTTP pipeline verification, not an empirical benchmark or an improvement claim.


## P6 budgets and audit / P6 预算与审计

Add both `--recall-max-tokens 4096 --recall-context-tokens 32768` to opt into
P5. The fixed tokenizer and both caps appear in the descriptor and budget
identity. Recall input is a cumulative normalized-planner budget; this is not
complete provider billing. Omitting both flags retains the older behavior.
The forced Space policy survives session boundaries and snapshot forks.

通过上述两个参数同时启用 P5，固定编码器和两个上限进入 descriptor 与预算身份。
累计 Recall 规划输入预算不等于完整 provider 账单。不提供参数时保持旧行为；强制
Space 策略跨会话与快照仍生效。启用预算后，memory backend 的 `limit_chars` 若无法
容纳整个记忆包，将整体省略并返回 `truncated:true`，不会切断 JSON 或警告。

The evaluator-only `learning_audit` extension accepts an empty body in either
MIB protocol. It enumerates native Skills, revisions, Decisions, Attempts,
Outcomes, Trials and Evaluations without a model call, task finalization or
clock advance. The returned procedure digest excludes Conversations, time and
MnemonicState. Counts are only lower bounds when `complete=false` (256 rows per
kind and 4 MiB total projection bounds). Inventory never confers applicability;
`recommendation_allowed` is null. Never feed audit results back through Observe.

评测侧专用的 `learning_audit` 在两个 MIB 协议中均接收空 body；它不调用模型、结束
业务任务或推进时钟。它枚举原生程序及学习记录，摘要排除 Conversation、时间和
MnemonicState。超过每类 256 项或总投影 4 MiB 时 `complete=false`，计数仅为下界，
不能据零证明不存在。清单不验证当前应用条件，`recommendation_allowed` 为 null；
不要把审计结果经 Observe 注回模型。

The optional `mib.learning_longitudinal.v1` descriptor explicitly reports
`persistent` or `no_memory`. Persistent mode is not a configured `normal`
learning arm. Neither native comparison/review nor isolated ungated application
is currently bound, and the P6 runner must reject those unsupported conditions.
Model configuration alone cannot supply an independent observer or authorize a
candidate program. Full costs remain incomplete.

该扩展明确声明 `persistent` 或 `no_memory`。目前尚未绑定原生比较/复核及隔离无门槛
应用；P6 runner 必须拒绝将现有持久组当作已接线的正常学习组或无门槛组。模型配置
本身不能提供独立观察者或授权候选程序；完整成本仍未验收。

See [Brain P6 implementation and remaining work](https://github.com/ldclabs/anda-brain/blob/main/anda_brain/README.md#mib-integration).
