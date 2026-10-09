<script lang="ts" generics="T extends string">
  /**
   * The app's one dropdown, on bits-ui DropdownMenu. Two shapes out of one
   * primitive:
   *
   * - **Select** (default) — bordered like an input and as wide as its
   *   container. Picking writes `value`, which is bindable.
   * - **Bare** — pass a `trigger` snippet and the select chrome comes off; the
   *   trigger becomes an inline text control (the desktop composer's memory
   *   and model pickers, a bookmark's "Add to folder").
   *
   * The content panel caps its height and scrolls; pass `searchable` (with a
   * localized `searchPlaceholder`) for long vocabularies like Git branches —
   * the filter box is pinned above the scrolling item list.
   *
   * Action menus can give an item a second line (`description`), a rule above
   * it (`separator`), its own check mark (`checked`, for a toggle among
   * commands) and `tone: 'danger'` for a destructive command; `heading` puts
   * a caption above the items.
   */
  import { cn } from '$lib/utils'
  import { Check, ChevronDown } from '@lucide/svelte'
  import { DropdownMenu } from 'bits-ui'
  import type { Snippet } from 'svelte'

  type Item = {
    value: T
    label: string
    description?: string
    separator?: boolean
    checked?: boolean
    tone?: 'danger'
  }

  let {
    items,
    value = $bindable(),
    onSelect,
    id,
    ariaLabel,
    title,
    dir = document.documentElement.dir === 'rtl' ? 'rtl' : 'ltr',
    disabled = false,
    searchable = false,
    searchPlaceholder,
    align = 'start',
    sideOffset = 6,
    trigger,
    heading,
    class: className
  }: {
    items: readonly Item[]
    /** Left off only by action menus, whose items are commands, not choices. */
    value?: T
    /** Picking is an ACTION rather than an assignment — supply this and `value`
     * is never written. The language switcher needs it: choosing a language
     * saves it and reloads the window, so its `value` is derived state that
     * cannot be assigned to. Callers that just want the picked value should
     * leave this off and `bind:value` instead. */
    onSelect?: (value: T) => void
    /** Lets a `<label for>` elsewhere point at the trigger. */
    id?: string
    /** Names the control for a screen reader, which hears it followed by the
     * current choice. Pass it even inside a `<label>`: a label names the
     * trigger on its own and drops the choice. */
    ariaLabel?: string
    /** Native tooltip. Defaults to the selected label, which is the useful
     * thing for a select whose text is truncated. */
    title?: string
    /** Defaults to the page's, so an Arabic menu aligns and reads right to left. */
    dir?: 'ltr' | 'rtl'
    disabled?: boolean
    searchable?: boolean
    searchPlaceholder?: string
    align?: 'start' | 'center' | 'end'
    sideOffset?: number
    /** Replaces the whole trigger content — **chevron included** — and takes
     * the select chrome off. It gets the selected item, if any. */
    trigger?: Snippet<[Item | undefined]>
    heading?: string
    class?: string
  } = $props()

  let open = $state(false)
  let query = $state('')

  const selected = $derived(items.find((item) => item.value === value))
  // An `aria-label` replaces the trigger's content, so it carries the choice.
  const accessibleName = $derived(
    ariaLabel && selected ? `${ariaLabel}, ${selected.label}` : ariaLabel
  )

  /** The one place a pick lands, so the search box's Enter and a click on a row
   * cannot drift apart. Re-picking the current choice is a no-op, as it is for
   * a native select. Assignment happens only on the bindable path. */
  function pick(next: T) {
    if (next === value) return
    if (onSelect) onSelect(next)
    else value = next
  }

  const filtered = $derived.by(() => {
    const q = query.trim().toLowerCase()
    if (!searchable || !q) return items
    return items.filter((item) => item.label.toLowerCase().includes(q))
  })

  $effect(() => {
    if (!open) query = ''
  })

  // bits-ui returns focus to the trigger once the menu has finished closing.
  // A pick that already moved focus on (Rename opens a dialog) keeps it.
  function keepMovedFocus(event: Event) {
    const active = document.activeElement
    if (
      active &&
      active !== document.body &&
      !active.closest('[data-slot="dropdown-menu-content"]')
    )
      event.preventDefault()
  }

  // bits-ui focuses the content element on open; grab focus for the filter
  // box one frame later so it wins.
  function focusOnOpen(node: HTMLInputElement) {
    const raf = requestAnimationFrame(() => node.focus())
    return { destroy: () => cancelAnimationFrame(raf) }
  }

  function onSearchKeydown(e: KeyboardEvent) {
    if (e.key === 'Enter') {
      e.preventDefault()
      const first = filtered[0]
      if (first) {
        pick(first.value)
        open = false
      }
      return
    }
    // Arrows walk into the list, Escape/Tab close — everything else stays
    // in the input (the menu would otherwise typeahead-steal focus).
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp' || e.key === 'Escape' || e.key === 'Tab')
      return
    e.stopPropagation()
  }
