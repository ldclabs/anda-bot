# Anda Hippocampus API 文档（含 TypeScript 类型）

## 1) 通用约定

- Base URL: `http://{host}:{port}`
- 认证头：`Authorization: Bearer <token>`
- 若 `ED25519_PUBKEYS` 为空/未提供，则鉴权关闭。
- 支持序列化：
  - 请求：`Content-Type: application/json | application/cbor | text/markdown`
  - 响应：`Accept: application/json | application/cbor | text/markdown`
- 大多数业务接口返回 RPC 包装结构：`RpcResponse<T>`

---

## 2) TypeScript 类型定义

```ts
export type TokenScope = 'read' | 'write' | '*';

export interface RpcError {
  message: string;
  data?: unknown;
}

export interface RpcResponse<T> {
  result?: T;
  error?: RpcError;
  next_cursor?: string;
}

export interface InputContext {
  user?: string;
  agent?: string;
  session?: string;
  topic?: string;
}

export type MessageRole = 'system' | 'user' | 'assistant' | 'tool';

export type MessageContentPart =
  | string
  | {
      type: string;
      text?: string;
      [k: string]: unknown;
    };

export interface Message {
  role: MessageRole;
  content: string | MessageContentPart[];
  name?: string;  // user 或 tool 的名称
  user?: string;  // user ID
  timestamp?: string; // ISO 8601
}

export interface FormationInput {
  messages: Message[];
  context?: InputContext;
  timestamp: string; // ISO 8601
}

export interface RecallInput {
  query: string;
  context?: InputContext;
}

export interface MaintenanceParameters {
  stale_event_threshold_days?: number;
  confidence_decay_factor?: number;
  unsorted_max_backlog?: number;
  orphan_max_count?: number;
}

export interface MaintenanceInput {
  trigger?: 'scheduled' | 'threshold' | 'on_demand';
  scope?: 'full' | 'quick';
  timestamp: string; // ISO 8601
  parameters?: MaintenanceParameters;
}

export interface AddSpaceTokenInput {
  scope: TokenScope;
}

export interface RevokeSpaceTokenInput {
  token: string;
}

export interface SetSpacePublicInput {
  public: boolean;
}

export interface CreateOrUpdateSpaceInput {
  user: string; // Principal
  space_id: string; // 形如 s{sharding}-{id}
  tier: number;
}

export interface SpaceTier {
  tier: number;
  updated_at: number; // Unix timestamp in milliseconds
}

export interface SpaceToken {
  scope: TokenScope;
  usage: number;
  created_at: number; // Unix timestamp in milliseconds
  updated_at: number; // Unix timestamp in milliseconds
}

export interface StorageStats {
  [k: string]: number | string | boolean | null;
}

export interface SpaceStatus {
  space_id: string;
  owner: string;
  db_stats: StorageStats;
  concepts: number;
  propositions: number;
  conversations: number;
  public: boolean;
  tier: SpaceTier;
}

export interface Usage {
  input_tokens?: number;
  output_tokens?: number;
  total_tokens?: number;
}

export interface AgentOutput {
  content: string;
  conversation?: number;
  failed_reason?: string;
  usage?: Usage;
  model?: string;
  [k: string]: unknown;
}

export type ConversationStatus =
  | 'submitted'
  | 'working'
  | 'completed'
  | 'failed'
  | 'cancelled';

export interface Conversation {
  _id: number;
  user: string;
  thread?: string;
  messages: Message[];
  resources: unknown[];
  artifacts: unknown[];
  status: ConversationStatus;
  failed_reason?: string | null;
  period: number;
  created_at: number;
  updated_at: number;
  usage: Usage;
  steering_messages?: string[];
  follow_up_messages?: string[];
  ancestors?: number[];
}

export interface ServiceInfo {
  name: string;
  version: string;
  sharding: number;
  description: string;
}
```

---

## 3) 接口列表

## 3.1 公共接口

### GET `/`

- 说明：返回产品网页（HTML 或 Markdown）。
- 鉴权：无
- 响应：`text/html` 或 `text/markdown`

### GET `/info`

- 说明：服务信息
- 鉴权：无
- 响应（JSON）：`ServiceInfo`

### GET `/SKILL.md`

- 说明：返回技能描述 Markdown
- 鉴权：无
- 响应：`text/markdown`

---

## 3.2 空间业务接口（`/v1/{space_id}`）

> `space_id` 格式：`s{sharding}-{id}`，且 sharding 必须与服务实例一致。

### POST `/v1/{space_id}/formation`

