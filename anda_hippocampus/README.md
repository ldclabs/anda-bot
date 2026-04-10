# Anda Hippocampus — Technical Documentation

A dedicated LLM-powered memory management service that maintains a persistent **Cognitive Nexus** (Knowledge Graph) on behalf of business AI agents via [KIP (Knowledge Interaction Protocol)](https://github.com/ldclabs/KIP).

Business agents interact entirely through natural language and a REST API — no KIP knowledge required.

## Architecture

```
┌─────────────────────┐
│   Business Agent    │  ← Focuses on business logic & user interaction
│  (No KIP knowledge) │    Only speaks natural language
└────────┬────────────┘
         │ Natural Language / REST API
         ▼
┌─────────────────────┐
│    Hippocampus      │  ← The ONLY layer that understands KIP
│   (LLM + KIP)       │    Three agents: Formation / Recall / Maintenance
└────────┬────────────┘
         │ KIP (KQL / KML / META)
         ▼
┌─────────────────────┐
│  Cognitive Nexus    │  ← Persistent Knowledge Graph (backed by AndaDB)
│  (Knowledge Graph)  │
└─────────────────────┘
```

## Features

- **Zero KIP knowledge required** — Business agents interact through natural language and a simple REST API.
- **Persistent, structured memory** — Facts, preferences, relationships, events, and patterns encoded into a knowledge graph.
- **Three operational modes** — Formation (encoding), Recall (retrieval), and Maintenance (consolidation & pruning).
- **Multi-space isolation** — Each space has its own independent database, knowledge graph, and conversation history.
- **Triple serialization** — Supports JSON, CBOR, and Markdown for request/response payloads (negotiated via `Content-Type` / `Accept` headers).
- **Pluggable storage backends** — Local filesystem, AWS S3, or in-memory (for development/testing).

## Agents

### Formation — Memory Encoding (`formation_memory`)

Receives conversation messages and encodes them into structured memory within the Cognitive Nexus via KIP.

**System prompt:** [HippocampusFormation.md](https://github.com/ldclabs/anda-hippocampus/blob/main/anda_hippocampus/assets/HippocampusFormation.md)

**Processing pipeline:**
1. Receives `FormationInput` (messages + optional context + timestamp).
2. Creates a tracked `Conversation` record (status: `Submitted` → `Working` → `Completed` | `Failed`).
3. LLM analyzes messages, extracting three types of memory:
   - **Episodic memory** — Events with timestamps, participants, outcomes
   - **Semantic memory** — Stable facts, preferences, relationships, domain knowledge
   - **Cognitive memory** — Behavioral patterns, decision criteria, communication style
4. Deduplicates against existing knowledge (SEARCH before CREATE).
5. Encodes structured memory into the Cognitive Nexus via `execute_kip` tool.

**Key behaviors:**
- Sequential processing with automatic queue draining — new conversations are picked up after the current one completes.
- Atomic single-conversation processing via `processing_conversation` flag.
- Schema auto-evolution — defines new concept types/predicates when needed.

### Recall — Memory Retrieval (`recall_memory`)

Translates natural language queries into knowledge graph lookups and returns synthesized answers.

**System prompt:** [HippocampusRecall.md](https://github.com/ldclabs/anda-hippocampus/blob/main/anda_hippocampus/assets/HippocampusRecall.md)

**Processing pipeline:**
1. Receives `RecallInput` (query + optional context).
2. Analyzes query intent (entity lookup, relationship traversal, attribute query, event recall, pattern detection, etc.).
3. Grounds entities to actual graph nodes (resolves ambiguity).
4. Executes structured KQL queries via read-only memory tools + conversation search.
5. Iterative deepening — follows up with additional queries if needed (max 5 rounds).
6. Synthesizes results into a coherent natural language answer.

**Available tools:**
- `MemoryReadonly` — Read-only access to the knowledge graph

### Maintenance — Memory Metabolism (`maintenance_memory`)

Consolidates, prunes, and optimizes the knowledge graph during scheduled or on-demand cycles.

**System prompt:** [HippocampusMaintenance.md](https://github.com/ldclabs/anda-hippocampus/blob/main/anda_hippocampus/assets/HippocampusMaintenance.md)

**Processing phases (full scope):**
1. **Assessment** — Audit memory health (read-only): `DESCRIBE PRIMER`, pending SleepTasks, unsorted items, orphans, stale events.
2. **SleepTask Processing** — Handle queued actions: `consolidate_to_semantic`, `archive`, `merge_duplicates`, `reclassify`, `review`.
3. **Unsorted Inbox** — Reclassify items to appropriate topic domains.
4. **Stale Event Consolidation** — Extract semantic knowledge from old events (configurable threshold), create linked Preference/Fact nodes.
5. **Duplicate Merging** — Find and merge similar concepts, updating all propositions.
6. **Orphan Cleanup** — Assign domain-less concepts to appropriate domains.
7. **Confidence Decay** — Age facts by reducing confidence scores (`confidence * decay_factor`).

**Key behaviors:**
- Single-execution guard — only one maintenance cycle can run at a time per space.
- Non-destructive principle — archives before deleting, decays confidence rather than removing.
- Async execution — returns immediately with conversation ID; actual processing in background.

## API Endpoints

Detailed API docs (with TypeScript request/response types):
- English: [API.md](https://github.com/ldclabs/anda-hippocampus/blob/main/anda_hippocampus/API.md)
- 中文: [API_cn.md](https://github.com/ldclabs/anda-hippocampus/blob/main/anda_hippocampus/API_cn.md)
- Agent Skill: [SKILL.md](https://github.com/ldclabs/anda-hippocampus/blob/main/anda_hippocampus/SKILL.md)

| Method  | Path                                             | Description                                                                   | Auth Scope                   |
| ------- | ------------------------------------------------ | ----------------------------------------------------------------------------- | ---------------------------- |
| `GET`   | `/`                                              | Anda Hippocampus website                                                      | —                            |
| `GET`   | `/favicon.ico`                                   | Favicon                                                                       | —                            |
| `GET`   | `/apple-touch-icon.webp`                         | Apple touch icon                                                              | —                            |
| `GET`   | `/info`                                          | Service info (name, version, sharding)                                        | —                            |
| `GET`   | `/SKILL.md`                                      | Skill description (Markdown)                                                  | —                            |
| `GET`   | `/v1/{space_id}/info`                            | Get space status & statistics                                                 | `read` (CWT or space token)  |
| `GET`   | `/v1/{space_id}/formation_status`                | Get formation status (lightweight endpoint for monitoring formation progress) | `read` (CWT or space token)  |
| `POST`  | `/v1/{space_id}/formation`                       | Submit messages for memory encoding                                           | `write` (CWT or space token) |
| `POST`  | `/v1/{space_id}/recall`                          | Query memory with natural language                                            | `read` (CWT or space token)  |
| `POST`  | `/v1/{space_id}/maintenance`                     | Trigger maintenance cycle                                                     | `write` (CWT or space token) |
| `POST`  | `/v1/{space_id}/execute_kip_readonly`            | Execute a KIP request (read-only mode, suitable for queries)                  | `read` (CWT or space token)  |
| `GET`   | `/v1/{space_id}/conversations/{conversation_id}` | Get one conversation detail                                                   | `read` (CWT or space token)` |
| `GET`   | `/v1/{space_id}/conversations`                   | List conversations (cursor pagination)                                        | `read` (CWT or space token)  |
| `GET`   | `/v1/{space_id}/management/space_tokens`         | List space tokens                                                             | `read` (CWT)                 |
| `POST`  | `/v1/{space_id}/management/add_space_token`      | Add a space token                                                             | `write` (CWT)                |
| `POST`  | `/v1/{space_id}/management/revoke_space_token`   | Revoke a space token                                                          | `write` (CWT)                |
| `PATCH` | `/v1/{space_id}/management/update_space`         | Update space information (name, description, public/private)                  | `write` (CWT)                |
| `PATCH` | `/v1/{space_id}/management/restart_formation`    | Restart a formation task (manager only)                                       | `write` (CWT)                |
| `GET`   | `/v1/{space_id}/management/space_byok`           | Get BYOK (Bring Your Own Key) configuration (manager only)                    | `read` (CWT)                 |
| `PATCH` | `/v1/{space_id}/management/space_byok`           | Update BYOK (Bring Your Own Key) configuration (manager only)                 | `write` (CWT)                |
| `POST`  | `/admin/{space_id}/update_space_tier`            | Update a space tier (manager only)                                            | `write` (CWT)                |
| `POST`  | `/admin/create_space`                            | Create a new space (manager only)                                             | `write` (CWT)                |

### Content Negotiation

Triple serialization via `Content-Type` / `Accept` headers:

- `application/json` — JSON (default)
- `application/cbor` — CBOR (binary, more compact)
- `text/markdown` — Markdown (human-readable text)

All responses use an RPC envelope:

```json
{"result": { ... }, "error": null}
```

### Authentication

All endpoints (except `/`, `/info` and `/SKILL.md`) require a Bearer token:

```
Authorization: Bearer <base64_encoded_cose_sign1_token>
```

If `ED25519_PUBKEYS` is not provided (empty), authentication is effectively disabled: API requests are accepted without signature verification.

Token format: COSE Sign1 message signed with Ed25519 keys, containing CWT claims:

| Claim   | Purpose                                                 |
| ------- | ------------------------------------------------------- |
| `sub`   | Principal ID (who is making the request)                |
| `aud`   | Audience — the space ID being accessed (or `*` for any) |
| `scope` | Permission level: `read`, `write` (or `*` for any)      |

### POST /admin/create_space

Create a new isolated memory space. Requires manager principal.

**Request:**
```json
{
  "user": "<owner_principal_id>",
  "space_id": "my_space_001",
  "tier": 0
}
```

**Response:**
```json
{
  "result": {
    "space_id": "my_space_001",
    "owner": "owner_principal_id",
    ...
  }
}
```

### POST /v1/{space_id}/formation

Submit conversation messages for memory encoding. Processing is asynchronous — returns immediately while encoding continues in the background.

**Request:**
```json
{
  "messages": [
    {
      "role": "user",
      "content": "I prefer dark mode. My timezone is UTC+8.",
      "name": "Alice"
    },
    {
      "role": "assistant",
      "content": "Got it! I've noted your preferences."
    }
  ],
  "context": {
    "counterparty": "alice_principal_id",
    "agent": "customer_bot_001",
    "source": "source_123",
    "topic": "settings"
  },
  "timestamp": "2026-03-09T10:30:00Z"
}
```

| Field                  | Type        | Required | Description                                                     |
| ---------------------- | ----------- | -------- | --------------------------------------------------------------- |
| `messages`             | `Message[]` | Yes      | Conversation messages (`role`: `user` / `assistant` / `system`) |
| `context.counterparty` | `string`    | No       | User identifier                                                 |
| `context.agent`        | `string`    | No       | Calling agent identifier                                        |
| `context.source`       | `string`    | No       | Identifier of the source of the current interaction content     |
| `context.topic`        | `string`    | No       | Conversation topic                                              |
| `timestamp`            | `string`    | Yes      | ISO 8601 timestamp                                              |

**Response:**
```json
{
  "result": {
    "conversation": 1,
    ...
  }
}
```

### POST /v1/{space_id}/recall

Query memory with natural language. Returns a synthesized answer from the knowledge graph and conversation history.

**Request:**
```json
{
  "query": "What are Alice's preferences?",
  "context": {
    "counterparty": "alice_principal_id",
    "topic": "settings"
  }
}
```

| Field                  | Type     | Required | Description                   |
| ---------------------- | -------- | -------- | ----------------------------- |
| `query`                | `string` | Yes      | Natural language question     |
| `context.counterparty` | `string` | No       | User identifier               |
| `context.agent`        | `string` | No       | Calling agent identifier      |
| `context.topic`        | `string` | No       | Topic hint for disambiguation |

**Response:**
```json
{
  "result": {
    "content": "Alice prefers dark mode and operates in UTC+8 timezone.",
    ...
  }
}
```

### POST /v1/{space_id}/maintenance

Trigger a memory maintenance cycle. Runs asynchronously with single-execution guard.

**Request:**
```json
{
  "trigger": "on_demand",
  "scope": "full",
  "timestamp": "2026-03-10T03:00:00Z",
  "parameters": {
    "stale_event_threshold_days": 7,
    "confidence_decay_factor": 0.95,
    "unsorted_max_backlog": 20,
    "orphan_max_count": 10
  }
}
```

| Field                                   | Type     | Required | Description                                               |
| --------------------------------------- | -------- | -------- | --------------------------------------------------------- |
| `trigger`                               | `string` | Yes      | `scheduled` / `threshold` / `on_demand`                   |
| `scope`                                 | `string` | Yes      | `full` (all phases) / `quick` (assessment + urgent tasks) |
| `timestamp`                             | `string` | Yes      | ISO 8601 timestamp                                        |
| `parameters.stale_event_threshold_days` | `u32`    | No       | Days before events are considered stale (default: 7)      |
| `parameters.confidence_decay_factor`    | `f64`    | No       | Decay multiplier per cycle (default: 0.95)                |
| `parameters.unsorted_max_backlog`       | `u32`    | No       | Max unsorted items to process (default: 20)               |
| `parameters.orphan_max_count`           | `u32`    | No       | Max orphans to process (default: 10)                      |

**Response:**
```json
{
  "result": {
    "conversation": 8,
    ...
  }
}
```

### GET /v1/{space_id}/info

Get space statistics and health information.

**Response:**
```json
{
  "result": {
    "space_id": "my_space_001",
    "owner": "principal_id",
    "db_stats": { "total_items": 150, "total_bytes": 524288 },
    "concepts": 85,
    "propositions": 120,
    "conversations": 12,
    ...
  }
}
```

## Recall Function Definition

Business agents can register the Recall endpoint as an LLM tool/function call. See [RecallFunctionDefinition.json](https://github.com/ldclabs/anda-hippocampus/blob/main/anda_hippocampus/assets/RecallFunctionDefinition.json) for the OpenAI function-calling format.

## Memory Space Lifecycle

### Creation
1. Creates a new `AndaDB` instance.
2. Initializes `CognitiveNexus` (knowledge graph).
3. Loads bootstrap KIP definitions (`$self`, `$system`, core meta-types).
4. Stores creator/owner principal IDs.

### Runtime
- Spaces are **lazy-loaded** on first access via `OnceCell`.
- In-memory cache with access tracking.
- **5-minute interval**: Flush active spaces to storage.
- **20-minute idle timeout**: Evict unused spaces from cache.
- Graceful shutdown: Flush all spaces before exit.

### Memory Types in the Cognitive Nexus

| Type            | Nodes                                   | Description                                 |
| --------------- | --------------------------------------- | ------------------------------------------- |
| **Concept**     | `{type: "UpperCamelCase", name: "..."}` | Entities with typed attributes and metadata |
| **Proposition** | `(Subject, Predicate, Object)`          | Directed relationships between concepts     |
| **Domain**      | Grouping node                           | Organizational containers for concepts      |

The schema is self-describing — all type definitions are stored as nodes within the graph itself. Types can be defined on-the-fly by the Formation agent as needed.

## Configuration

### CLI Arguments / Environment Variables

| Env Variable      | CLI Flag            | Default                                                   | Description                                                                          |
| ----------------- | ------------------- | --------------------------------------------------------- | ------------------------------------------------------------------------------------ |
| `LISTEN_ADDR`     | `--addr`            | `127.0.0.1:8042`                                          | Listen address                                                                       |
| `ED25519_PUBKEYS` | `--ed25519-pubkeys` | —                                                         | Comma-separated Base64 Ed25519 public keys; if empty, API authentication is disabled |
| `MODEL_FAMILY`    | `--model-family`    | `gemini`                                                  | Model family to use for encoding and recall (e.g., `gemini`, `anthropic`, `openai`)  |
| `MODEL_API_KEY`   | `--model-api-key`   | —                                                         | Google Gemini API key                                                                |
| `MODEL_API_BASE`  | `--model-api-base`  | `https://generativelanguage.googleapis.com/v1beta/models` | Gemini API base URL                                                                  |
| `MODEL_NAME`      | `--model-name`      | `gemini-3-flash-preview`                                  | LLM model for agents                                                                 |
| `HTTPS_PROXY`     | `--https-proxy`     | —                                                         | HTTPS proxy URL                                                                      |
| `SHARDING_IDX`    | `--sharding-idx`    | `0`                                                       | Shard index for this instance                                                        |
| `MANAGERS`        | `--managers`        | —                                                         | Comma-separated manager principal IDs                                                |
| `CORS_ORIGINS`    | `--cors-origins`    | —                                                         | CORS allowed origins: empty = disabled, `*` = allow all, or comma-separated origins  |

`CORS_ORIGINS` examples:
- `""` (empty): CORS disabled
- `"*"`: allow all origins
- `"https://app.example.com,https://admin.example.com"`: allow specific origins

### Storage Backends

| Subcommand | Description                     | Key Env Variables                                                        |
| ---------- | ------------------------------- | ------------------------------------------------------------------------ |
| *(none)*   | In-memory storage (dev/testing) | —                                                                        |
| `local`    | Local filesystem storage        | `LOCAL_DB_PATH` (default `./db`)                                         |
| `aws`      | AWS S3 storage                  | `AWS_BUCKET`, `AWS_REGION`, `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY` |

## Running

```bash
# Development (in-memory storage)
cargo run -p anda_hippocampus

# Local filesystem storage
cargo run -p anda_hippocampus -- local --db ./data

# AWS S3 storage
cargo run -p anda_hippocampus -- aws --bucket my-bucket --region us-east-1
```

### Run with Docker image

```bash
# Pull image
docker pull ghcr.io/ldclabs/anda_hippocampus_amd64:latest

# Run with ENV (in-memory by default)
docker run --rm -p 8042:8042 \
  -e LISTEN_ADDR=0.0.0.0:8042 \
  -e GEMINI_API_KEY=your_key \
  ghcr.io/ldclabs/anda_hippocampus_amd64:latest

# Override startup args (example: local storage)
docker run --rm -p 8042:8042 \
  -v $(pwd)/data:/data \
  ghcr.io/ldclabs/anda_hippocampus_amd64:latest local --db /data

# Override startup args (example: AWS S3 storage)
docker run --rm -p 8042:8042 \
  -e AWS_ACCESS_KEY_ID=your_ak \
  -e AWS_SECRET_ACCESS_KEY=your_sk \
  ghcr.io/ldclabs/anda_hippocampus_amd64:latest aws --bucket my-bucket --region us-east-1
```

## Dependencies

Key crates from the Anda ecosystem:

| Crate                  | Purpose                                                        |
| ---------------------- | -------------------------------------------------------------- |
| `anda_core`            | Core traits (`Agent`, `Tool`, `AgentContext`) and types        |
| `anda_engine`          | Agent engine, model integration, memory management             |
| `anda_db`              | Persistent database layer (`AndaDB`) with configurable storage |
| `anda_kip`             | KIP syntax parser and built-in knowledge templates             |
| `anda_cognitive_nexus` | Cognitive Nexus knowledge graph implementation                 |
| `object_store`         | Object store abstraction                                       |

## License

Copyright © LDC Labs

Licensed under the MIT or Apache-2.0 license.
