import { OpenClawPluginApi } from 'openclaw/plugin-sdk';

/**
 * TypeScript types for the Anda Hippocampus API.
 * Reference: anda_hippocampus/API.md
 */
interface RpcError {
    message: string;
    data?: unknown;
}
interface RpcResponse<T> {
    result?: T;
    error?: RpcError;
}
interface InputContext {
    user?: string;
    agent?: string;
    session?: string;
    topic?: string;
}
type MessageRole = 'system' | 'user' | 'assistant' | 'tool';
type MessageContentPart = string | {
    type: string;
    text?: string;
    [k: string]: unknown;
};
interface Message {
    role: MessageRole;
    content: string | MessageContentPart[];
    name?: string | undefined;
    user?: string | undefined;
    timestamp?: string | undefined;
}
interface Usage {
    input_tokens?: number;
    output_tokens?: number;
    total_tokens?: number;
}
interface AgentOutput {
    content: string;
    conversation?: number;
    failed_reason?: string;
    usage?: Usage;
    model?: string;
    [k: string]: unknown;
}
/**
 * Plugin configuration options.
 */
interface HippocampusPluginConfig {
    /** Anda Hippocampus base URL. Default: "https://brain.anda.ai" */
    baseUrl?: string;
    /** Memory space ID (required) */
    spaceId: string;
    /** Space token for API authentication (required) */
    spaceToken: string;
    /** Default context to include with every request */
    defaultContext?: InputContext;
    /** Request timeout for formation in ms. Default: 30000 */
    formationTimeoutMs?: number;
    /** Request timeout for recall in ms. Default: 120000 */
    recallTimeoutMs?: number;
}

/**
 * HTTP client for the Anda Hippocampus API.
 */
declare class HippocampusClient {
    private readonly baseUrl;
    private readonly spaceId;
    private readonly spaceToken;
    private readonly formationTimeoutMs;
    private readonly recallTimeoutMs;
    constructor(config: HippocampusPluginConfig);
    /**
     * Send conversation messages for memory encoding (fire-and-forget).
     * The API processes asynchronously — returns immediately with a conversation ID.
     */
    formation(messages: Message[], context?: InputContext): Promise<RpcResponse<AgentOutput>>;
    /**
     * Query memory with natural language. May take 10–100 seconds.
     */
    recall(query: string, context?: InputContext): Promise<RpcResponse<AgentOutput>>;
    private post;
}

/**
 * Create the Anda Hippocampus OpenClaw plugin.
 *
 * - Registers a `recall_memory` agent tool that queries the Recall endpoint.
 * - Hooks into `agent_end` to send conversation messages to the Formation
 *   endpoint (fire-and-forget) for memory encoding.
 *
 * @example
 * ```ts
 * import { createHippocampusPlugin } from 'anda-hippocampus'
 *
 * export default createHippocampusPlugin({
 *   spaceId: 'my_space_001',
 *   spaceToken: 'ST_xxxxx',
 * })
 * ```
 */
declare function createHippocampusPlugin(config: HippocampusPluginConfig): {
    id: string;
    name: string;
    description: string;
    version: string;
    register(api: OpenClawPluginApi): void;
};

export { HippocampusClient, type HippocampusPluginConfig, type InputContext, createHippocampusPlugin };
