<script lang="ts" module>
  // Each window remembers its own reading positions, including across view changes.
  const readingPositions = new Map<string, number>()
</script>

<script lang="ts">
  import { ArrowDown } from '@lucide/svelte'
  import { tick, type Snippet } from 'svelte'
  import { getMessage } from '$lib/i18n'
  import type { ChatMessage } from './client/types'

  let {
    source,
    messages,
    viewport = $bindable(null),
    scrollClass = '',
    contentClass = '',
    onScroll,
    children
  }: {
    source: string
    messages: ChatMessage[]
    viewport?: HTMLElement | null
    scrollClass?: string
    contentClass?: string
    onScroll?: () => void
    children: Snippet
  } = $props()

  let content = $state<HTMLDivElement | null>(null)
  let away = $state(false)
  let renderedSource: string | undefined
  let lastId: string | undefined

  function aboveLatest(el: HTMLElement) {
    return el.scrollHeight - el.clientHeight - el.scrollTop > 48
  }

  function measure() {
    if (!viewport) return
    away = aboveLatest(viewport)
    onScroll?.()
  }

  function remember() {
    measure()
    if (source !== renderedSource || !viewport) return
    if (away) readingPositions.set(source, viewport.scrollTop)
    else readingPositions.delete(source)
  }

  // Check the old layout before new messages change its height. Opening a
  // disclosure or reading further up never asks the timeline to follow.
  $effect.pre(() => {
    const el = viewport
    if (!el) return
    const latest = messages.at(-1)
    const currentSource = source
    if (!latest) {
      // Wait for history to load before restoring its saved scroll offset.
      renderedSource = undefined
      lastId = undefined
      return
    }
    const switched = renderedSource !== currentSource
    const ownPrompt = latest.role === 'user' && latest.id !== lastId
    const follow = !aboveLatest(el) || ownPrompt
    const position = switched ? readingPositions.get(source) : undefined
    renderedSource = source
    lastId = latest.id
    if (!switched && !follow) return
    void tick().then(() => {
      if (viewport !== el || source !== currentSource) return
      el.scrollTop = position ?? el.scrollHeight
      measure()
    })
  })

  $effect(() => {
    if (!viewport || !content) return
    const observer = new ResizeObserver(measure)
    observer.observe(viewport)
    observer.observe(content)
    return () => observer.disconnect()
  })

  function jump() {
    if (!viewport) return
    viewport.scrollTop = viewport.scrollHeight
    viewport.focus({ preventScroll: true })
    remember()
  }
</script>

<div class="timeline-shell">
  <main
    bind:this={viewport}
    class="timeline-scroll {scrollClass}"
    onscroll={remember}
    tabindex="-1"
  >
    <div bind:this={content} class="timeline-content {contentClass}">
      {@render children()}
    </div>
  </main>
  {#if away}
    <button type="button" class="jump-bottom" onclick={jump}>
      <ArrowDown size={14} aria-hidden="true" />{getMessage('jumpToLatest')}
    </button>
  {/if}
</div>

<style>
  .timeline-shell {
    position: relative;
    display: flex;
    flex: 1;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
  }
  .timeline-scroll {
    flex: 1;
    overflow-x: hidden;
    overflow-y: auto;
    min-height: 0;
  }
  .timeline-content {
    min-height: 100%;
  }
  .jump-bottom {
    position: absolute;
    inset-block-end: 0.5rem;
    left: 50%;
    transform: translateX(-50%);
    display: flex;
    align-items: center;
    gap: 0.375rem;
    border: 1px solid var(--message-border, #ddd);
    border-radius: 999px;
    min-height: 2rem;
    padding: 0.25rem 0.75rem;
    background: var(--message-bg, var(--desk-input, white));
    color: var(--message-text, var(--desk-text, #272727));
    box-shadow: 0 2px 8px #0001;
    font-size: 0.75rem;
    white-space: nowrap;
  }
  .jump-bottom:hover {
    background: var(--message-surface-hover, var(--desk-hover, #eee));
  }
  .jump-bottom:focus-visible {
    outline: 2px solid var(--chat-accent, #059669);
  }
</style>
