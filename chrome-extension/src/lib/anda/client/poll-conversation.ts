import type { ChatMessage } from './types'

type PollConversationItem = IteratorResult<ChatMessage, null>

export class PollConversation implements AsyncIterable<ChatMessage> {
  #messages: ChatMessage[] = []
  #waiters: ((item: PollConversationItem) => void)[] = []
  #producerDone = false
  #consumerDone = false

  push(...messages: ChatMessage[]): void {
    if (messages.length === 0 || this.#producerDone || this.#consumerDone) {
      return
    }

    this.#messages.push(...messages)
    this.drain()
  }

  finish(): void {
    if (this.#producerDone) {
      return
    }

    this.#producerDone = true
    this.drain()
  }

  close(): void {
    if (this.#consumerDone) {
      return
    }

    this.#consumerDone = true
    this.#messages = []
    this.drain()
  }

  drain(): void {
    while (this.#drainOne()) {}
  }

  #drainOne(): boolean {
    if (this.#waiters.length === 0) {
      return false
    }

    if (this.#messages.length > 0) {
      const waiter = this.#waiters.shift()!
      const value = this.#messages.shift()!
      waiter({ value, done: false })
      return true
    }

    if (!this.#producerDone && !this.#consumerDone) {
      return false
    }

    const waiter = this.#waiters.shift()!
    waiter({ value: null, done: true })
    return true
  }

  [Symbol.asyncIterator](): AsyncIterator<ChatMessage, null, undefined> {
    return {
      next: () => {
        if (this.#consumerDone) {
          return Promise.resolve({ value: null, done: true })
        }

        return new Promise<PollConversationItem>((resolve) => {
          this.#waiters.push(resolve)
          this.drain()
        })
      },

      return: () => {
        this.close()
        return Promise.resolve({ value: null, done: true })
      }
    }
  }
}
