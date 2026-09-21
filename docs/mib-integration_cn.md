# MIB 评测宿主

[English](mib-integration.md)

使用 `env -u LIBRARY_PATH cargo build -p anda_bot --features mib --bin anda` 编译。
通过 `anda mib --model-config /absolute/path/model.json --api-key-env MIB_MODEL_API_KEY --listen 127.0.0.1:8043 --memory-mode persistent` 启动。
`model.json` 是 Anda Engine 的 `ModelConfig` 对象；使用指定环境变量提供凭证时，将 `api_key` 留空。不要把真实凭证提交到仓库。入口拒绝 `--home`，不会初始化生产 home、身份、IM、cron、浏览器、自动更新和桌面会话恢复。

- Track B：`/mib-agent/v0.1/{operation}`，协议为 `mib-agent/0.1`，实现 describe/reset/observe/respond/act/maintain/session_boundary/close。Bot 复用现有系统指令渲染器，执行有界的业务模型步骤；MIB 执行提供的原生任务工具调用。该运行模式不宣称覆盖完整的桌面 daemon。
- Track A：`/mib-memory/v0.1/{operation}`，协议为 `mib-memory-backend/0.1`，实现 describe/reset/observe/retrieve/maintain/session_boundary/close。MIB 保留自己的业务 Agent、模型、提示词、工具和采样设置。Retrieve 返回 Brain Recall 上下文，不是最终任务答案或源图谱引用。
- Agent 与 memory backend 使用不同 run 命名空间。Observe 等待真实 Formation 完成，Maintenance 失败或超时使运行无效；Completed 只说明模型流程结束。重复请求返回原结果，内容冲突拒绝，已有 `run_id` 不允许通过 reset 重用。HTTP 断连后已接受工作仍由宿主管理，close 中断等待并清理，空闲期限会回收运行。
- `--memory-mode no-memory` 跳过长期观察和召回，但保留当前任务的工具回复，直到任务结束。Persistent 模式保留图谱和 Notes，并在会话边界清除临时上下文。目前尚未声明支持无门槛学习条件。
- 所有成功响应的 `body.costs` 都是累计 run 快照，`cost_scope` 为 `cumulative_run`；每 run 取最后一份，不能逐响应求和。未知 token 数和耗时保留 null，provider 内部重试和 observer 用量仍可能缺测，因此 `accounting_complete=false`。业务输出上限不等于全部模型工作的总预算。
- 幂等性仅在进程内有效。descriptor 暴露宿主 epoch；进程重启后必须开始新的评测，不能恢复旧的内存运行。容量限制：8 个活跃 run、1024 条 run 记录，每个 run 最多 10,000 条请求收据及 32 MiB。容量耗尽时会明确报错。默认操作超时为 180 秒，空闲期限为 900 秒。

详细实现、迁移说明和证据见 [Brain MIB 集成](https://github.com/ldclabs/anda-brain/blob/main/anda_brain/README.md#mib-integration)。源码构建使用 Brain 和其他核心依赖锁定的 registry 版本。仅在本地开发 Brain 时，才取消 `Cargo.toml` 中同级 `anda_brain` patch 的注释。

验证：`env -u LIBRARY_PATH RUST_MIN_STACK=16777216 cargo test -p anda_bot --features mib mib::`。被忽略的 `serve_local_transport_fixture` 测试是用于 HTTP 流水线验证、需显式运行的本地模型测试夹具，不是实测基准或改进声明。

## Recall 预算与审计

同时添加 `--recall-max-tokens 4096 --recall-context-tokens 32768`，启用有界 Recall 记忆包。
固定 tokenizer 和两个上限进入 descriptor 与预算身份。Recall 输入采用累计的规范化规划器预算，
不等于完整 provider 账单。不提供这两个参数时保持旧行为。强制 Space 策略跨会话边界和快照分叉仍生效。
启用预算后，memory backend 的 `limit_chars` 若无法容纳整个记忆包，将整体省略并返回
`truncated:true`，不会切断 JSON 或警告。

评测侧专用的 `learning_audit` 扩展在两个 MIB 协议中均接收空 body；它不调用模型、结束业务任务或推进时钟。
它枚举原生 Skills、修订、Decisions、Attempts、Outcomes、Trials 和 Evaluations。
返回的程序摘要排除 Conversations、时间和 MnemonicState。超过每类 256 项或总投影 4 MiB 时，
`complete=false`，计数仅为下界，不能据零证明不存在。清单不验证当前应用条件，
`recommendation_allowed` 为 null；不要把审计结果经 Observe 注回模型。

可选的 `mib.learning_longitudinal.v1` descriptor 明确声明 `persistent` 或 `no_memory`。
Persistent 模式不是已配置的 `normal` 学习组。目前尚未绑定原生比较/复核及隔离无门槛应用；
MIB runner 必须拒绝这些不支持的条件，不能将现有持久组当作已接线的正常学习组或无门槛组。
模型配置本身不能提供独立观察者或授权候选程序；完整成本计量仍不完备。

参见 [Brain MIB 实现与剩余工作](https://github.com/ldclabs/anda-brain/blob/main/anda_brain/README.md#mib-integration)。

独立的 `learning` Cargo feature 可在生产宿主通过 `brain.runtime_config` 安装 Brain 原生业务绑定，不改变隔离 MIB 宿主的 descriptor。组合宿主机制测试使用 `--features mib,learning`；原生学习仍需独立部署的 executor/observer/source 服务及校准批准。在成本计量和来源追踪完整之前，不声明支持 normal/ungated MIB 条件，不能只修改 capability 布尔值。参见 [Brain 运行时集成](brain-integration_cn.md)。
