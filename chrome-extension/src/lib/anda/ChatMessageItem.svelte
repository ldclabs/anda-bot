<script lang="ts" module>
  const expandedDetailMessageIds = new Set<string>()
</script>

<script lang="ts">
  import type { ChatAttachment, ChatMessage } from '$lib/anda/client/types'
  import { buttonClass, cardClass, cardContentClass } from '$lib/anda/ui'
  import { renderMarkdown } from '$lib/utils/markdown'
  import { Check, Clipboard, Download, FileText, Image, LoaderCircle, Wrench } from '@lucide/svelte'
  import { onMount, tick } from 'svelte'

  let { message }: { message: ChatMessage } = $props()

  let copied = $state(false)
  let detailsExpanded = $state(false)
  let downloadingAttachmentIds = $state(new Set<string>())
  const isUser = $derived(message.role === 'user')
  const isSystem = $derived(message.role === 'system')
  const isTool = $derived(message.role === 'tool')
  const mainText = $derived(message.text.trim())
  const thinkingText = $derived((message.thinkingText || '').trim())
  const hasMainText = $derived(Boolean(mainText))
  const hasAttachments = $derived(Boolean(message.attachments?.length))
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

  function attachmentMimeType(attachment: ChatAttachment): string {
    return attachment.type || attachment.resource.mime_type || ''
  }

  function attachmentMetaLabel(attachment: ChatAttachment): string {
    return [attachmentMimeType(attachment), fileSizeLabel(attachment.size)].filter(Boolean).join(' / ')
  }

  function attachmentDescription(attachment: ChatAttachment): string {
    return (attachment.resource.description || '')
      .trim()
      .replace(/^\[\$system:[^\]]+\]\s*/i, '')
      .trim()
  }

  function attachmentDownloadUrl(attachment: ChatAttachment): string {
    const blob = attachment.resource.blob?.trim()
    if (blob) {
      return `data:${attachmentMimeType(attachment) || 'application/octet-stream'};base64,${blob}`
    }

    const uri = attachment.resource.uri?.trim()
    if (/^(https?:|file:|data:|blob:)/i.test(uri || '')) {
      return uri || ''
    }
    return ''
  }

  function safeDownloadName(name: string): string {
    return name.replace(/[\\/:*?"<>|]+/g, '-').trim() || 'attachment'
  }

  async function saveAttachment(attachment: ChatAttachment) {
    const url = attachmentDownloadUrl(attachment)
    if (!url || !chrome.downloads?.download) {
      return
    }

    downloadingAttachmentIds = new Set([...downloadingAttachmentIds, attachment.id])
    try {
      await chrome.downloads.download({
        url,
        filename: safeDownloadName(attachment.name),
        saveAs: true
      })
    } finally {
      const next = new Set(downloadingAttachmentIds)
      next.delete(attachment.id)
      downloadingAttachmentIds = next
    }
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
    : isTool || (!hasMainText && !hasAttachments)
      ? 'justify-items-center'
      : 'justify-items-start'}"
>
  {#if hasThinkingText && (isTool || (!hasMainText && !hasAttachments))}
    <button
      type="button"
      class={buttonClass(
        'outline',
        'xs',
        'rounded-full bg-background/70 text-muted-foreground shadow-sm hover:border-emerald-200 hover:text-emerald-700'
      )}
      onclick={toggleDetails}
    >
      <Wrench class="size-3" />
      <span>{detailsExpanded ? `Hide ${detailLabel}` : `Show ${detailLabel}`}</span>
    </button>
  {/if}

  {#if hasMainText || hasAttachments}
    <div
      class={cardClass(
        `relative max-w-[92%] min-w-0 gap-0 overflow-visible rounded-lg py-0 leading-relaxed shadow-2xs ${
          isUser
            ? ' rounded-br-none bg-sky-50 text-slate-950'
            : isSystem
              ? 'rounded-bl-none border-amber-200 bg-amber-50 text-amber-950'
              : isTool
                ? 'border-stone-200 bg-stone-50 text-stone-800'
                : 'rounded-bl-none border-stone-100 bg-white text-stone-950'
        }`
      )}
    >
      <div
        class="pointer-events-none absolute -top-3 {isUser
          ? '-left-3'
          : '-right-3'} z-10 opacity-0 transition duration-150 group-hover/card:pointer-events-auto group-hover/card:opacity-100 group-focus-within/card:pointer-events-auto group-focus-within/card:opacity-100"
      >
        <button
          type="button"
          class={buttonClass(
            'outline',
            'icon-sm',
            'pointer-events-none scale-95 bg-background/95 text-muted-foreground shadow-md backdrop-blur-sm duration-150 group-hover/card:pointer-events-auto group-hover/card:scale-100 group-focus-within/card:pointer-events-auto group-focus-within/card:scale-100 hover:border-emerald-200 hover:text-emerald-700 focus-visible:pointer-events-auto focus-visible:scale-100'
          )}
          aria-label="Copy message"
          title="Copy message"
          onclick={copyMessage}
        >
          {#if copied}
            <Check class="size-4" />
          {:else}
            <Clipboard class="size-4" />
          {/if}
        </button>
      </div>

      <div class={cardContentClass('px-3 py-2')}>
        {#if hasMainText}
          <div class="md-content w-full min-w-0 text-pretty wrap-break-word">{@html html}</div>
        {/if}

        {#if message.attachments?.length}
          <div class="{hasMainText ? 'mt-2' : ''} grid gap-1.5">
            {#each message.attachments as attachment (attachment.id)}
              <div
                class="max-w-full rounded-md border border-border/70 bg-background/65 p-1.5 text-[11px] text-muted-foreground"
              >
                <div class="flex min-w-0 items-center gap-2">
                  <div
                    class="grid size-9 shrink-0 place-items-center overflow-hidden rounded-sm border border-border/60 bg-muted/50 text-emerald-700"
                  >
                    {#if attachmentMimeType(attachment).startsWith('image/') && attachment.resource.blob}
                      <img
                        src={`data:${attachmentMimeType(attachment)};base64,${attachment.resource.blob}`}
                        alt={attachment.name}
                        class="size-full object-cover"
                      />
                    {:else if attachmentMimeType(attachment).startsWith('image/')}
                      <Image class="size-4" />
                    {:else}
                      <FileText class="size-4" />
                    {/if}
                  </div>

                  <div class="min-w-0 flex-1">
                    <div class="truncate font-medium text-foreground" title={attachment.name}>
                      {attachment.name}
                    </div>
                    {#if attachmentMetaLabel(attachment)}
                      <div class="truncate text-[10px] text-muted-foreground/75">
                        {attachmentMetaLabel(attachment)}
                      </div>
                    {/if}
                  </div>

                  <button
                    type="button"
                    class={buttonClass(
                      'ghost',
                      'icon-xs',
                      'size-6 rounded-sm text-muted-foreground hover:text-emerald-700'
                    )}
                    disabled={!attachmentDownloadUrl(attachment) ||
                      downloadingAttachmentIds.has(attachment.id)}
                    aria-label={`Save ${attachment.name}`}
                    title={attachmentDownloadUrl(attachment)
                      ? `Save ${attachment.name}`
                      : 'No downloadable data'}
                    onclick={() => saveAttachment(attachment)}
                  >
                    {#if downloadingAttachmentIds.has(attachment.id)}
                      <LoaderCircle class="size-3 animate-spin" />
                    {:else}
                      <Download class="size-3" />
                    {/if}
                  </button>
                </div>

                {#if attachmentDescription(attachment)}
                  <div
                    class="mt-1.5 max-h-44 overflow-y-auto rounded-sm border border-border/50 bg-muted/35 px-2 py-1.5 whitespace-pre-wrap text-[11px] leading-relaxed text-foreground/80"
                  >
                    {attachmentDescription(attachment)}
                  </div>
                {/if}
              </div>
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
              <button
                type="button"
                class={buttonClass(
                  'ghost',
                  'xs',
                  'h-auto min-w-0 px-1.5 py-0.5 text-[11px] font-semibold text-muted-foreground/75 hover:text-muted-foreground'
                )}
                onclick={toggleDetails}
              >
                <Wrench class="size-3 shrink-0" />
                <span class="truncate">
                  {detailsExpanded ? 'Hide thinking and tools' : 'Show thinking and tools'}
                </span>
              </button>
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
      </div>
    </div>
  {/if}

  {#if hasThinkingText && !hasMainText && detailsExpanded}
    <div
      class={cardClass(
        'relative max-w-[92%] min-w-0 gap-0 rounded-lg border-dashed bg-muted/50 py-0 text-[12px] leading-relaxed text-muted-foreground shadow-2xs'
      )}
    >
      <div class={cardContentClass('px-3 py-2')}>
        <div class="md-content w-full min-w-0 text-pretty wrap-break-word opacity-80">
          {@html thinkingHtml}
        </div>
        {#if messageTimeLabel}
          <div class="mt-1 text-right text-[10px] text-muted-foreground/70">
            {messageTimeLabel}
          </div>
        {/if}
      </div>
    </div>
  {/if}
</article>
