# Brain runtime integration

Anda Bot 0.12 embeds Brain 0.12. The ordinary Formation/Recall/Maintenance loop works without a runtime configuration. Durable inbox actions, independent observations, semantic attention, utility, trust and learning are separately configured capabilities. Compilation, installed bindings, enabled automation, mechanism tests and measured business improvement are different states.

## Startup

In `~/.anda/config.yaml` (or your selected home):

```yaml
brain:
  runtime_config: brain-runtime.yaml
```

Paths are relative to the configuration directory. `BRAIN_RUNTIME_CONFIG` overrides this field; an absolute path is accepted. Omit the setting for the existing memory behavior. Restart after changes. Runtime configuration is trusted deployment data, never a model argument, HTTP body or recalled memory.

Copy [the inbox example](../anda_bot/assets/brain-runtime.example.yaml), replace its owner CWT subject with the actual identity, and keep the configuration outside the repository. Brain validates the complete native configuration before loading any Space. Invalid adapters, unavailable environment secrets and conflicting principals fail startup. Bootstrap does not restore revoked grants on restart, re-arm Watches, grant `manage_trust`, or authorize Skill execution. A controller, recipient, observer and governor have distinct roles. A wildcard Bot CWT is not automatically any of these native principals.

The existing Brain background lifecycle discovers persistent work, runs one owner per shard and drains native writes at shutdown. Learning HTTP services must already be listening when configured: startup probes executor capabilities and observer identity before the Bot gateway starts. Do not configure a startup callback to the not-yet-listening gateway itself.

## Inbox and identities

Existing browser tokens exported before Brain audience support must be re-exported with `anda browser token` and replaced in the extension settings. New tokens target `anda_bot`; their subject, scope and bounded expiry are retained.

Open **Attention inbox** in the browser Brain view. Use `/brain inbox`, `/brain status` or `/brain next <cursor>` in the TUI. `/brain answer <id> <event_key> <answer>` sends a clarification; `/brain statement <id> <event_key> <statement>` records an attributed statement. Use `/brain help` for the command forms.

Model tools `brain_attention`, `brain_respond`, and `brain_runtime_status` are available through `tools_select`. The response tool's strict schema requires `answer` and `statement`, with the unused field set to null; it sends Brain's native tagged response after validation. External IM users remain excluded from these tools. HTTP and WS use the caller's own bearer. Engine tools map the authenticated caller to the explicitly configured native principal. Anonymous/local/public access never bypasses runtime authentication. Recipient queues and audit inventories are not shared across users.

Reading a page does not claim work. `complete` ends a snapshot page walk, not a task. On an expired or invalid cursor, refresh from the first page. Response URLs use the returned hex `id`, not `wake/v1/...`. The browser saves each pending response before sending and retries the same event/text after a lost acknowledgement or reload. A TUI retry must keep the same event key and text. Same-key changed text is a conflict. A clarification answer is data, not authority; a statement is not an independent Outcome.

`attention_inbox_v1` uses Brain's native persistent delivery and Attempt identity. Its native records remain the work truth after restart. Bot stores only off-graph conversation/channel/recipient/route associations; `(reply_target, thread)` is retained. No existing IM adapter is enabled as an automatic Brain action: `send -> Result<()>` cannot prove delivery or authoritative NotStarted. Unsupported adapter names fail startup; ordinary IM chat continues to use its existing permission and routing rules. No Watch is converted to unattended cron or a full-access goal.

## Recall and Formation receipts

Brain HTTP KIP bodies use the application contract (`command`, or `operations` with `execution`, plus `parameters`, `read`, and top-level `dry_run`). Responses remain KIP 2.0 envelopes. Both response status levels are checked. The native Rust envelope is converted at one boundary; unsupported native request metadata, preconditions, deadlines and extensions are rejected rather than dropped.

`recall_memory` accepts optional `budget` with the pinned Brain tokenizer, `max_tokens` and `context_tokens`. Null keeps legacy behavior. Complete packets are preserved, including insufficient coverage; no additional memory is injected through artifacts or receipts. Failed recalls are marked as errors and their measured direct/nested usage is retained without summing nested totals twice. Missing billing information remains incomplete.

A host journal under `bot-brain/v1/` in the existing DB object store retains Recall delivery identity, Brain conversation/receipt, Bot conversation/turn and an unambiguous model tool-call ID. Parallel identical calls remain unassigned rather than guessed. Receipts prove delivery only. Ordinary retrieval does not write utility, trust or execution authority; legacy trace receipts do not imply contribution.

Formation submission windows are recorded before sending. Accepted Brain conversation IDs are retained, and `/brain formation <id>` reads that specific conversation's submitted/working/completed/failed state. The global formation high-water mark is not a completion receipt. A rejected request can retry with backoff; unknown acceptance after transport loss or process interruption blocks that window for reconciliation instead of blindly resending. There is no server-side submission idempotency key, so this integration does not claim exactly-once Formation.

## Independent observations and optional calibration

The native `/outcomes` route and typed Rust client accept independently authenticated instruments. Ordinary model tools and completion hooks cannot submit Outcomes. Native Action context/Decision records carry actual `used_refs`, `applied_revisions` and Recall receipts; the observer's `OutcomeInput.utility` can carry the native single-contribution witness. Delivery, use and contribution remain distinct. Unattributable ordinary tasks stay audit-only, without fabricated trials or score updates.

Startup `semantic`, `utility` and `trust` fields go directly through Brain's validators and native runtimes. Their model/endpoint/tokenizer/identity/calibration contracts are pinned by deployment configuration. Utility remains Concept-scoped; trust requires binary factual evidence and exact actor/predicate/context. Proposals and governance remain native Rust capabilities, with no model trust setter and no bootstrap `manage_trust`. Automatic application needs explicit calibrated configuration. Unknown measurements are not zero, and mechanism fixtures do not establish useful learning.

## Learning and MIB

`learning = ["anda_brain/learning"]` is independent of `mib`. Build with `--features learning` to install `workflow_http_v1` through the same runtime configuration. [The learning example](../anda_bot/assets/brain-learning.example.json) is a template with all automatic switches and calibration approval off. Replace identities, endpoints, pins and calibration material before deployment; its thresholds are illustrative, not calibrated defaults.

The compiled native adapter supports only `tool_workflow.precondition.v1`: real inspect/prepare/commit, isolated reset, stable requests, current fences/deadlines, status lookup, cancellation, complete logs and measured budgets. It probes separate executor and observer services and obtains frozen cohorts from the registered source. It uses Brain's existing Decision→Attempt→independent Outcome→Trial/Evaluation controller, archival, review and safety recovery. It does not turn arbitrary Bot shell or IM tools into a learning executor.

The MIB host still advertises `persistent`/`no_memory`, `learning=false`, `independent_observer=false`, and incomplete accounting. Enabling the Cargo feature does not silently change those experimental conditions. Normal/ungated provider trials require their real business services, independent instrumentation, full accounting, approved frozen plans and an explicit evaluation budget. No production credentials, notifications, paid model trials or cross-repository MIB changes are supplied by this integration. See [MIB host](mib-integration.md) and [Brain's business contract](https://github.com/ldclabs/anda-brain/blob/main/anda_brain/LEARNING_RUNTIME.md).
