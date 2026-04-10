# Anda Hippocampus API Documentation (with TypeScript Types)

## 1) Common Conventions

- Base URL: `http://{host}:{port}`
- Auth header: `Authorization: Bearer <token>`
- If `ED25519_PUBKEYS` is empty/not provided, authentication is disabled.
- Supported serialization formats:
  - Request: `Content-Type: application/json | application/cbor | text/markdown`
  - Response: `Accept: application/json | application/cbor | text/markdown`
- Most business endpoints return an RPC envelope: `RpcResponse<T>`

---

## 2) TypeScript Type Definitions

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
  counterparty?: string;
  agent?: string;
  source?: string;
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
  name?: string;  // user or tool name
  user?: string;  // user ID
  timestamp?: number; // Unix timestamp in milliseconds
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
  name: string;
  expires_at?: number; // Unix timestamp in milliseconds
}

export interface RevokeSpaceTokenInput {
  token: string;
}

export interface UpdateSpaceInput {
  name?: string;
  description?: string;
  public?: boolean;
}

export interface FormationRestartInput {
  conversation: number;
}

export interface CreateOrUpdateSpaceInput {
  user: string;
  space_id: string;
  tier: number;
}

export interface ModelConfig {
  family: string; // "gemini", "anthropic", "openai", "deepseek", "mimo" etc.
  model: string;
  api_base: string;
  api_key: string;
  disabled: boolean;
}

export interface SpaceTier {
  tier: number;
  updated_at: number; // Unix timestamp in milliseconds
}

export interface SpaceToken {
  token: string;
  name: string;
  scope: TokenScope;
  usage: number;
  created_at: number; // Unix timestamp in milliseconds
  updated_at: number; // Unix timestamp in milliseconds
  expires_at?: number; // Unix timestamp in milliseconds
}

export interface StorageStats {
  [k: string]: number | string | boolean | null;
}

export interface SpaceInfo {
  id: string;
  name?: string;
  description?: string;
  owner: string;
  db_stats: StorageStats;
  concepts: number;
  propositions: number;
  conversations: number;
  public: boolean;
  tier: SpaceTier;
  formation_usage: Usage;
  recall_usage: Usage;
  maintenance_usage: Usage;
  formation_processed_id: number;
  maintenance_processed_id: number;
  maintenance_at: MaintenanceAt;
}

export interface FormationStatus {
  id: string;
  concepts: number;
  propositions: number;
  conversations: number;
  formation_processing: boolean;
  maintenance_processing: boolean;
  formation_processed_id: number;
  maintenance_processed_id: number;
  maintenance_at: MaintenanceAt;
}

