<script lang="ts">
  import { ChevronDown, List } from '@lucide/svelte'
  import type { Snippet } from 'svelte'

  let {
    label,
    width = '17rem',
    sidebar,
    children
  }: {
    label: string
    width?: string
    sidebar: Snippet
    children: Snippet
  } = $props()

  const id = $props.id()
  let open = $state(false)
  let toggle: HTMLButtonElement

  /** A selection takes a compact window back to its detail and keeps keyboard focus in view. */
  export function showDetail() {
    if (!open) return
    open = false
    if (toggle.getClientRects().length) toggle.focus()
  }
</script>

<div class="@container/workspace h-full min-h-0">
  <div
    class="grid h-full min-h-0 grid-cols-[minmax(0,1fr)] grid-rows-[auto_minmax(0,1fr)] overflow-hidden @min-[44rem]/workspace:grid-cols-[var(--list-width)_minmax(0,1fr)] @min-[44rem]/workspace:grid-rows-1"
    style:--list-width={width}
  >
    <button
      type="button"
      bind:this={toggle}
      class="flex min-h-10 min-w-0 items-center gap-2 border-b bg-sidebar px-4 text-start text-sm font-medium hover:bg-muted focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-ring @min-[44rem]/workspace:hidden"
      aria-expanded={open}
      aria-controls={id}
      onclick={() => (open = !open)}
    >
      <List class="size-4 shrink-0" /><span class="min-w-0 flex-1 truncate">{label}</span>
      <ChevronDown class={`size-4 shrink-0 ${open ? 'rotate-180' : ''}`} />
    </button>
    <div
      {id}
      class={`min-h-0 min-w-0 @min-[44rem]/workspace:block [&>aside]:h-full ${open ? 'block' : 'hidden'}`}
    >
      {@render sidebar()}
    </div>
    <div
      class={`min-h-0 min-w-0 @min-[44rem]/workspace:block [&>section]:h-full ${open ? 'hidden' : 'block'}`}
    >
      {@render children()}
    </div>
  </div>
</div>
