# Brain runtime integration

Anda Bot 0.13 embeds Brain 0.13 on KIP 2.0 (`cognitive-memory@2.0.0`) and uses its Memory Interface binding (`memory_basic`): Formation windows are observed intents with durable receipts, recall waits on the conversation's own receipts, and memory attention has a saved cursor. The ordinary Formation/Recall/Maintenance loop works without a runtime configuration. Durable inbox actions, independent observations, semantic attention, utility, trust and learning are separately configured capabilities. Compilation, installed bindings, enabled automation, mechanism tests and measured business improvement are different states.

## Start with ordinary memory

Run `anda memory guide` for an offline guide, or `anda memory` to check a running daemon. These commands do not call a model; the guide does not create a home or identity. The browser dashboard now opens **Memory** with an example, connection status, records and source quotes. **Explore relationships** preserves the graph view. TUI `/memory` and bare `/brain` show the same overview; explicit `/brain status` keeps its technical meaning.

Tell Anda a useful preference, wait for background processing, then use `/new` and ask about it. A reply saying “remembered” is not a storage receipt. Message badges and `/memory activity` report the specific conversation's processing; a completed Formation need not contain the fact you expected. Unknown acceptance is reconciled without resubmission. Source quotes link to the original message only when the durable mapping and content digests agree.

The owner can search, correct, stop using or delete supported records. Search is an explicit model operation with a complete Recall packet, warnings and default limits of 4,096 packet tokens and 32,768 planner input tokens; it has no automatic retry or total monetary cap. Corrections and removal show a revision-bound preview (ten minutes for native changes, fifteen for a misrecording report) before confirmation. An interrupted change is checked by its operation ID, not submitted as a new operation.

A correction asks which change it is. **I said it wrong** (`correct`) supersedes the owner's own claim for the same time; undo previews another conditional correction. **Things changed** (`world_change`) writes one new claim from now, and temporal succession ends the old value, which stays true for its time. **You recorded it wrong** (`misrecorded`) is recording repair, never a correction: the owner's report is sent as a Memory Interface `revise` with `change_kind: "misrecorded"` under the operation's key, Brain re-reads the original source and repairs the extraction, and the change stays `committing` until Brain has processed it (a retry replays the same receipt). A repair or erasure that cannot finish is shown as failed or blocked rather than pending.

Stopping use archives the previewed claims, cited inputs and recorded dependents. Deletion runs as a Memory Interface `forget` with `mode: "semantic"`: it purges that bounded native set, scrubs Brain's own Formation and Recall transcripts that quoted it, and keeps the ErasurePlan report (`completed`, `partial` when a surface could not be verified, `blocked` by a legal hold) with the confirmed change. A deletion the native product path already confirmed under an older receipt is only reconciled. Both persist source exclusions, fence in-flight processing and reset native/Bot Notes and processing history. Related source conversations stop contributing automatically, including their continuation chain. Other independent records, original Bot chats/files/logs/backups, already delivered model context and provider copies remain. Minimal source identifiers/digests are retained to prevent replay. Source gaps, protected dependents, retention holds or an overlarge closure refuse the operation. Restoring an old database backup without current exclusion metadata is outside this guarantee.

For a fresh conversation, use the browser composer's **Memory mode** control, or:

```sh
anda agent run --memory-mode no-store --prompt 'Help with this task without saving long-term memory'
anda agent run --memory-mode off --prompt 'Help using only this new conversation'
```

These modes constrain host-managed Brain/Notes, including implicit profile initialization and automatic Formation. Restricted modes reject unsupported nested agents, memory actions, bookmarks and cron changes. They do not make chat history, files, logs or provider processing incognito. Changing mode creates a new conversation without copying old context; existing conversations keep their persisted policy.

## Product API and development dependency

`/daemon/memory/v1` is an owner control plane using Bot `ToolResponse`, separate from native KIP and model tools. HTTP and WebSocket requests revalidate the original credential. Overview, records, search, change prepare/commit/status/discard, inbox setup and record subscriptions share the same service. Conversation activity additionally supports an authenticated caller's own specified conversation. Other records and counts are not exposed to that caller. Identifiers in product responses are strings.

Activity pages use submission time and a stable journal key, including after rebuilding an older index. Refresh from the first page if a saved cursor is rejected after upgrading. Startup and periodic reconciliation repair the index; routine polling reads changed and unfinished records. Source verification is cached only within one caller's request.

Record pages stop before exceeding their response budget and return a cursor for the first unreturned record. `response_size_limit` means more records are available, not that their source provenance is missing. Large source quotes are shortened with `text_truncated`, preserving record identity and the original claim labels. Watch listing accepts optional `cursor` and `limit` (1–50), returns active subscriptions with a caller-bound cursor, and omits cancelled history. The browser follows all pages before enabling new subscriptions.

