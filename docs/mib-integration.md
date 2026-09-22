# MIB evaluation host

[中文版](mib-integration_cn.md)

## Run an isolated comparison

The product entry point is `anda memory evaluate` in a build with `mib`. It runs before production home/identity initialization and rejects `--home`. It never reads your normal memory or model credentials. Provide a separate `ModelConfig` with `api_key` empty and a named environment variable.

```sh
anda memory evaluate plan --model-config /absolute/path/model.json --track memory --output ./memory-plan.json
# Review the frozen plan and its limits before this explicit, potentially paid step:
anda memory evaluate run --plan ./memory-plan.json --output ./memory-run
anda memory evaluate report ./memory-run
```

`plan` calls no models; `report` only rebuilds saved results. `run` refuses existing output directories and mismatched plan, executable, model, dataset or prompt digests. Each case/repeat/arm has its own native run and namespace. The four frozen synthetic cases cover preferences, changed facts, names in different projects and missing information. Only `persistent` and `no_memory` are supported. Track `memory` fixes the business model/prompt outside the memory host; `agent` measures the existing bounded agent adapter, not the whole desktop daemon. Scores and expected answers do not enter observed training messages.

Output contains `plan.json`, `manifest.json`, `events.jsonl`, `cases/*.json`, `report.json` and `report.md`. Reports retain completed, invalid, not-started and unknown runs, valid paired denominators, final cumulative cost snapshots, cleanup and native capability descriptors. Rebuilding a report recovers the latest complete cost event after a torn final append. It never retries or re-executes a case. Ctrl+C or the evaluation deadline requests local host shutdown; in-flight results stay unknown, and external provider settlement may remain unmeasured.

The plan bounds protocol operations, time, output tokens and local Recall packet/planner context. These are not a hard total currency cap or a bound on every provider retry. `accounting_complete=false`, unknown cost is null, and uncertainty is `not_estimated`. The shipped template is a small synthetic comparison, not a claim of general improvement or calibrated business learning. The repository tests use mechanism fixtures; no paid trial was run for this implementation.

Business deployment uses a different [reviewable workflow contract](../anda_bot/assets/memory/workflow-template.json). Runtime readiness distinguishes compiled code, installed services, matching identities, reviewed calibration and explicit automation approval. A ready isolated learning flow is not permission to deploy an action against a business system. Keep the native registration/grants, deployment approval and rollback evidence separate.

Build with `env -u LIBRARY_PATH cargo build -p anda_bot --features mib --bin anda`.
Run `anda mib --model-config /absolute/path/model.json --api-key-env MIB_MODEL_API_KEY --listen 127.0.0.1:8043 --memory-mode persistent`.
`model.json` is an Anda Engine `ModelConfig` object; use an empty `api_key` with the named environment variable. Never commit real credentials to the repository. `--home` is rejected. Normal production home, identity, IM, cron, browser, automatic updates and desktop session recovery are never initialized by this entry point.

- Track B: `/mib-agent/v0.1/{operation}`, protocol `mib-agent/0.1`, implements describe/reset/observe/respond/act/maintain/session_boundary/close. Bot reuses its existing system instruction renderer and executes a bounded business model step; MIB executes the supplied native task tool calls. This profile does not claim coverage of the complete desktop daemon.
- Track A: `/mib-memory/v0.1/{operation}`, protocol `mib-memory-backend/0.1`, implements describe/reset/observe/retrieve/maintain/session_boundary/close. MIB keeps its own business agent, model, prompt, tools and sampling. Retrieve returns Brain Recall context, not a final task answer or source graph citation.
- Agent and memory backend use separate run namespaces. Observe waits for actual Formation completion; a Maintenance failure or timeout invalidates the run. Completed only means that the model workflow has ended. Duplicate requests return the original result, conflicting content is rejected, and an existing `run_id` cannot be reused through reset. Accepted work remains managed by the host after an HTTP disconnect; close interrupts waits and performs cleanup, and idle deadlines reclaim runs.
- `--memory-mode no-memory` skips long-term observation and recall, while preserving current-task tool replies until the task ends. Persistent mode retains graph and Notes while clearing transient context at session boundaries. No ungated learning condition is advertised yet.
- In every successful response, `body.costs` is a cumulative run snapshot with `cost_scope` set to `cumulative_run`. Use the last snapshot per run; do not sum responses. Unknown token counts and durations remain null. Provider-internal retries and observer usage may still be unmeasured, so `accounting_complete=false`. The business output cap is not a total budget for all model work.
- Idempotency is in-process. The descriptor exposes a host epoch. Process restart requires a fresh evaluation; it cannot resume old in-memory runs. Limits: 8 active runs, 1024 total run records, 10,000 request receipts/32 MiB per run. Capacity exhaustion is explicit. Default operation/idle deadlines: 180/900 seconds.

Detailed implementation, migration and evidence: [Brain MIB integration](https://github.com/ldclabs/anda-brain/blob/main/anda_brain/README.md#mib-integration). This development checkout uses the explicitly authorized sibling Brain patch for native product contracts; switch back to registry after the native release. Other core dependencies remain locked registry packages.

Validation: `env -u LIBRARY_PATH RUST_MIN_STACK=16777216 cargo test -p anda_bot --features mib mib::`. The ignored `serve_local_transport_fixture` test is an explicit local model fixture for HTTP pipeline verification, not an empirical benchmark or an improvement claim.

## Recall budgets and audit

Add both `--recall-max-tokens 4096 --recall-context-tokens 32768` to opt into
bounded Recall packets. The fixed tokenizer and both caps appear in the descriptor and budget
identity. Recall input is a cumulative normalized-planner budget; this is not
complete provider billing. Omitting both flags retains the older behavior.
The forced Space policy survives session boundaries and snapshot forks.
With budgets enabled, if the memory backend's `limit_chars` cannot accommodate
the entire packet, the packet is omitted with `truncated:true`; neither JSON nor
warnings are cut off.

The evaluator-only `learning_audit` extension accepts an empty body in either
MIB protocol. It enumerates native Skills, revisions, Decisions, Attempts,
Outcomes, Trials and Evaluations without a model call, task finalization or
clock advance. The returned procedure digest excludes Conversations, time and
MnemonicState. Counts are only lower bounds when `complete=false` (256 rows per
kind and 4 MiB total projection bounds); a zero count does not prove absence.
Inventory never confers applicability; `recommendation_allowed` is null.
Never feed audit results back through Observe.

The optional `mib.learning_longitudinal.v1` descriptor explicitly reports
`persistent` or `no_memory`. Persistent mode is not a configured `normal`
learning arm. Neither native comparison/review nor isolated ungated application
is currently bound, and the MIB runner must reject those unsupported conditions.
Model configuration alone cannot supply an independent observer or authorize a
candidate program. Full costs remain incomplete.

See [Brain MIB implementation and remaining work](https://github.com/ldclabs/anda-brain/blob/main/anda_brain/README.md#mib-integration).

The independent `learning` Cargo feature installs production Brain workflow bindings through `brain.runtime_config`; it does not change this isolated MIB host's descriptor. Use `--features mib,learning` for combined host mechanism tests. Native learning needs independently deployed executor/observer/source services and approved calibration. No normal/ungated MIB condition is advertised without complete accounting and provenance; changing capability booleans alone is insufficient. See [Brain runtime integration](brain-integration.md).
