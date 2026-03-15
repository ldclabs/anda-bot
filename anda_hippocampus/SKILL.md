---
name: anda_hippocampus
description: |
  Long-term memory service for LLM agents.
  Provides persistent, structured memory (Cognitive Nexus) through three operations:
  Formation (encode conversations into memory), Recall (query memory with natural language), and Maintenance (consolidate and prune memory).

  Use this service when:
  - You need to persist facts, preferences, relationships, or events across sessions
  - You want to recall previous conversations, decisions, or user context
  - You need structured long-term memory without understanding KIP syntax
  - You want to trigger memory consolidation or cleanup

  Common trigger phrases:
  - "remember this", "save this for later", "don't forget"
  - "what did I say last time?", "recall my preferences"
  - "what do we know about X?", "who is X?"
  - "run memory maintenance", "consolidate memory"
metadata:
  version: 0.1.0
  url: https://brain.anda.ai/SKILL.md
  keywords:
    - long-term memory
    - agent memory
    - knowledge graph
    - cognitive nexus
    - memory formation
    - memory recall
    - memory maintenance
    - KIP
    - persistent memory
---

# 🧠 Anda Hippocampus

Persistent long-term memory service for LLM agents, powered by a Knowledge Graph (Cognitive Nexus) and KIP (Knowledge Interaction Protocol). Anda Hippocampus is [open-source software](https://github.com/ldclabs/anda-hippocampus) — you can self-host it or use the cloud SaaS at `https://brain.anda.ai`.

Business agents interact entirely through **natural language** and a simple REST API — no KIP knowledge required.

```
Business Agent  ──natural language──▶  Hippocampus  ──KIP──▶  Cognitive Nexus
 (your agent)                         (this service)          (knowledge graph)
```

---

## What You Get

Three operational modes cover the full memory lifecycle:

| Mode | Endpoint | Purpose | Auth |
|------|----------|---------|------|
| **Formation** | `POST /v1/{space_id}/formation` | Encode conversations into structured memory | `write` (CWT or space token) |
| **Recall** | `POST /v1/{space_id}/recall` | Query memory with natural language | `read` (CWT or space token) |
| **Maintenance** | `POST /v1/{space_id}/maintenance` | Consolidate, prune, and organize memory | `write` (CWT or space token) |

Supporting endpoints:

| Method | Endpoint | Purpose | Auth |
|--------|----------|---------|------|
| `GET` | `/` | Anda Hippocampus website | — |
| `GET` | `/info` | Service info (name, version, sharding) | — |
| `GET` | `/SKILL.md` | This skill description | — |
| `GET` | `/v1/{space_id}/status` | Space status and statistics | `read` (CWT or space token) |
| `GET` | `/v1/{space_id}/conversations/{conversation_id}` | Get one conversation detail | `read` (CWT or space token) |
| `GET` | `/v1/{space_id}/conversations` | List conversations (cursor pagination) | `read` (CWT or space token) |
| `GET` | `/v1/{space_id}/management/space_tokens` | List space tokens | `read` (CWT) |
| `POST` | `/v1/{space_id}/management/add_space_token` | Add a space token | `write` (CWT) |
| `POST` | `/v1/{space_id}/management/revoke_space_token` | Revoke a space token | `write` (CWT) |
| `POST` | `/v1/{space_id}/management/update_space` | Update space information (name, description, public/private) | `write` (CWT) |
| `POST` | `/admin/{space_id}/update_space_tier` | Update a space tier | manager (CWT) |
| `POST` | `/admin/create_space` | Create a new memory space | manager (CWT) |
> Auth scopes in tables apply when authentication is enabled (`ED25519_PUBKEYS` is set).

---

## When to Use This Service

Use Anda Hippocampus when your agent needs to:

- **Persist knowledge across sessions** — user preferences, facts, decisions, relationships, events
- **Recall previous context** — what happened before, what the user said, what decisions were made
- **Share memory across agents** — multiple agents can read/write to the same space
- **Maintain memory health** — consolidate old events, deduplicate facts, decay stale knowledge

The service handles all the complexity of knowledge graph management. Your agent just sends messages and asks questions in natural language.

## When NOT to Use

- Temporary conversation context that only matters in the current session
- Large file storage (use object storage instead)
- Real-time data streaming
- Secrets, passwords, or API keys (the service is not a vault)

---

## Concepts

### Memory Space

Each space is an isolated environment with its own knowledge graph, conversation history, and database. Spaces are identified by a `space_id` string.

### Memory Types

The Formation agent extracts three types of memory from conversations:

1. **Episodic Memory** (Events) — What happened, when, who participated, outcome
2. **Semantic Memory** (Stable Knowledge) — Facts, preferences, relationships, domain knowledge
3. **Cognitive Memory** (Patterns) — Behavioral patterns, decision criteria, communication style

### Cognitive Nexus

The underlying knowledge graph consists of:

- **Concept Nodes** — Entities with a type and name (e.g., `{type: "Person", name: "Alice"}`, `{type: "Preference", name: "dark_mode"}`)
- **Proposition Links** — Directed relationships between concepts (e.g., `(Alice, "prefers", dark_mode)`)

---

## Authentication

If `ED25519_PUBKEYS` is configured, protected endpoints require a Bearer token in the `Authorization` header.

If `ED25519_PUBKEYS` is empty/not provided, authentication is disabled and requests are accepted without signature verification.

```
Authorization: Bearer <base64_encoded_cose_sign1_token>
```

Management endpoints (`/v1/{space_id}/management/*`) and admin endpoints (`/admin/*`) still follow their role/scope checks when auth is enabled.

---

## API Reference

For complete endpoint and TypeScript schema details, see:
- `https://github.com/ldclabs/anda-hippocampus/blob/main/anda_hippocampus/API.md` (English)
- `https://github.com/ldclabs/anda-hippocampus/blob/main/anda_hippocampus/API_cn.md` (中文)

### Content Negotiation

The API supports triple serialization. Set `Content-Type` and `Accept` headers accordingly:

- `application/json` — JSON (default)
- `application/cbor` — CBOR (binary, more compact)
- `text/markdown` — Markdown (raw text or formatted Markdown)

All responses are wrapped in an RPC envelope when using JSON or CBOR:

```json
{
  "result": { ... },
  "error": null
}
```

When `Accept: text/markdown` is used, the response is returned as raw text or a Markdown formatted string.

On error:

```json
{
  "result": null,
  "error": {
    "message": "error description",
    "data": { ... }
  }
}
```

#### Markdown Serialization Sample

If `Accept: text/markdown` is specified, the `result` field's content will be directly serialized as the response body.

**Request:**
```http
POST /v1/my_space_001/recall
Accept: text/markdown

What are Alice's preferences?
```

**Response (HTTP 200):**
```markdown
Alice has the following known preferences:
- **Dark mode** in all applications (confidence: 0.9, since 2025-01-15)
- **Email communication** preferred over phone calls (confidence: 0.8, since 2025-01-10)

Alice is currently working on **Project Aurora** and was last seen on 2025-01-15 discussing settings preferences.

Gaps:
- No information found about Alice's language preferences.
```

---

### Create Space

Create a new isolated memory space.

```
POST /admin/create_space
Authorization: Bearer <token>
Content-Type: application/json
```

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
  "result": { ... }
}
```

---

### Formation — Encode Conversations into Memory

Send conversation messages to be analyzed and encoded into the knowledge graph. The service extracts facts, preferences, relationships, events, and patterns, then stores them as structured knowledge.

Processing is asynchronous — the endpoint returns immediately with a conversation ID while encoding continues in the background. New submissions are queued and processed sequentially.

```
POST /v1/{space_id}/formation
Authorization: Bearer <token>
Content-Type: application/json
```

**Request:**

```json
{
  "messages": [
    {
      "role": "user",
      "content": "I prefer dark mode for all my apps. My timezone is UTC+8.",
      "name": "Alice"
    },
    {
      "role": "assistant",
      "content": "Got it! I've noted your preference for dark mode and UTC+8 timezone."
    }
  ],
  "context": {
    "user": "alice_principal_id",
    "agent": "customer_bot_001",
    "session": "sess_2026-03-09_abc",
    "topic": "settings"
  },
  "timestamp": "2026-03-09T10:30:00Z"
}
```

**Response:**

```json
{
  "result": { "conversation": 1, ... }
}
```

**Fields:**

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `messages` | `Message[]` | Yes | Conversation messages (role: `user` / `assistant` / `system`) |
| `context` | `InputContext` | No | Contextual metadata to help with encoding |
| `context.user` | `string` | No | User identifier |
| `context.agent` | `string` | No | Calling agent identifier |
| `context.session` | `string` | No | Session identifier |
| `context.topic` | `string` | No | Conversation topic |
| `timestamp` | `string` | Yes | ISO 8601 timestamp of the conversation |

**Tips for best results:**

- Include the `context` field whenever possible — it helps the encoder associate knowledge correctly
- Send complete conversation segments, not individual messages
- Include timestamps to enable proper temporal reasoning
- The `name` field in messages helps distinguish between multiple users in the same conversation

---

### Recall — Query Memory

Ask a natural language question and receive a synthesized answer drawn from the knowledge graph and conversation history.

```
POST /v1/{space_id}/recall
Authorization: Bearer <token>
Content-Type: application/json
```

**Request:**

```json
{
  "query": "What are Alice's preferences?",
  "context": {
    "user": "alice_principal_id",
    "topic": "settings"
  }
}
```

**Response:**

```json
{
  "result": {
    "content": "Alice prefers dark mode for all applications and operates in the UTC+8 timezone.",
    ...
  }
}
```

Note: `result.content` is the primary contract. Additional fields may vary by model/runtime.

**Query examples:**

| Intent | Example query |
|--------|--------------|
| Entity lookup | "Who is Alice?" |
| Relationship | "Who does Alice work with?" |
| Attribute | "What are Alice's preferences?" |
| Event recall | "What happened in our last meeting?" |
| Domain exploration | "What do we know about Project Aurora?" |
| Pattern detection | "Does Alice prefer email or chat?" |
| Existence check | "Have we discussed the pricing strategy?" |

---

### Maintenance — Memory Consolidation

Trigger a maintenance cycle to consolidate, prune, and optimize the knowledge graph. This runs asynchronously in the background with a single-execution guard (only one maintenance cycle can run at a time per space).

```
POST /v1/{space_id}/maintenance
Authorization: Bearer <write_token>
Content-Type: application/json
```

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

**Response:**

```json
{
  "result": { "conversation": 8, ... }
}
```

**Fields:**

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `trigger` | `string` | Yes | `scheduled` / `threshold` / `on_demand` |
| `scope` | `string` | Yes | `full` (all phases) / `quick` (assessment + urgent tasks only) |
| `timestamp` | `string` | Yes | ISO 8601 timestamp |
| `parameters` | `object` | No | Override default thresholds |

---

### Space Status

Get statistics and health information for a memory space.

```
GET /v1/{space_id}/status
Authorization: Bearer <token>
```

**Response:**

```json
{
  "result": {
    "space_id": "my_space_001",
    "owner": "principal_id",
    "db_stats": {
      "total_items": 150,
      "total_bytes": 524288
    },
    "concepts": 85,
    "propositions": 120,
    "conversations": 12
  }
}
```

---

## Integration Pattern

A typical integration workflow for a business agent (use `brain.anda.ai` as the host):

### 1. Prepare: Create a memory space (one-time setup, should be done by an admin or manager)

```bash
curl -sX POST https://brain.anda.ai/admin/create_space \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"user": "owner_principal_id", "space_id": "my_space_001", "tier": 0}'
```

### 2. Remember: Send conversations for memory encoding

After each meaningful conversation with a user, send the messages to Formation:

```bash
curl -sX POST https://brain.anda.ai/v1/my_space_001/formation \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "messages": [
      {"role": "user", "content": "I work at Acme Corp as a senior engineer."},
      {"role": "assistant", "content": "Nice to meet you! Noted that you are a senior engineer at Acme Corp."}
    ],
    "context": {"user": "user_123", "agent": "onboarding_bot"},
    "timestamp": "2026-03-09T10:30:00Z"
  }'
