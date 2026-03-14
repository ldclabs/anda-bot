// src/index.ts
import { Type } from "@sinclair/typebox";

// src/client.ts
var DEFAULT_BASE_URL = "https://brain.anda.ai";
var DEFAULT_FORMATION_TIMEOUT = 3e4;
var DEFAULT_RECALL_TIMEOUT = 12e4;
var HippocampusClient = class {
  baseUrl;
  spaceId;
  spaceToken;
  formationTimeoutMs;
  recallTimeoutMs;
  constructor(config) {
    this.baseUrl = (config.baseUrl ?? DEFAULT_BASE_URL).replace(/\/+$/, "");
    this.spaceId = config.spaceId;
    this.spaceToken = config.spaceToken;
    this.formationTimeoutMs = config.formationTimeoutMs ?? DEFAULT_FORMATION_TIMEOUT;
    this.recallTimeoutMs = config.recallTimeoutMs ?? DEFAULT_RECALL_TIMEOUT;
  }
  /**
   * Send conversation messages for memory encoding (fire-and-forget).
   * The API processes asynchronously — returns immediately with a conversation ID.
   */
  async formation(messages, context) {
    const body = {
      messages,
      context,
      timestamp: (/* @__PURE__ */ new Date()).toISOString()
    };
    return this.post(
      `/v1/${encodeURIComponent(this.spaceId)}/formation`,
      body,
      this.formationTimeoutMs
    );
  }
  /**
   * Query memory with natural language. May take 10–100 seconds.
   */
  async recall(query, context) {
    const body = { query, context };
    return this.post(
      `/v1/${encodeURIComponent(this.spaceId)}/recall`,
      body,
      this.recallTimeoutMs
    );
  }
  async post(path, body, timeoutMs) {
    const url = `${this.baseUrl}${path}`;
    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), timeoutMs);
    try {
      const res = await fetch(url, {
        method: "POST",
        headers: {
          "Content-Type": "application/json",
          Accept: "application/json",
          Authorization: `Bearer ${this.spaceToken}`
        },
        body: JSON.stringify(body),
        signal: controller.signal
      });
      if (!res.ok) {
        const text = await res.text().catch(() => "");
        return {
          error: {
            message: `HTTP ${res.status}: ${text || res.statusText}`
          }
        };
      }
      return await res.json();
    } catch (err) {
      const message = err instanceof DOMException && err.name === "AbortError" ? `Request timed out after ${timeoutMs}ms` : `Request failed: ${err instanceof Error ? err.message : String(err)}`;
      return { error: { message } };
    } finally {
      clearTimeout(timer);
    }
  }
};

// src/index.ts
var RecallContextSchema = Type.Optional(
  Type.Object({
    user: Type.Optional(
      Type.String({
        description: "The identifier of the user currently being interacted with, if applicable."
      })
    ),
    agent: Type.Optional(
      Type.String({
        description: "The identifier of the calling business agent, if applicable."
      })
    ),
    topic: Type.Optional(
      Type.String({
        description: "The topic of the current conversation, to help disambiguate the query."
      })
    )
  })
);
var RecallParamsSchema = Type.Object({
  query: Type.String({
    description: "A natural language question or description of what information to retrieve from memory. Be specific and include relevant context. Examples: 'What are Alice's communication preferences?', 'What happened in our last discussion about Project Aurora?', 'Who are the members of the engineering team?', 'What decisions were made about the pricing strategy?'"
  }),
  context: RecallContextSchema
});
function extractTextContent(content) {
  if (typeof content === "string") return content;
  if (!Array.isArray(content)) return "";
  return content.map((part) => {
    if (typeof part === "string") return part;
    if (part.type === "text" && typeof part.text === "string")
      return part.text;
    return "";
  }).filter(Boolean).join("\n");
}
function convertAgentMessages(agentMessages) {
  const result = [];
  for (const msg of agentMessages) {
    if (typeof msg !== "object" || msg === null) continue;
    const m = msg;
    const role = m["role"];
    if (role === "user") {
      const text = extractTextContent(m["content"]);
      if (text) {
        result.push({
          role: "user",
          content: text,
          timestamp: typeof m["timestamp"] === "number" ? new Date(m["timestamp"]).toISOString() : void 0
        });
      }
    } else if (role === "assistant") {
      const parts = m["content"];
      if (Array.isArray(parts)) {
        const text = extractTextContent(parts);
        if (text) {
          result.push({
            role: "assistant",
            content: text,
            timestamp: typeof m["timestamp"] === "number" ? new Date(m["timestamp"]).toISOString() : void 0
          });
        }
      }
    }
  }
  return result;
}
function createHippocampusPlugin(config) {
  const client = new HippocampusClient(config);
  const defaultContext = config.defaultContext;
  return {
    id: "anda-hippocampus",
    name: "Anda Hippocampus",
    description: "Persistent long-term memory via Anda Hippocampus. Encodes conversations and provides recall_memory tool.",
    version: "0.1.0",
    register(api) {
      const recallTool = {
        name: "recall_memory",
        label: "Recall Memory",
        description: "Recall information from your long-term memory (Cognitive Nexus). Send a natural language query describing what you want to remember or look up \u2014 the memory system will search and return relevant knowledge, including facts, preferences, relationships, past events, and any other stored information. Use this whenever you need context from previous interactions or stored knowledge to answer the user's question.",
        parameters: RecallParamsSchema,
        async execute(_toolCallId, params) {
          const query = params.query?.trim();
          if (!query) {
            return {
              content: [
                {
                  type: "text",
                  text: 'Error: "query" parameter is required and must be a non-empty string.'
                }
              ],
              details: { error: true }
            };
          }
          const callContext = {
            ...defaultContext,
            ...params.context ?? {}
          };
          const res = await client.recall(query, callContext);
          if (res.error) {
            return {
              content: [
                {
                  type: "text",
                  text: `Error recalling memory: ${res.error.message}`
                }
              ],
              details: { error: true }
            };
          }
          return {
            content: [
              {
                type: "text",
                text: res.result?.content ?? "No relevant memory found."
              }
            ],
            details: {
              conversation: res.result?.conversation,
              model: res.result?.model
            }
          };
        }
      };
      api.registerTool(recallTool);
      api.on("agent_end", (event) => {
        const messages = convertAgentMessages(
          event.messages ?? []
        );
        if (messages.length === 0) return;
        const context = { ...defaultContext };
        client.formation(messages, context).catch((err) => {
          api.logger.error(
            `[anda-hippocampus] Formation failed: ${err instanceof Error ? err.message : String(err)}`
          );
        });
      });
    }
  };
}
export {
  HippocampusClient,
  createHippocampusPlugin
};
