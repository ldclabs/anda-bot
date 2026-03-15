# anda-hippocampus

OpenClaw plugin for [Anda Hippocampus](https://brain.anda.ai) that gives agents persistent long-term memory.

It does two things:

- automatically sends completed conversations to Anda Hippocampus for memory formation
- registers a `recall_memory` tool so the agent can retrieve relevant memory with natural-language queries

By default the plugin uses the hosted Anda Hippocampus service, but `baseUrl` can point to a self-hosted or local deployment when memory data must stay inside your own network.

## What This Plugin Provides

| Capability      | Type           | What it does                                                                                     |
| --------------- | -------------- | ------------------------------------------------------------------------------------------------ |
| `agent_end`     | Automatic hook | Converts the finished conversation into Hippocampus messages and sends them to the formation API |
| `recall_memory` | Agent tool     | Queries long-term memory using natural language and returns the synthesized result               |

This plugin does not try to replace OpenClaw's full prompt lifecycle. It focuses on one reliable write path and one recall tool.

## Prerequisites

- OpenClaw `>=2026.3.8`
- An Anda Hippocampus space ID and space token
- Optional: a self-hosted or local Anda Hippocampus deployment if you do not want to use the hosted service

## Installation

The plugin is auto-discovered by OpenClaw from the package metadata. After installation, you only need to enable it in `openclaw.json`.

### 1. Install from SKILL.md via Claw

```text
Please add plugin "anda-hippocampus" from https://brain.anda.ai/SKILL.md to your OpenClaw workspace. Use the following configuration:

{
  "spaceId": "my_space_001",
  "spaceToken": "ST_xxxxx",
  "baseUrl": "https://brain.anda.ai" // hosted default
  // "baseUrl": "http://localhost:8042" // self-hosted/local deployment
  // ... other optional config fields ...
}
```

### 2. Install the plugin via CLI

1. Install the plugin package:
```bash
openclaw plugins install anda-hippocampus
```

2. Update anda-hippocampus configuration in `openclaw.json`:
```json
{
  "plugins": {
    "entries": {
      "anda-hippocampus": {
        "enabled": true,
        "config": {
          "spaceId": "my_space_001",
          "spaceToken": "ST_xxxxx",
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

- `spaceId`: your Hippocampus space ID
- `spaceToken`: your space token
- `baseUrl`: optional, defaults to `https://brain.anda.ai`; set this to your own deployment for self-hosted or local use

If memory data must stay inside your network, set `baseUrl` to your own Anda Hippocampus deployment.

## Configuration

| Option               | Type           | Required | Default                 | Description                                                                                  |
| -------------------- | -------------- | -------- | ----------------------- | -------------------------------------------------------------------------------------------- |
| `spaceId`            | `string`       | Yes      | —                       | Memory space ID used for both formation and recall                                           |
| `spaceToken`         | `string`       | Yes      | —                       | Bearer token used to authenticate API requests                                               |
| `baseUrl`            | `string`       | No       | `https://brain.anda.ai` | Anda Hippocampus base URL. Can point to the hosted service or a self-hosted/local deployment |
| `defaultContext`     | `InputContext` | No       | —                       | Context merged into every request                                                            |
| `formationTimeoutMs` | `number`       | No       | `30000`                 | Timeout for formation requests                                                               |
| `recallTimeoutMs`    | `number`       | No       | `120000`                | Timeout for recall requests                                                                  |

### `defaultContext`

```ts
interface InputContext {
  user?: string
  agent?: string
  session?: string
  topic?: string
}
```

Notes:

- `defaultContext` is merged with per-call recall context
- during `agent_end`, the plugin fills `agent` and `session` from the OpenClaw runtime when available
- `baseUrl` has trailing slashes removed automatically

## How It Works

```text
Agent turn completes in OpenClaw
        |
        v
`agent_end` hook extracts user and assistant messages
        |
        v
POST /v1/{space_id}/formation
        |
        v
Anda Hippocampus queues background memory formation

Later, when the agent needs memory:
        |
        v
Agent calls `recall_memory`
        |
        v
POST /v1/{space_id}/recall
        |
        v
Plugin returns the synthesized memory result to the agent
```

### Formation Behavior

After each completed agent turn, the plugin converts `AgentMessage[]` into Hippocampus messages and sends them to:

```text
POST /v1/{space_id}/formation
```

Formation is fire-and-forget. The plugin does not block the agent while the service processes memory in the background.

Current conversion behavior:

- user text messages are included
- assistant text content is included
- tool results and custom messages are skipped
- message timestamps are preserved when available

### Recall Tool

The plugin registers a `recall_memory` tool that sends queries to:

```text
POST /v1/{space_id}/recall
```

Recall may take longer than normal tool calls because the Hippocampus service may search a knowledge graph and synthesize a response. The default timeout is 120 seconds.

Tool parameters:

| Parameter       | Type     | Required | Description                   |
| --------------- | -------- | -------- | ----------------------------- |
| `query`         | `string` | Yes      | Natural-language memory query |
| `context.user`  | `string` | No       | Current user identifier       |
| `context.agent` | `string` | No       | Calling agent identifier      |
| `context.topic` | `string` | No       | Topic hint for disambiguation |

Example:

```json
{
  "query": "What preferences has Alice expressed about release communication?",
  "context": {
    "user": "alice",
    "topic": "product launches"
  }
}
```

## Operational Notes

- Formation failures are logged and do not crash the agent flow
- Recall failures are returned as tool errors so the agent can react explicitly
- If no relevant memory is found, the tool returns `No relevant memory found.`

## Troubleshooting

| Problem                             | Likely cause                                  | What to check                                                                      |
| ----------------------------------- | --------------------------------------------- | ---------------------------------------------------------------------------------- |
| Formation requests are not arriving | Invalid `spaceId`, `spaceToken`, or `baseUrl` | Verify credentials and watch for `[anda-hippocampus] Formation failed` logs        |
| Recall times out                    | Recall search is taking too long              | Increase `recallTimeoutMs`                                                         |
| Empty recall results                | Query too vague or memory not formed yet      | Try a more specific query with `context.user`, `context.agent`, or `context.topic` |
| Data must stay on-prem              | Hosted default not acceptable                 | Point `baseUrl` to your self-hosted or local deployment                            |

## License

Copyright © LDC Labs

Licensed under Apache-2.0 license.
