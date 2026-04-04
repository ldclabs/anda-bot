import type { OpenClawPluginApi, AnyAgentTool } from 'openclaw/plugin-sdk'
import packageJson from '../package.json'
import { HippocampusClient } from './client.ts'
import type { HippocampusPluginConfig, InputContext, Message } from './types.ts'

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
            typeof m['timestamp'] === 'number' ? m['timestamp'] : undefined
        })
      }
    } else if (role === 'assistant') {
      const parts = m['content']
      const text = extractTextContent(parts)
      if (text) {
        result.push({
          role: 'assistant',
          content: text,
          timestamp:
            typeof m['timestamp'] === 'number' ? m['timestamp'] : undefined
        })
      }
    }
    // Skip toolResult and custom messages — not useful for formation
  }
  return result
}

const andaHippocampusPlugin = {
  id: 'anda-hippocampus',
  name: 'Anda Hippocampus',
  description:
    'Autonomous graph memory for OpenClaw agents. Encodes conversations to a knowledge graph and provides recall_memory tool to retrieve memory in natural language. https://brain.anda.ai/',
  version: packageJson.version,

  register(api: OpenClawPluginApi) {
    const config = (api.pluginConfig ?? {}) as any as HippocampusPluginConfig
    if (config.spaceId == null || config.spaceToken == null) {
      api.logger.error(
        '[anda-hippocampus] Invalid configuration: spaceId and spaceToken are required. You can obtain them at https://anda.ai/brain'
      )
      return
    }

    const client = new HippocampusClient(config)
    const defaultContext = config.defaultContext
    // ── recall_memory tool ──────────────────────────────────────────
    const recallTool: AnyAgentTool = {
      name: 'recall_memory',
      label: 'Recall Memory',
      description:
        "Recall information from your long-term memory (Cognitive Nexus). Send a natural language query describing what you want to remember or look up — the memory system will search and return relevant knowledge, including facts, preferences, relationships, past events, and any other stored information. Use this whenever you need context from previous interactions or stored knowledge to answer the user's question.",
      parameters: {
        'type': 'object',
        'properties': {
          'query': {
            'type': 'string',
            'description':
              "A natural language question or description of what information to retrieve from memory. Be specific and include relevant context. Examples: 'What are Alice's communication preferences?', 'What happened in our last discussion about Project Aurora?', 'Who are the members of the engineering team?', 'What decisions were made about the pricing strategy?'"
          },
          'context': {
            'type': 'object',
            'description':
              'Optional current conversational context to help narrow the search. Provide any relevant identifiers or topic hints that could improve retrieval accuracy.',
            'properties': {
              'user': {
                'type': 'string',
                'description':
                  'The identifier of the user currently being interacted with, if applicable.'
              },
              'agent': {
                'type': 'string',
                'description':
                  'The identifier of the calling business agent, if applicable.'
              },
              'topic': {
                'type': 'string',
                'description':
                  'The topic of the current conversation, to help disambiguate the query.'
              }
            }
          }
        },
        'required': ['query']
      },

      async execute(
        _toolCallId: string,
        params: { query?: string; context?: Partial<InputContext> }
      ) {
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
          api.logger.error(
            `[anda-hippocampus] Recall failed: ${res.error.message}`
          )
          return {
            content: [
              {
                type: 'text' as const,
                text: `Error recalling memory: ${res.error.message}`
              }
            ],
            details: { error: true }
          }
        } else {
          api.logger.debug?.(
            `[anda-hippocampus] Recall successful: ${JSON.stringify(res.result)}.`
          )
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
    api.on('agent_end', (event, ctx) => {
      const originalMessages = (event.messages as unknown[]) ?? []
      const messages = convertAgentMessages(originalMessages)
      api.logger.info(
        `[anda-hippocampus] agent_end: extracted ${messages.length} messages for formation.`
      )

      if (messages.length === 0) return
      const context: InputContext = {
        ...defaultContext,
        agent: ctx.agentId || defaultContext?.agent
      }
      client
        .formation(messages, context)
        .then((res) => {
          api.logger.info(
            `[anda-hippocampus] Formation completed: ${JSON.stringify(res)}.`
          )
        })
        .catch((err) => {
          api.logger.error(
            `[anda-hippocampus] Formation failed: ${JSON.stringify(err)}`
          )
        })
    })

    api.logger.info(
      '[anda-hippocampus] Plugin registered successfully. Ready to encode memories and handle recall_memory tool calls.'
    )
  }
}

export default andaHippocampusPlugin
