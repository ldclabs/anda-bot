<script lang="ts" module>
  const expandedDetailMessageIds = new Set<string>()
</script>

<script lang="ts">
  import type { ChatMessage } from '$lib/anda/client/types'
  import { Badge } from '$lib/components/ui/badge/index.js'
  import { Button } from '$lib/components/ui/button/index.js'
  import { Card, CardContent } from '$lib/components/ui/card/index.js'
  import { renderMarkdown } from '$lib/utils/markdown'
  import { Check, Clipboard, Wrench } from '@lucide/svelte'
  import { onMount, tick } from 'svelte'

  let { message }: { message: ChatMessage } = $props()

  let copied = $state(false)
  let detailsExpanded = $state(false)
  const isUser = $derived(message.role === 'user')
  const isSystem = $derived(message.role === 'system')
  const isTool = $derived(message.role === 'tool')
  const mainText = $derived(message.text.trim())
  const thinkingText = $derived((message.thinkingText || '').trim())
  const hasMainText = $derived(Boolean(mainText))
  const hasThinkingText = $derived(Boolean(thinkingText))
  const messageTimeLabel = $derived(timeLabel(message.timestamp))
  const detailLabel = $derived(isTool ? 'tool output' : 'thinking and tools')
  const [html, hook] = $derived.by(() => renderMarkdown(mainText))
  const [thinkingHtml, thinkingHook] = $derived.by(() => renderMarkdown(thinkingText))

  async function copyMessage() {
    if (!navigator.clipboard || !mainText) {
      return
    }
    await navigator.clipboard.writeText(mainText)
    copied = true
    window.setTimeout(() => {
      copied = false
    }, 1200)
  }

  async function toggleDetails() {
    setDetailsExpanded(!detailsExpanded)
    if (detailsExpanded) {
      await tick()
      thinkingHook()
    }
  }

  function setDetailsExpanded(expanded: boolean) {
    detailsExpanded = expanded
    if (expanded) {
      expandedDetailMessageIds.add(message.id)
      return
    }
    expandedDetailMessageIds.delete(message.id)
  }

  function timeLabel(value: string | number | null | undefined): string {
    if (!value) {
      return ''
    }
    const date = new Date(value)
    if (Number.isNaN(date.getTime())) {
      return ''
    }
    return date.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })
  }

  function fileSizeLabel(size: number | undefined): string {
    if (!size) {
      return ''
    }
    if (size < 1024) {
      return `${size} B`
    }
    if (size < 1024 * 1024) {
      return `${(size / 1024).toFixed(1)} KB`
    }
    return `${(size / 1024 / 1024).toFixed(1)} MB`
  }

  onMount(() => {
    if (expandedDetailMessageIds.has(message.id)) {
      detailsExpanded = true
    }
    hook()
    thinkingHook()
  })
</script>

<article
  id={message.id}
  class="grid w-full gap-1 {isUser
    ? 'justify-items-end'
    : isTool || !hasMainText
      ? 'justify-items-center'
      : 'justify-items-start'}"