```

### 3. Recall: Query memory before responding

Before generating a response, check if relevant memory exists:

```bash
curl -sX POST https://brain.anda.ai/v1/my_space_001/recall \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "query": "Where does this user work and what is their role?",
    "context": {"user": "user_123"}
  }'
```

### 4. Maintain: Schedule periodic maintenance

Run maintenance to keep memory healthy and relevant:

```bash
curl -sX POST https://brain.anda.ai/v1/my_space_001/maintenance \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "trigger": "scheduled",
    "scope": "full",
    "timestamp": "2026-03-10T03:00:00Z"
  }'
```

---

## OpenClaw Integration

The [`anda-hippocampus`](https://github.com/ldclabs/anda-hippocampus/tree/main/anda-hippocampus-openclaw) plugin integrates Anda Hippocampus into [OpenClaw](https://openclaw.ai/) agents, providing automatic memory encoding and a `recall_memory` tool — no manual API calls needed.

### Prerequisites: Create a Brain Space (for cloud SaaS users)

Before installing the plugin, you need a `spaceId` and `spaceToken`:

1. Go to the **Anda Hippocampus Console**: [https://anda.ai/brain](https://anda.ai/brain)
2. Sign in and **create a new brain space** — you will get your `spaceId`.
3. In the space settings, **create an API Key** — this is your `spaceToken`.

> If you are self-hosting Anda Hippocampus, create spaces via the admin API instead (see [Integration Pattern](#integration-pattern) above).

### Install

1. Install the plugin package:
```bash
openclaw plugins install anda-hippocampus
```

2. Update anda-hippocampus configuration in `openclaw.json` with the `spaceId` and `spaceToken` obtained from the console:
```json
{
  "plugins": {
    "entries": {
      "anda-hippocampus": {
        "enabled": true,
        "config": {
          "spaceId": "my_space_001",
          "spaceToken": "STxxxxx",
          "baseUrl": "https://brain.anda.ai" // or "http://localhost:8042" for self-hosted/local
        }
      }
    }
  }
}
```

3. Restart OpenClaw Gateway.
```sh
openclaw gateway restart
```

Required fields:

- `spaceId`: your Hippocampus space ID (created at [anda.ai/brain](https://anda.ai/brain))
- `spaceToken`: your space API Key (created at [anda.ai/brain](https://anda.ai/brain))
- `baseUrl`: optional, defaults to `https://brain.anda.ai`; set this to your own deployment for self-hosted or local use