Both ordinary and structured Recall share a reusable HTTP transport with automatic retries disabled. A failed response may follow accepted model work, so a new search is an explicit action. Change confirmation preserves revision/expiry errors; unresolved admissions are still checked against the native receipt before reporting success.

The DB/KIP/Nexus 0.14 stack and the matching Core/Engine 0.16 come from crates.io; until Brain 0.13 is published, `[patch.crates-io]` builds it from the sibling `anda-brain` checkout. `Cargo.lock` holds one type identity for each. A 0.12.0 database is KIP 1.x and migrates on first start. A database that a development build wrote under the 2.1.0 draft (Brain 0.12.1 / Nexus 0.13.4) is not opened in place; migrate it to a new database first (see Brain's draft-Space migration) and keep the old one read-only for rollback. Do not restore a pre-change database as a software rollback without preserving current source exclusions.

Deletion and recording repair validate a preview before admitting work. An expired or discarded preview must be reviewed again; deletion also checks that its complete native scope has not changed. Admitted work retains its operation identity for recovery. A completed recording repair clears Bot Notes even though it does not advance Brain's product epoch. Formation retries reuse the source time saved for the original attempt, keeping the staged source and input digest stable when the runner supplies a later time.

## Set up questions only when needed

Ordinary memory works without an inbox. Select **Set up a memory inbox** in Memory, or run `anda memory inbox setup` to preview the owner-only configuration. Confirm the shown digest with `--apply`; the wizard makes private backups, checks the original configuration digest and writes a minimal managed runtime file. Environment overrides and custom runtime files are preserved and require an explicit manual merge. Configuration has no effect until you run `anda restart`; verify access afterward. A config rollback does not revoke native grants.

After setup, **Ask me when this memory changes** creates an actual recipient-owned Watch. The controller receives an archive grant scoped only to that new Watch so cancellation needs no broad management rights. Cancellation archives the native Watch; retry/restart does not re-arm it or restore a revoked cancellation grant. Existing delivered questions are not retracted by cancelling future observation. Questions and delivery receipts are different native inbox items.

Use `/memory inbox`, `/memory next`, `/memory answer <number> <text>` and `/memory retry <number>` in the TUI. The terminal and browser save the original response/event key before sending. A lost response can be retried with the same data; changed pending text is rejected. An acknowledgement means the answer was accepted, not that a business task succeeded.

Read the inbox with `anda memory inbox`. Follow the printed `anda memory inbox --cursor <next_cursor>` command for another page. With `--json`, the cursor is in `result.next_cursor`.


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

Browser tokens exported before Brain audience support keep working: the embedded Brain accepts a trusted user's token that names no audience as addressed to its one Space, and still verifies its signature, expiry, subject and scope. New tokens target `anda_bot` explicitly; re-exporting with `anda browser token` is optional.

Open **Attention inbox** in the browser Brain view. Use `/brain inbox`, `/brain status` or `/brain next <cursor>` in the TUI. `/brain answer <id> <event_key> <answer>` sends a clarification; `/brain statement <id> <event_key> <statement>` records an attributed statement. Use `/brain help` for the command forms.

Model tools `brain_attention`, `brain_respond`, `brain_runtime_status` and `brain_feedback` are available through `tools_select`. `brain_attention` also returns `memory_attention`: the due commitments and fired watches raised since the caller last looked, with a saved per-caller cursor, whether or not the inbox is configured. `brain_runtime_status` adds the Memory Interface descriptor Brain advertises. `brain_feedback` records the model's own report about a decision or attempt as attributed agent evidence (`feedback`); it is never a grade, an outcome or a verified fact, and restricted memory modes disable it. The response tool's strict schema requires `answer` and `statement`, with the unused field set to null; it sends Brain's native tagged response after validation. External IM users remain excluded from these tools. HTTP and WS use the caller's own bearer. Engine tools map the authenticated caller to the explicitly configured native principal. Anonymous/local/public access never bypasses runtime authentication. Recipient queues and audit inventories are not shared across users.

Reading a page does not claim work. `complete` ends a snapshot page walk, not a task. On an expired or invalid cursor, refresh from the first page. Response URLs use the returned hex `id`, not `wake/v1/...`. The browser saves each pending response before sending and retries the same event/text after a lost acknowledgement or reload. The product TUI outbox preserves that event key and text; the advanced `/brain answer` command still requires the caller to supply them. Same-key changed text is a conflict. A clarification answer is data, not authority; a statement is not an independent Outcome.

`attention_inbox_v1` uses Brain's native persistent delivery and Attempt identity. Its native records remain the work truth after restart. Bot stores only off-graph conversation/channel/recipient/route associations; `(reply_target, thread)` is retained. No existing IM adapter is enabled as an automatic Brain action: `send -> Result<()>` cannot prove delivery or authoritative NotStarted. Unsupported adapter names fail startup; ordinary IM chat continues to use its existing permission and routing rules. No Watch is converted to unattended cron or a full-access goal.

## Recall and Formation receipts

Brain HTTP KIP bodies use the application contract (`command`, or `operations` with `execution`, plus `parameters`, `read`, and top-level `dry_run`). Responses remain KIP 2.0 envelopes. Both response status levels are checked. The native Rust envelope is converted at one boundary; unsupported native request metadata, preconditions, deadlines and extensions are rejected rather than dropped.

`recall_memory` accepts optional `budget` with the pinned Brain tokenizer, `max_tokens` and `context_tokens`. Null keeps legacy behavior. Complete packets are preserved, including insufficient coverage; no additional memory is injected through artifacts or receipts. Failed recalls are marked as errors and their measured direct/nested usage is retained without summing nested totals twice. Missing billing information remains incomplete.

Before recalling, `recall_memory` waits up to 20 seconds for the current conversation's own outstanding Formation receipts (the Memory Interface processing barrier). Receipts still being processed or failed are named in a note ahead of the answer (budgeted packets are left unchanged); receipts are cleared from the conversation's session only after a successful recall accounted for them. A new session starts from the memory attention raised since the owner's last session, as a data section of the system context; external IM senders never receive it. This is an `attention` read with no model call: a full `resume` recall needs a query and runs the Recall model pass, so it is not run automatically per session.

A host journal under `bot-brain/v1/` in the existing DB object store retains Recall delivery identity, Brain conversation/receipt, Bot conversation/turn and an unambiguous model tool-call ID. Parallel identical calls remain unassigned rather than guessed. Receipts prove delivery only. Ordinary retrieval does not write utility, trust or execution authority; legacy trace receipts do not imply contribution.

Formation submission windows are recorded before sending. Each window is staged as a source under the Bot's own product identity (so product deletion keeps covering it) with its Formation context, then sent as a Memory Interface `observe` whose idempotency key names the conversation, window and attempt, with the Bot conversation as its source-order stream. The window row keeps the receipt and the Brain conversation it started; its state follows the receipt's phase (recorded → accepted, processed/available → completed, failed → failed). An interrupted submission resends under the same key and replays the same receipt instead of forming the window twice; a longer window after an interruption, or a definite rejection, moves to the next attempt's key. A native Formation failure after acceptance remains Brain's to retry. `/brain formation <id>` still reads a specific Brain conversation, and the global formation high-water mark is not a completion receipt. Without the embedded host (an HTTP-only client) the older direct Formation path is used unchanged.

## Independent observations and optional calibration

The native `/outcomes` route and typed Rust client accept independently authenticated instruments. Ordinary model tools and completion hooks cannot submit Outcomes. Native Action context/Decision records carry actual `used_refs`, `applied_revisions` and Recall receipts; the observer's `OutcomeInput.utility` can carry the native single-contribution witness. Delivery, use and contribution remain distinct. Unattributable ordinary tasks stay audit-only, without fabricated trials or score updates.

Startup `semantic`, `utility` and `trust` fields go directly through Brain's validators and native runtimes. Their model/endpoint/tokenizer/identity/calibration contracts are pinned by deployment configuration. Utility remains Concept-scoped; trust requires binary factual evidence and exact actor/predicate/context. Proposals and governance remain native Rust capabilities, with no model trust setter and no bootstrap `manage_trust`. Automatic application needs explicit calibrated configuration. Unknown measurements are not zero, and mechanism fixtures do not establish useful learning.

## Learning and MIB

`learning = ["anda_brain/learning"]` is independent of `mib`. Build with `--features learning` to install `workflow_http_v1` through the same runtime configuration. [The learning example](../anda_bot/assets/brain-learning.example.json) is a template with all automatic switches and calibration approval off. Replace identities, endpoints, pins and calibration material before deployment; its thresholds are illustrative, not calibrated defaults.

The compiled native adapter supports only `tool_workflow.precondition.v1`: real inspect/prepare/commit, isolated reset, stable requests, current fences/deadlines, status lookup, cancellation, complete logs and measured budgets. It probes separate executor and observer services and obtains frozen cohorts from the registered source. It uses Brain's existing Decision→Attempt→independent Outcome→Trial/Evaluation controller, archival, review and safety recovery. It does not turn arbitrary Bot shell or IM tools into a learning executor.

The MIB host still advertises `persistent`/`no_memory`, `learning=false`, `independent_observer=false`, and incomplete accounting. Enabling the Cargo feature does not silently change those experimental conditions. Normal/ungated provider trials require their real business services, independent instrumentation, full accounting, approved frozen plans and an explicit evaluation budget. No production credentials, notifications, paid model trials or cross-repository MIB changes are supplied by this integration. See [MIB host](mib-integration.md) and [Brain's business contract](https://github.com/ldclabs/anda-brain/blob/main/anda_brain/LEARNING_RUNTIME.md).