>
  {#if hasThinkingText && (isTool || !hasMainText)}
    <Button
      variant="outline"
      size="xs"
      class="rounded-full bg-background/70 text-muted-foreground shadow-sm hover:border-emerald-200 hover:text-emerald-700"
      onclick={toggleDetails}
    >
      <Wrench class="size-3" />
      <span>{detailsExpanded ? `Hide ${detailLabel}` : `Show ${detailLabel}`}</span>
    </Button>
  {/if}

  {#if hasMainText}
    <Card
      class="relative max-w-[92%] min-w-0 gap-0 overflow-visible rounded-lg py-0 leading-relaxed shadow-2xs {isUser
        ? ' rounded-br-none bg-sky-50 text-slate-950'
        : isSystem
          ? 'rounded-bl-none border-amber-200 bg-amber-50 text-amber-950'
          : isTool
            ? 'border-stone-200 bg-stone-50 text-stone-800'
            : 'rounded-bl-none border-stone-100 bg-white text-stone-950'}"
    >
      <div
        class="pointer-events-none absolute -top-4 {isUser
          ? '-left-4'
          : '-right-4'} z-10 opacity-0 transition duration-150 group-hover/card:pointer-events-auto group-hover/card:opacity-100 group-focus-within/card:pointer-events-auto group-focus-within/card:opacity-100"
      >
        <Button
          variant="outline"
          size="icon-sm"
          class="pointer-events-none scale-95 bg-background/95 text-muted-foreground shadow-md backdrop-blur-sm duration-150 group-hover/card:pointer-events-auto group-hover/card:scale-100 group-focus-within/card:pointer-events-auto group-focus-within/card:scale-100 hover:border-emerald-200 hover:text-emerald-700 focus-visible:pointer-events-auto focus-visible:scale-100"
          aria-label="Copy message"
          title="Copy message"
          onclick={copyMessage}
        >
          {#if copied}
            <Check class="size-4" />
          {:else}
            <Clipboard class="size-4" />
          {/if}
        </Button>
      </div>

      <CardContent class="px-3 py-2">
        <div class="md-content w-full min-w-0 text-pretty wrap-break-word">{@html html}</div>

        {#if message.attachments?.length}
          <div class="mt-2 flex flex-wrap gap-1.5">
            {#each message.attachments as attachment (attachment.id)}
              <Badge
                variant="outline"
                class="max-w-full rounded-md bg-background/70 py-1 text-[11px] font-normal text-muted-foreground"
                title={attachment.name}
              >
                <span class="truncate">{attachment.name}</span>
                {#if fileSizeLabel(attachment.size)}
                  <span class="shrink-0 text-muted-foreground/70">
                    {fileSizeLabel(attachment.size)}
                  </span>
                {/if}
              </Badge>
            {/each}
          </div>
        {/if}

        {#if hasThinkingText || messageTimeLabel}
          <div
            class="mt-1.5 flex min-h-5 items-center gap-2 {hasThinkingText
              ? 'border-t border-border/70 pt-1.5'
              : ''}"
          >
            {#if hasThinkingText}
              <Button
                variant="ghost"
                size="xs"
                class="h-auto min-w-0 px-1.5 py-0.5 text-[11px] font-semibold text-muted-foreground/75 hover:text-muted-foreground"
                onclick={toggleDetails}
              >
                <Wrench class="size-3 shrink-0" />
                <span class="truncate">
                  {detailsExpanded ? 'Hide thinking and tools' : 'Show thinking and tools'}
                </span>
              </Button>
            {/if}
            {#if messageTimeLabel}
              <div class="ml-auto shrink-0 text-[10px] leading-none text-muted-foreground/70">
                {messageTimeLabel}
              </div>
            {/if}
          </div>
        {/if}

        {#if hasThinkingText && detailsExpanded}
          <div
            class="md-content mt-1 w-full min-w-0 text-[12px] leading-relaxed text-pretty wrap-break-word text-muted-foreground opacity-80"
          >
            {@html thinkingHtml}
          </div>
        {/if}
      </CardContent>
    </Card>
  {/if}

  {#if hasThinkingText && !hasMainText && detailsExpanded}
    <Card
      class="relative max-w-[92%] min-w-0 gap-0 rounded-lg border-dashed bg-muted/50 py-0 text-[12px] leading-relaxed text-muted-foreground shadow-2xs"
    >
      <CardContent class="px-3 py-2">
        <div class="md-content w-full min-w-0 text-pretty wrap-break-word opacity-80">
          {@html thinkingHtml}
        </div>
        {#if messageTimeLabel}
          <div class="mt-1 text-right text-[10px] text-muted-foreground/70">
            {messageTimeLabel}
          </div>
        {/if}
      </CardContent>
    </Card>
  {/if}
</article>
