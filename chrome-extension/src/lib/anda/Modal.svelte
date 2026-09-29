<script lang="ts">
  /**
   * The app's one modal, on bits-ui Dialog (AlertDialog for a confirmation): a
   * title bar, a scrolling body and an optional footer for its actions. Results that can run long (a memory
   * search, an entity lookup) open here instead of stretching the page they
   * were asked from, and so do forms and flows that ask for a decision (the
   * settings, a memory change, the inbox setup, deleting a channel).
   *
   * The panel caps itself at 85vh and only the body scrolls, so the title and
   * the close button stay in reach however much the body holds. A height
   * floor keeps a loading state from opening as a splinter that then jumps.
   * The body's padding is the space above its first block, whatever top
   * margin that block carries.
   *
   * `data-slot` hooks let the desktop app restyle the panel and keep it
   * clickable over its title bar's drag region.
   */
  import { getMessage } from '$lib/i18n'
  import { cn } from '$lib/utils'
  import { X } from '@lucide/svelte'
  import { AlertDialog, Dialog } from 'bits-ui'
  import type { Snippet } from 'svelte'
  import { buttonClass, dialogContentClass, dialogDescriptionClass, dialogOverlayClass } from './ui'

  let {
    open = $bindable(false),
    onOpenChangeComplete,
    alert = false,
    title,
    description,
    contentClass,
    children,
    footer
  }: {
    /** Two-way, and it must be real `$state` in the caller: bits-ui writes it
     * back on every dismissal (the close button, Escape, the scrim). */
    open?: boolean
    /** Fires once the open or close animation has finished, however `open`
     * changed. A dialog its parent mounts only while it is shown unmounts
     * itself here, after it has faded out. */
    onOpenChangeComplete?: (open: boolean) => void
    /** A confirmation that needs an answer: an `alertdialog` a click outside
     * does not dismiss, with no × — the footer's buttons are the answers, and
     * Escape still cancels. */
    alert?: boolean
    title: string
    /** One line under the title, such as the query the results answer. */
    description?: string
    contentClass?: string
    children: Snippet
    footer?: Snippet
  } = $props()

  // The two primitives share their title, description, overlay and portal.
  const Root = $derived(alert ? AlertDialog.Root : Dialog.Root)
  const Content = $derived(alert ? AlertDialog.Content : Dialog.Content)
</script>

<Root bind:open {onOpenChangeComplete}>
  <Dialog.Portal>
    <Dialog.Overlay class={dialogOverlayClass()} data-slot="modal-overlay" />
    <Content
      class={dialogContentClass(
        cn(
          'flex max-h-[85vh] min-h-[min(20rem,calc(100vh-2rem))] flex-col gap-0 overflow-hidden p-0 sm:max-w-2xl',
          contentClass
        )
      )}
      data-slot="modal-content"
    >
      <div class="flex shrink-0 items-start justify-between gap-3 border-b bg-muted/35 px-5 py-4">
        <div class="grid min-w-0 gap-1">
          <Dialog.Title class="text-base font-semibold break-words">{title}</Dialog.Title>
          {#if description}<Dialog.Description
              class={dialogDescriptionClass('text-xs leading-relaxed break-words')}
              >{description}</Dialog.Description
            >{/if}
        </div>
        {#if !alert}<Dialog.Close
            class={buttonClass('ghost', 'icon-sm', '-me-2 -mt-1')}
            aria-label={getMessage('close')}><X class="size-4" /></Dialog.Close
          >{/if}
      </div>
      <div
        class="scrollbar-slim min-h-0 flex-1 overflow-x-clip overflow-y-auto px-5 py-4 [&>:first-child]:mt-0"
      >
        {@render children()}
      </div>
      {#if footer}
        <div
          class="flex shrink-0 flex-wrap items-center justify-end gap-2 border-t bg-muted/35 px-5 py-3"
        >
          {@render footer()}
        </div>
      {/if}
    </Content>
  </Dialog.Portal>
</Root>
