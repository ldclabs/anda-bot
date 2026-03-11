---
name: anda_hippocampus
version: 0.1.0
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

Persistent long-term memory service for LLM agents, powered by a Knowledge Graph (Cognitive Nexus) and KIP (Knowledge Interaction Protocol).

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
| **Formation** | `POST /v1/{space_id}/formation` | Encode conversations into structured memory | `write` |
| **Recall** | `POST /v1/{space_id}/recall` | Query memory with natural language | `read` |
| **Maintenance** | `POST /v1/{space_id}/maintenance` | Consolidate, prune, and organize memory | `write` |

Supporting endpoints:

| Method | Endpoint | Purpose | Auth |
|--------|----------|---------|------|
| `GET` | `/` | Service info (name, version, shard) | — |
| `GET` | `/SKILL.md` | This skill description | — |
| `POST` | `/admin/create_space` | Create a new memory space | manager |
| `GET` | `/v1/{space_id}/status` | Space status and statistics | `read` |

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

Each space is an isolated environment with its own knowledge graph, conversation history, and database. Spaces are identified by IDs in the format `s{shard_index}-{name}`, e.g. `s0-d688lqjs0946lfo0014g`. The shard index must match the server's configured shard.

### Memory Types

The Formation agent extracts three types of memory from conversations:

1. **Episodic Memory** (Events) — What happened, when, who participated, outcome
2. **Semantic Memory** (Stable Knowledge) — Facts, preferences, relationships, domain knowledge
3. **Cognitive Memory** (Patterns) — Behavioral patterns, decision criteria, communication style

### Cognitive Nexus

The underlying knowledge graph consists of:

- **Concept Nodes** — Entities with a type and name (e.g., `Person:Alice`, `Preference:dark_mode`)
- **Proposition Links** — Directed relationships between concepts (e.g., `(Alice, prefers, dark_mode)`)

---

## Authentication

All endpoints (except `/` and `/SKILL.md`) require a Bearer token in the `Authorization` header.

```
Authorization: Bearer <base64_encoded_cose_sign1_token>
```

---

## API Reference

### Content Negotiation

The API supports dual serialization. Set `Content-Type` and `Accept` headers accordingly:

- `application/json` — JSON (default)
- `application/cbor` — CBOR (binary, more compact)

All responses are wrapped in an RPC envelope:

```json
{
  "result": { ... },
  "error": null
}
```

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
  "space_id": "s0-my_space_name"
}
```

**Response:**

```json
{
  "result": { ... }
}
```

The `space_id` must start with `s{shard_index}-` matching the server's configured shard index (default: `s0-`).

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
  "result": { "_id": 1, ... }
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
    "status": "success",
    "answer": "Alice prefers dark mode for all applications and operates in the UTC+8 timezone.",
    "gaps": []
  }
}
```

**Status values:**

| Status | Meaning |
|--------|---------|
| `success` | Query fully answered from available memory |
| `partial` | Some aspects answered, but gaps remain (check `gaps` array) |
| `not_found` | No relevant memory found for this query |

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
  "result": { "_id": 8, ... }
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
    "space_id": "s0-d688lqjs0946lfo0014g",
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

A typical integration workflow for a business agent:

### 1. Prepare: Create a memory space (one-time setup)

```bash
curl -sX POST https://your-hippocampus-host/admin/create_space \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"user": "owner_principal_id", "space_id": "s0-my_agent_memory"}'
```

### 2. Remember: Send conversations for memory encoding

After each meaningful conversation with a user, send the messages to Formation:

```bash
curl -sX POST https://your-hippocampus-host/v1/s0-my_agent_memory/formation \
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
curl -sX POST https://your-hippocampus-host/v1/s0-my_agent_memory/recall \
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
curl -sX POST https://your-hippocampus-host/v1/s0-my_agent_memory/maintenance \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "trigger": "scheduled",
    "scope": "full",
    "timestamp": "2026-03-10T03:00:00Z"
  }'
```

---

## Troubleshooting

| Symptom | Fix |
|---------|-----|
| `401 Unauthorized` | Check Bearer token: valid signature, correct `aud` (space ID), and required `scope` (`read` or `write`) |
| `404 Not Found` on space endpoints | Verify the `space_id` format matches `s{shard}-{name}` and the shard matches the server |
| Formation returns but nothing in memory | Formation is async — check space status after a few seconds; look at the conversation status |
| Recall returns `not_found` | Memory may not have been encoded yet, or the query doesn't match stored knowledge; try broader phrasing |
| Maintenance rejected | Only one maintenance cycle can run at a time per space; wait for the current one to finish |
| Empty recall for new space | Expected — a new space has no memory yet; send conversations via Formation first |

---

## Configuration Reference

The service is configured via CLI arguments and environment variables:

| Env Variable | Default | Description |
|--------------|---------|-------------|
| `LISTEN_ADDR` | `127.0.0.1:8080` | Listen address |
| `ED25519_PUBKEYS` | — | Comma-separated Base64-encoded Ed25519 public keys |
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