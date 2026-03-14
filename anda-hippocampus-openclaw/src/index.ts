import { Type, type Static } from '@sinclair/typebox'
import type { OpenClawPluginApi, AnyAgentTool } from 'openclaw/plugin-sdk'
import { HippocampusClient } from './client.ts'
import type { HippocampusPluginConfig, InputContext, Message } from './types.ts'

export type { HippocampusPluginConfig, InputContext }
export { HippocampusClient }

// ---------------------------------------------------------------------------
// recall_memory tool — TypeBox schema
// ---------------------------------------------------------------------------

const RecallContextSchema = Type.Optional(
  Type.Object({
    user: Type.Optional(
      Type.String({
        description:
          'The identifier of the user currently being interacted with, if applicable.'
      })
    ),
    agent: Type.Optional(
      Type.String({
        description:
          'The identifier of the calling business agent, if applicable.'
      })
    ),
    topic: Type.Optional(
      Type.String({
        description:
          'The topic of the current conversation, to help disambiguate the query.'
      })
    )
  })
)

const RecallParamsSchema = Type.Object({
  query: Type.String({
    description:
      "A natural language question or description of what information to retrieve from memory. Be specific and include relevant context. Examples: 'What are Alice's communication preferences?', 'What happened in our last discussion about Project Aurora?', 'Who are the members of the engineering team?', 'What decisions were made about the pricing strategy?'"
  }),
  context: RecallContextSchema
})

type RecallParams = Static<typeof RecallParamsSchema>

// ---------------------------------------------------------------------------
// Message conversion: AgentMessage → Hippocampus Message
// ---------------------------------------------------------------------------

function extractTextContent(content: unknown): string {
  if (typeof content === 'string') return content
  if (!Array.isArray(content)) return ''
  return content
    .map((part: { type?: string; text?: string }) => {
      if (typeof part === 'string') return part
      if (part.type === 'text' && typeof part.text === 'string')
        return part.text
      return ''
    })
    .filter(Boolean)
    .join('\n')
}

function convertAgentMessages(agentMessages: unknown[]): Message[] {
  const result: Message[] = []
  for (const msg of agentMessages) {
    if (typeof msg !== 'object' || msg === null) continue
    const m = msg as Record<string, unknown>
    const role = m['role'] as string | undefined

    if (role === 'user') {
      const text = extractTextContent(m['content'])
      if (text) {
        result.push({
          role: 'user',
          content: text,
          timestamp:
            typeof m['timestamp'] === 'number'
              ? new Date(m['timestamp']).toISOString()
              : undefined
        })
      }
    } else if (role === 'assistant') {
      const parts = m['content']
      if (Array.isArray(parts)) {
        const text = extractTextContent(parts)
        if (text) {
          result.push({
            role: 'assistant',
            content: text,
            timestamp:
              typeof m['timestamp'] === 'number'
                ? new Date(m['timestamp']).toISOString()
                : undefined
          })
        }
      }
    }
    // Skip toolResult and custom messages — not useful for formation
  }
  return result
}

// ---------------------------------------------------------------------------
// Plugin factory
// ---------------------------------------------------------------------------

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
export function createHippocampusPlugin(config: HippocampusPluginConfig) {
  const client = new HippocampusClient(config)
  const defaultContext = config.defaultContext

  return {
    id: 'anda-hippocampus',
    name: 'Anda Hippocampus',
    description:
      'Persistent long-term memory via Anda Hippocampus. Encodes conversations and provides recall_memory tool.',
    version: '0.1.0',

    register(api: OpenClawPluginApi) {
      // ── recall_memory tool ──────────────────────────────────────────
      const recallTool: AnyAgentTool = {
        name: 'recall_memory',
        label: 'Recall Memory',
        description:
          "Recall information from your long-term memory (Cognitive Nexus). Send a natural language query describing what you want to remember or look up — the memory system will search and return relevant knowledge, including facts, preferences, relationships, past events, and any other stored information. Use this whenever you need context from previous interactions or stored knowledge to answer the user's question.",
        parameters: RecallParamsSchema,

        async execute(_toolCallId: string, params: RecallParams) {
          const query = params.query?.trim()
          if (!query) {
            return {
              content: [
                {
                  type: 'text' as const,
                  text: 'Error: "query" parameter is required and must be a non-empty string.'
                }
              ],
              details: { error: true }
            }
          }

          const callContext: InputContext = {
            ...defaultContext,
            ...(params.context ?? {})
          }

          const res = await client.recall(query, callContext)
          if (res.error) {
            return {
              content: [
                {
                  type: 'text' as const,
                  text: `Error recalling memory: ${res.error.message}`
                }
              ],
              details: { error: true }
            }
          }

          return {
            content: [
              {
                type: 'text' as const,
                text: res.result?.content ?? 'No relevant memory found.'
              }
            ],
            details: {
              conversation: res.result?.conversation,
              model: res.result?.model
            }
          }
        }
      }

      api.registerTool(recallTool)

      // ── agent_end hook → formation (fire-and-forget) ────────────────
      api.on('agent_end', (event) => {
        const messages = convertAgentMessages(
          (event.messages as unknown[]) ?? []
        )
        if (messages.length === 0) return

        const context: InputContext = { ...defaultContext }
        client.formation(messages, context).catch((err) => {
          api.logger.error(
            `[anda-hippocampus] Formation failed: ${err instanceof Error ? err.message : String(err)}`
          )
        })
      })
    }
  }
}
