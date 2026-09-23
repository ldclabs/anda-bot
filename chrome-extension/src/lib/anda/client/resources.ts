import { apiResult, type DaemonApi } from './daemon'
import type { Resource } from './types'

/** Bounded, deduplicated resource reads for one daemon connection. */
export class ResourceCache {
  #cache = new Map<number, Resource>()
  #requests = new Map<number, Promise<Resource>>()
  #bytes = 0
  #generation = 0
  #active = 0
  #waiters: Array<() => void> = []

  constructor(
    private daemon: DaemonApi,
    private maxBytes = 16 * 1024 * 1024
  ) {}

  clear(): void {
    this.#generation++
    this.#cache.clear()
    this.#requests.clear()
    this.#bytes = 0
  }

  async load(resource: Resource): Promise<Resource | null> {
    const id = resource._id || 0
    if (resource.blob) return resource
    if (!id) return null
    const cached = this.#cache.get(id)
    if (cached) {
      this.#cache.delete(id)
      this.#cache.set(id, cached)
      return mergeResource(resource, cached)
    }
    let request = this.#requests.get(id)
    if (!request) {
      request = this.#read(id, this.#generation).finally(() => {
        if (this.#requests.get(id) === request) this.#requests.delete(id)
      })
      this.#requests.set(id, request)
    }
    return mergeResource(resource, await request)
  }

  async #read(id: number, generation: number): Promise<Resource> {
    if (this.#active >= 4) await new Promise<void>((resolve) => this.#waiters.push(resolve))
    else this.#active++
    try {
      if (generation !== this.#generation) throw new Error('Connection settings changed')
      const result = await apiResult<Resource>(this.daemon, 'resources_api', {
        type: 'GetResource',
        _id: id
      })
      if (generation !== this.#generation) throw new Error('Connection settings changed')
      const bytes = (result.blob?.length || 0) * 2
      // Also bound entries when a resource has no inline bytes.
      while (this.#cache.size && (this.#bytes + bytes > this.maxBytes || this.#cache.size >= 128)) {
        const [oldId, old] = this.#cache.entries().next().value!
        this.#cache.delete(oldId)
        this.#bytes -= (old.blob?.length || 0) * 2
      }
      if (bytes <= this.maxBytes) {
        this.#cache.set(id, result)
        this.#bytes += bytes
      }
      return result
    } finally {
      const next = this.#waiters.shift()
      if (next) next()
      else this.#active--
    }
  }
}

function mergeResource(summary: Resource, full: Resource): Resource {
  return {
    ...summary,
    ...full,
    tags: full.tags?.length ? full.tags : summary.tags,
    metadata: { ...summary.metadata, ...full.metadata },
    blob: full.blob || summary.blob,
    description: full.description || summary.description
  }
}