- 作用：提交记忆写入任务
- 鉴权：SpaceToken/CWT `write`
- 请求体：`FormationInput`（Markdown 模式下也允许原始字符串）
- 响应（JSON/CBOR）：`RpcResponse<AgentOutput>`
- 响应（Markdown）：`string`（仅返回 `AgentOutput.content`）

### POST `/v1/{space_id}/recall`

- 作用：按自然语言召回记忆
- 鉴权：SpaceToken/CWT `read`（公开空间免鉴权，私有空间需有效 token）
- 请求体：`RecallInput`（Markdown 模式下也允许原始字符串）
- 响应：`RpcResponse<AgentOutput>`

### POST `/v1/{space_id}/maintenance`

- 作用：触发维护（睡眠/整理）
- 鉴权：SpaceToken/CWT `write`
- 请求体：`MaintenanceInput`（Markdown 模式下也允许原始字符串）
- 响应：`RpcResponse<AgentOutput>`

### GET `/v1/{space_id}/status`

- 作用：获取空间状态和统计
- 鉴权：SpaceToken/CWT `read`（公开空间免鉴权，私有空间需有效 token）
- 响应：`RpcResponse<SpaceStatus>`

### GET `/v1/{space_id}/conversations/{conversation_id}`

- 作用：获取单条会话详情
- 鉴权：SpaceToken/CWT `read`（公开空间免鉴权，私有空间需有效 token）
- 响应：`RpcResponse<Conversation>`

### GET `/v1/{space_id}/conversations?cursor=<cursor>&limit=<n>`

- 作用：分页列出会话
- 鉴权：SpaceToken/CWT `read`（公开空间免鉴权，私有空间需有效 token）
- Query:
  - `cursor?: string`
  - `limit?: number`
- 响应：`RpcResponse<Conversation[]>`（并通过 `next_cursor` 给出下一页游标）

---

## 3.3 空间管理接口（`/v1/{space_id}/management`）

### GET `/v1/{space_id}/management/space_tokens`

- 作用：列出 Space Token
- 鉴权：必须通过 CWT `read`（用户管理级鉴权）
- 响应：`RpcResponse<SpaceToken[]>`

### POST `/v1/{space_id}/management/add_space_token`

- 作用：新增 Space Token
- 鉴权：必须通过 CWT `write`（用户管理级鉴权）
- 请求体：`AddSpaceTokenInput`
- 响应：`RpcResponse<string>`（新 token，前缀通常为 `ST`）

### POST `/v1/{space_id}/management/revoke_space_token`

- 作用：吊销 Space Token
- 鉴权：必须通过 CWT `write`（用户管理级鉴权）
- 请求体：`RevokeSpaceTokenInput`
- 响应：`RpcResponse<boolean>`（是否成功吊销）

### POST `/v1/{space_id}/management/set_public`

- 作用：设置空间公开/私有
- 鉴权：必须通过 CWT `write`（用户管理级鉴权）
- 请求体：`SetSpacePublicInput`
- 响应：`RpcResponse<true>`

---

## 3.4 管理员接口（`/admin`）

### POST `/admin/create_space`

- 作用：创建空间
- 鉴权：平台管理员 + CWT `write`
- 请求体：`CreateOrUpdateSpaceInput`
- 响应：`RpcResponse<SpaceStatus>`

### POST `/admin/update_space_tier`

- 作用：更新空间 tier
- 鉴权：平台管理员 + CWT `write`
- 请求体：`CreateOrUpdateSpaceInput`
- 响应：`RpcResponse<SpaceTier>`

---

## 4) 前端调用示例（TS）

```ts
async function rpcPost<TReq, TRes>(
  url: string,
  body: TReq,
  token?: string
): Promise<RpcResponse<TRes>> {
  const res = await fetch(url, {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
      Accept: 'application/json',
      ...(token ? { Authorization: `Bearer ${token}` } : {}),
    },
    body: JSON.stringify(body),
  });

  return (await res.json()) as RpcResponse<TRes>;
}

// Recall
const recall = await rpcPost<RecallInput, AgentOutput>(
  '/v1/s0-demo/recall',
  { query: '这个用户的偏好是什么？', context: { user: 'u1' } },
  'YOUR_TOKEN'
);

if (recall.error) {
  console.error(recall.error.message);
} else {
  console.log(recall.result?.content);
}
```

---

## 5) 错误语义

- 认证失败：HTTP `401`，响应体为 `RpcError`
- 参数错误：HTTP `400`，响应体为 `RpcError`
- 成功时：HTTP `200`，响应体通常为 `RpcResponse<T>`
