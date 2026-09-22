import type { BrainGraphSettings } from '../brain/api'
import { MemoryApi, type Activity } from './api'

/** One poller for the visible conversation, independent of assistant completion. */
export class ConversationMemoryActivity {
  messages = $state<Record<string, Activity>>({})
  error = $state('')
  #key = ''
  #generation = 0
  #timer: ReturnType<typeof setTimeout> | undefined
  #controller: AbortController | undefined
  #settings: BrainGraphSettings | null = null
  #conversation = ''
  #working = false
  #emptyChecks = 0

  configure(settings: BrainGraphSettings, conversation: string, working: boolean) {
    const key = JSON.stringify([settings.baseUrl, settings.spaceId, settings.token, conversation])
    if (key === this.#key) {
      if (this.#working !== working) {
        this.#working = working
        this.#schedule(0)
      }
      return
    }
    this.stop()
    this.#key = key
    this.#settings = settings
    this.#conversation = conversation
    this.#working = working
    this.#emptyChecks = 0
    this.messages = {}
    this.error = ''
    if (settings.token && conversation && !document.hidden) this.#schedule(0)
  }
  visibilityChanged() {
    this.#generation++
    this.#controller?.abort()
    clearTimeout(this.#timer)
    if (!document.hidden && this.#settings && this.#conversation) this.#schedule(0)
  }
  stop() {
    this.#generation++
    this.#controller?.abort()
    clearTimeout(this.#timer)
    this.#key = ''
    this.messages = {}
  }
  #schedule(delay: number) {
    clearTimeout(this.#timer)
    this.#timer = setTimeout(() => void this.#refresh(), delay)
  }
  async #refresh() {
    if (!this.#settings || !this.#conversation || document.hidden) return
    const generation = ++this.#generation
    this.#controller?.abort()
    this.#controller = new AbortController()
    const signal = this.#controller.signal
    try {
      const api = new MemoryApi(this.#settings)
      const overview = await api.overview(signal)
      if (generation !== this.#generation || !overview.caller) return
      const page = await api.activity(null, signal, this.#conversation)
      if (generation !== this.#generation) return
      const messages: Record<string, Activity> = {}
      for (const item of page.items) {
        if (!item.provenance_complete) continue
        for (const source of item.source_messages) {
          if (source.conversation !== this.#conversation || source.index === null) continue
          const key = `m-${source.conversation}-${source.index}`
          if (!messages[key] || messages[key].submitted_at < item.submitted_at) messages[key] = item
        }
      }
      this.messages = messages
      this.error = ''
      if (!page.items.length) this.#emptyChecks++
      const pending = page.items.some((item) =>
        ['submitting', 'accepted', 'processing'].includes(item.state)
      )
      if (
        pending ||
        this.#working ||
        overview.memory.formation_active ||
        (!page.items.length && this.#emptyChecks < 6)
      )
        this.#schedule(5000)
      else if (
        page.partial_reason ||
        page.items.some((item) => item.stale || item.state === 'unknown')
      )
        this.#schedule(60000)
    } catch {
      if (generation !== this.#generation) return
      this.messages = {}
      this.error = 'unavailable'
      // Credentials and old servers need user attention, not endless polling.
    }
  }
}