### What It Does

| Feature | Mechanism | Description |
|---------|-----------|-------------|
| **Memory encoding** | `agent_end` hook | After each agent turn, conversation messages are automatically sent to `POST /v1/{space_id}/formation` (fire-and-forget). |
| **Memory recall** | `recall_memory` tool | Registered as an agent tool; the LLM can call it with a natural language query to retrieve knowledge via `POST /v1/{space_id}/recall`. |

### Configuration Options

| Option | Type | Required | Default | Description |
|--------|------|----------|---------|-------------|
| `spaceId` | `string` | Yes | — | Memory space ID |
| `spaceToken` | `string` | Yes | — | Space token for API authentication |
| `baseUrl` | `string` | No | `https://brain.anda.ai` | Anda Hippocampus service URL |
| `defaultContext` | `InputContext` | No | — | Default context included with every request (`user`, `agent`, `session`, `topic`) |
| `formationTimeoutMs` | `number` | No | `30000` | Formation request timeout (ms) |
| `recallTimeoutMs` | `number` | No | `120000` | Recall request timeout (ms) — recall may take 10–100s |

### `recall_memory` Tool Parameters

The plugin registers a `recall_memory` tool that the LLM can invoke:

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `query` | `string` | Yes | Natural language question (e.g. "What are Alice's preferences?") |
| `context.user` | `string` | No | Current user identifier |
| `context.agent` | `string` | No | Calling agent identifier |
| `context.topic` | `string` | No | Topic hint for disambiguation |

