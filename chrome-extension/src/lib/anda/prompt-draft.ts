export const promptDraftRequestStorageKey = 'andaPromptDraftRequest'
export const promptDraftRequestMaxAgeMs = 5 * 60 * 1000

export interface PromptDraftRequest {
  id: string
  createdAt: number
  text: string
}

export function createPromptDraftRequest(text: string): PromptDraftRequest {
  return {
    id: globalThis.crypto?.randomUUID?.() || `${Date.now()}-${Math.random().toString(36).slice(2)}`,
    createdAt: Date.now(),
    text
  }
}

export function isPromptDraftRequest(value: unknown): value is PromptDraftRequest {
  if (!value || typeof value !== 'object') {
    return false
  }
  const request = value as Partial<PromptDraftRequest>
  return (
    typeof request.id === 'string' &&
    typeof request.createdAt === 'number' &&
    typeof request.text === 'string' &&
    request.text.trim().length > 0
  )
}
