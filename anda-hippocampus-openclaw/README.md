# anda-hippocampus

OpenClaw plugin for [Anda Hippocampus](https://brain.anda.ai) — persistent long-term memory for LLM agents.

## Features

- **Automatic memory encoding** — Every agent turn is sent to Anda Hippocampus for knowledge extraction (fire-and-forget via `agent_end` hook).
- **Memory recall tool** — Registers a `recall_memory` agent tool that queries long-term memory with natural language.

## Installation

```bash
pnpm add anda-hippocampus
```

## Quick Start

```ts
import { createHippocampusPlugin } from 'anda-hippocampus'

// Export as an OpenClaw plugin module
export default createHippocampusPlugin({
  spaceId: 'my_space_001',
  spaceToken: 'ST_xxxxx',
  // baseUrl: 'https://brain.anda.ai',  // default
})
```

## Configuration

| Option               | Type           | Required | Default                 | Description                                 |
| -------------------- | -------------- | -------- | ----------------------- | ------------------------------------------- |
| `spaceId`            | `string`       | Yes      | —                       | Memory space ID                             |
| `spaceToken`         | `string`       | Yes      | —                       | Space token for API authentication          |
| `baseUrl`            | `string`       | No       | `https://brain.anda.ai` | Anda Hippocampus base URL                   |
| `defaultContext`     | `InputContext` | No       | —                       | Default context included with every request |
| `formationTimeoutMs` | `number`       | No       | `30000`                 | Formation request timeout (ms)              |
| `recallTimeoutMs`    | `number`       | No       | `120000`                | Recall request timeout (ms)                 |

### `defaultContext`

```ts
interface InputContext {
  user?: string   // User identifier
  agent?: string  // Agent identifier
  session?: string // Session identifier
  topic?: string  // Conversation topic
}
```

## How It Works

### Memory Formation (`agent_end` hook)

After each agent turn completes, the plugin converts `AgentMessage[]` to Hippocampus messages and sends them to:

```
POST /v1/{space_id}/formation
```

This is fire-and-forget — the Hippocampus service queues the messages for background processing, extracting:
- **Episodic memory** — Events with timestamps, participants, outcomes
- **Semantic memory** — Facts, preferences, relationships
- **Cognitive memory** — Behavioral patterns, decision criteria

### Memory Recall (`recall_memory` tool)

The agent can call the `recall_memory` tool with a natural language query. The plugin sends it to:

```
POST /v1/{space_id}/recall
```

This may take 10–100 seconds as the Hippocampus service searches the knowledge graph and synthesizes an answer.

**Tool parameters:**

| Parameter       | Type     | Required | Description                   |
| --------------- | -------- | -------- | ----------------------------- |
| `query`         | `string` | Yes      | Natural language question     |
| `context.user`  | `string` | No       | Current user identifier       |
| `context.agent` | `string` | No       | Calling agent identifier      |
| `context.topic` | `string` | No       | Topic hint for disambiguation |

## License

Copyright © LDC Labs

Licensed under Apache-2.0 license.