</script>

<DropdownMenu.Root {dir} bind:open>
  <DropdownMenu.Trigger
    {id}
    {disabled}
    aria-label={accessibleName}
    data-slot="dropdown-menu-trigger"
    title={title ?? selected?.label}
    class={cn(
      'group items-center gap-1.5 transition-[color,box-shadow] outline-none select-none disabled:pointer-events-none disabled:opacity-50',
      trigger
        ? 'inline-flex min-w-0 rounded-md hover:text-foreground focus-visible:text-foreground data-[state=open]:text-foreground'
        : 'flex h-9 w-full min-w-0 justify-between rounded-md border border-input bg-transparent px-2.5 py-1 text-sm shadow-xs focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50 data-[state=open]:border-ring dark:bg-input/30 dark:hover:bg-input/50',
      className
    )}
  >
    {#if trigger}
      {@render trigger(selected)}
    {:else}
      <span class="truncate">{selected?.label ?? value}</span>
      <ChevronDown
        class="size-4 shrink-0 text-muted-foreground transition-transform group-data-[state=open]:rotate-180"
        aria-hidden="true"
      />
    {/if}
  </DropdownMenu.Trigger>

  <DropdownMenu.Portal>
    <DropdownMenu.Content
      {sideOffset}
      {align}
      onCloseAutoFocus={keepMovedFocus}
      data-slot="dropdown-menu-content"
      class={cn(
        'z-50 flex max-h-[min(24rem,var(--bits-dropdown-menu-content-available-height))] max-w-[min(28rem,var(--bits-dropdown-menu-content-available-width))] min-w-[max(8rem,var(--bits-dropdown-menu-anchor-width))] origin-(--bits-dropdown-menu-content-transform-origin) flex-col rounded-md border bg-popover p-1 text-sm text-popover-foreground shadow-md',
        'data-[state=closed]:animate-out data-[state=closed]:fade-out-0 data-[state=closed]:zoom-out-95 data-[state=open]:animate-in data-[state=open]:fade-in-0 data-[state=open]:zoom-in-95'
      )}
    >
      {#if searchable}
        <div class="border-b px-1 pt-0.5 pb-1.5">
          <input
            type="text"
            class="h-8 w-full rounded-sm border border-input bg-transparent px-2 outline-none placeholder:text-muted-foreground focus:border-ring"
            placeholder={searchPlaceholder}
            aria-label={searchPlaceholder ?? ariaLabel}
            bind:value={query}
            use:focusOnOpen
            onkeydown={onSearchKeydown}
          />
        </div>
      {/if}
      {#if heading}
        <div
          class="px-2 pt-1 pb-1.5 text-xs text-muted-foreground"
          data-slot="dropdown-menu-heading"
        >
          {heading}
        </div>
      {/if}
      <div class="min-h-0 flex-1 overflow-x-hidden overflow-y-auto {searchable ? 'pt-1' : ''}">
        {#each filtered as item (item.value)}
          {@const active = item.checked ?? item.value === value}
          {#if item.separator}
            <DropdownMenu.Separator class="my-1 h-px bg-border" />
          {/if}
          <DropdownMenu.Item
            onSelect={() => pick(item.value)}
            aria-current={active || undefined}
            data-tone={item.tone}
            class={cn(
              'flex cursor-default items-center justify-between gap-5 rounded-sm px-2 py-1.5 outline-none select-none data-highlighted:bg-accent data-highlighted:text-accent-foreground',
              active && 'font-medium',
              item.tone === 'danger' && 'text-destructive data-highlighted:text-destructive'
            )}
          >
            {#if item.description}
              <span class="grid min-w-0 flex-1">
                <span class="truncate">{item.label}</span>
                <span class="truncate text-xs font-normal text-muted-foreground"
                  >{item.description}</span
                >
              </span>
            {:else}
              <span class="truncate">{item.label}</span>
            {/if}
            {#if active}
              <Check class="size-4 shrink-0" aria-hidden="true" />
            {/if}
          </DropdownMenu.Item>
        {/each}
      </div>
    </DropdownMenu.Content>
  </DropdownMenu.Portal>
</DropdownMenu.Root>
