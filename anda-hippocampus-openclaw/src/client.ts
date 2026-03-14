import type {
  AgentOutput,
  FormationInput,
  HippocampusPluginConfig,
  Message,
  InputContext,
  RecallInput,
  RpcResponse
} from './types.ts'

const DEFAULT_BASE_URL = 'https://brain.anda.ai'
const DEFAULT_FORMATION_TIMEOUT = 30_000
const DEFAULT_RECALL_TIMEOUT = 120_000

/**
 * HTTP client for the Anda Hippocampus API.
 */
export class HippocampusClient {
  private readonly baseUrl: string
  private readonly spaceId: string
  private readonly spaceToken: string
  private readonly formationTimeoutMs: number
  private readonly recallTimeoutMs: number

  constructor(config: HippocampusPluginConfig) {
    this.baseUrl = (config.baseUrl ?? DEFAULT_BASE_URL).replace(/\/+$/, '')
    this.spaceId = config.spaceId
    this.spaceToken = config.spaceToken
    this.formationTimeoutMs =
      config.formationTimeoutMs ?? DEFAULT_FORMATION_TIMEOUT
    this.recallTimeoutMs = config.recallTimeoutMs ?? DEFAULT_RECALL_TIMEOUT
  }

  /**
   * Send conversation messages for memory encoding (fire-and-forget).
   * The API processes asynchronously — returns immediately with a conversation ID.
   */
  async formation(
    messages: Message[],
    context?: InputContext
  ): Promise<RpcResponse<AgentOutput>> {
    const body: FormationInput = {
      messages,
      context,
      timestamp: new Date().toISOString()
    }
    return this.post<FormationInput, AgentOutput>(
      `/v1/${encodeURIComponent(this.spaceId)}/formation`,
      body,
      this.formationTimeoutMs
    )
  }

  /**
   * Query memory with natural language. May take 10–100 seconds.
   */
  async recall(
    query: string,
    context?: InputContext
  ): Promise<RpcResponse<AgentOutput>> {
    const body: RecallInput = { query, context }
    return this.post<RecallInput, AgentOutput>(
      `/v1/${encodeURIComponent(this.spaceId)}/recall`,
      body,
      this.recallTimeoutMs
    )
  }

  private async post<TReq, TRes>(
    path: string,
    body: TReq,
    timeoutMs: number
  ): Promise<RpcResponse<TRes>> {
    const url = `${this.baseUrl}${path}`
    const controller = new AbortController()
    const timer = setTimeout(() => controller.abort(), timeoutMs)

    try {
      const res = await fetch(url, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
          Accept: 'application/json',
          Authorization: `Bearer ${this.spaceToken}`
        },
        body: JSON.stringify(body),
        signal: controller.signal
      })

      if (!res.ok) {
        const text = await res.text().catch(() => '')
        return {
          error: {
            message: `HTTP ${res.status}: ${text || res.statusText}`
          }
        }
      }

      return (await res.json()) as RpcResponse<TRes>
    } catch (err) {
      const message =
        err instanceof DOMException && err.name === 'AbortError'
          ? `Request timed out after ${timeoutMs}ms`
          : `Request failed: ${err instanceof Error ? err.message : String(err)}`
      return { error: { message } }
    } finally {
      clearTimeout(timer)
    }
  }
}