---

## Troubleshooting

| Symptom | Fix |
|---------|-----|
| `401 Unauthorized` | If auth is enabled (`ED25519_PUBKEYS` set), check Bearer token signature, `aud` (space ID), and required `scope` (`read`/`write`) |
| `404 Not Found` on space endpoints | Verify the `space_id` exists and the token `aud` matches the target space |
| Formation returns but nothing in memory | Formation is async — check space status after a few seconds; look at the conversation status |
| Recall seems empty or insufficient | Memory may not be encoded yet, or the query is too narrow; try broader phrasing and include `context` |
| Maintenance rejected | Only one maintenance cycle can run at a time per space; wait for the current one to finish |
| Empty recall for new space | Expected — a new space has no memory yet; send conversations via Formation first |

---

## Configuration Reference

The service is configured via CLI arguments and environment variables:

| Env Variable | Default | Description |
|--------------|---------|-------------|
| `LISTEN_ADDR` | `127.0.0.1:8042` | Listen address |
| `ED25519_PUBKEYS` | — | Comma-separated Base64-encoded Ed25519 public keys; if empty, API authentication is disabled |
| `GEMINI_API_KEY` | — | Google Gemini API key |
| `GEMINI_API_BASE` | `https://generativelanguage.googleapis.com/v1beta/models` | Gemini API base URL |
| `GEMINI_MODEL` | `gemini-3-flash-preview` | LLM model for agents |
| `HTTPS_PROXY` | — | HTTPS proxy URL |
| `SHARDING_IDX` | `0` | Shard index for this instance |
| `MANAGERS` | — | Comma-separated manager principal IDs |

**Storage backends:**

| Backend | Command | Key Config |
|---------|---------|------------|
| In-memory (dev) | `cargo run -p anda_hippocampus` | — |
| Local filesystem | `cargo run -p anda_hippocampus -- local` | `LOCAL_DB_PATH` (default `./db`) |
| AWS S3 | `cargo run -p anda_hippocampus -- aws` | `AWS_BUCKET`, `AWS_REGION` |