export interface MaintenanceAt {
  daydream: number;
  full: number;
  quick: number;
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
  label?: string;
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

export type KipCommandItem = string | { command: string; parameters: Record<string, unknown> };

export interface KipRequest {
  commands: KipCommandItem[];
  parameters?: Record<string, unknown>;
  dry_run?: boolean; // if true, the request will be parsed and validated but not executed (no side effects)
}

export interface KipError {
  code: string;
  message: string;
  hint?: string;
  data?: unknown;
}

export interface KipResponse<T> {
  result?: T;
  error?: KipError;
  next_cursor?: string;
}
```

---

## 3) Endpoint List

## 3.1 Public Endpoints

### GET `/`

- Description: Returns the product website (HTML or Markdown).
- Auth: None
- Response: `text/html` or `text/markdown`

### GET `/info`

- Description: Service information
- Auth: None
- Response (JSON): `ServiceInfo`

### GET `/SKILL.md`

- Description: Returns the skill description in Markdown
- Auth: None
- Response: `text/markdown`

---

## 3.2 Space Business Endpoints (`/v1/{space_id}`)

### POST `/v1/{space_id}/formation`

- Purpose: Submit a memory formation task
- Auth: SpaceToken/CWT `write`
- Request body: `FormationInput` (raw string is also accepted in Markdown mode)
- Response (JSON/CBOR): `RpcResponse<AgentOutput>`
- Response (Markdown): `string` (returns only `AgentOutput.content`)

### POST `/v1/{space_id}/recall`

- Purpose: Recall memory via natural-language query
- Auth: SpaceToken/CWT `read` (public spaces are unauthenticated; private spaces require a valid token)
- Request body: `RecallInput` (raw string is also accepted in Markdown mode)
- Response: `RpcResponse<AgentOutput>`

### POST `/v1/{space_id}/maintenance`

- Purpose: Trigger maintenance (sleep/consolidation)
- Auth: SpaceToken/CWT `write`
- Request body: `MaintenanceInput` (raw string is also accepted in Markdown mode)
- Response: `RpcResponse<AgentOutput>`

### POST `/v1/{space_id}/execute_kip_readonly`

- Purpose: Execute a KIP request (read-only mode, suitable for queries)
- Auth: SpaceToken/CWT `read` (public spaces are unauthenticated; private spaces require a valid token)
- Request body: `KipRequest`
- Response: `KipResponse<T>` (returns different result types based on the commands

### GET `/v1/{space_id}/info`

- Purpose: Get space status and statistics
- Auth: SpaceToken/CWT `read` (public spaces are unauthenticated; private spaces require a valid token)
- Response: `RpcResponse<SpaceInfo>`

### GET `/v1/{space_id}/formation_status`

- Purpose: Get formation status
- Auth: SpaceToken/CWT `read` (public spaces are unauthenticated; private spaces require a valid token)
- Response: `RpcResponse<FormationStatus>`

### GET `/v1/{space_id}/conversations/{conversation_id}?collection=<collection>`

- Purpose: Get a single conversation detail
- Auth: SpaceToken/CWT `read` (public spaces are unauthenticated; private spaces require a valid token)
- Query:
  - `collection?: string` // use "recall" to distinguish recall vs memory conversations
- Response: `RpcResponse<Conversation>`

### GET `/v1/{space_id}/conversations?collection=<collection>&cursor=<cursor>&limit=<n>`

- Purpose: List conversations with pagination
- Auth: SpaceToken/CWT `read` (public spaces are unauthenticated; private spaces require a valid token)
- Query:
  - `collection?: string` // use "recall" to distinguish recall vs memory conversations
  - `cursor?: string`
  - `limit?: number`
- Response: `RpcResponse<Conversation[]>` (next page cursor is returned via `next_cursor`)

---

## 3.3 Space Management Endpoints (`/v1/{space_id}/management`)

### GET `/v1/{space_id}/management/space_tokens`

- Purpose: List Space Tokens
- Auth: Must pass CWT `read` (user management-level auth)
- Response: `RpcResponse<SpaceToken[]>`

### POST `/v1/{space_id}/management/add_space_token`

- Purpose: Add a Space Token
- Auth: Must pass CWT `write` (user management-level auth)
- Request body: `AddSpaceTokenInput`
- Response: `RpcResponse<SpaceToken>` (new token, usually prefixed with `ST`)

### POST `/v1/{space_id}/management/revoke_space_token`

- Purpose: Revoke a Space Token
- Auth: Must pass CWT `write` (user management-level auth)
- Request body: `RevokeSpaceTokenInput`
- Response: `RpcResponse<boolean>` (whether revocation succeeded)

### PATCH `/v1/{space_id}/management/update_space`

- Purpose: Update space information (name, description, public/private)
- Auth: Must pass CWT `write` (user management-level auth)
- Request body: `UpdateSpaceInput`
- Response: `RpcResponse<true>`

### PATCH `/v1/{space_id}/management/restart_formation`
- Purpose: Restart a formation task by conversation ID (for failed/stale formations)
- Auth: Must pass CWT `write` (user management-level auth)
- Request body: `FormationRestartInput`
- Response: `RpcResponse<true>`

### GET `/v1/{space_id}/management/space_byok`
- Purpose: Get BYOK (Bring Your Own Key) configuration, i.e., use custom model configuration
- Auth: Must pass CWT `read` (user management-level auth)
- Response: `RpcResponse<ModelConfig>`

### PATCH `/v1/{space_id}/management/space_byok`
- Purpose: Update BYOK (Bring Your Own Key) configuration, i.e., use custom model configuration
- Auth: Must pass CWT `write` (user management-level auth)
- Request body: `ModelConfig`
- Response: `RpcResponse<true>`

---

## 3.4 Admin Endpoints (`/admin`)

### POST `/admin/create_space`

- Purpose: Create a space
- Auth: Platform admin + CWT `write`
- Request body: `CreateOrUpdateSpaceInput`
- Response: `RpcResponse<SpaceInfo>`

### POST `/admin/{space_id}/update_space_tier`

- Purpose: Update space tier
- Auth: Platform admin + CWT `write`
- Request body: `CreateOrUpdateSpaceInput`
- Response: `RpcResponse<SpaceTier>`

---

## 4) Frontend Call Example (TS)

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
  '/v1/my_space_001/recall',
  { query: 'What are this user\'s preferences?', context: { counterparty: 'user_1' } },
  'YOUR_TOKEN'
);

if (recall.error) {
  console.error(recall.error.message);
} else {
  console.log(recall.result?.content);
}
```

---

## 5) Error Semantics

- Authentication failure: HTTP `401`, response body is `RpcError`
- Invalid request/parameters: HTTP `400`, response body is `RpcError`
- Success: HTTP `200`, response body is usually `RpcResponse<T>`
