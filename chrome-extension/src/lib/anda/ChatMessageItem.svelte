<script lang="ts" module>
  const expandedDetailMessageIds = new Set<string>()
</script>

<script lang="ts">
  import type { ChatAttachment, ChatMessage } from '$lib/anda/client/types'
  import { andaClient } from '$lib/anda/client/side-panel.svelte'
  import { buttonClass, cardClass, cardContentClass } from '$lib/anda/ui'
  import { renderMarkdown } from '$lib/utils/markdown'
  import {
    Check,
    Clipboard,
    Copy,
    Download,
    FileText,
    Image,
    LoaderCircle,
    Printer,
    Wrench
  } from '@lucide/svelte'
  import { onDestroy, onMount, tick } from 'svelte'

  let { message }: { message: ChatMessage } = $props()

  let copied = $state(false)
  let richCopied = $state(false)
  let detailsExpanded = $state(false)
  let downloadingAttachmentIds = $state(new Set<string>())
  let resourceBlobs = $state(new Map<number, string>())
  let resourceObjectUrls = $state(new Map<string, string>())
  let loadingResourceIds = $state(new Set<number>())
  let failedResourceIds = $state(new Set<number>())
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
  const messageActionButtonClass = buttonClass(
    'outline',
    'icon-sm',
    'pointer-events-none scale-95 bg-background/95 text-muted-foreground shadow-md backdrop-blur-sm duration-150 group-hover/card:pointer-events-auto group-hover/card:scale-100 group-focus-within/card:pointer-events-auto group-focus-within/card:scale-100 hover:border-emerald-200 hover:text-emerald-700 focus-visible:pointer-events-auto focus-visible:scale-100'
  )

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

  async function copyRichMessage() {
    if (!navigator.clipboard || !mainText) {
      return
    }

    if (navigator.clipboard.write && typeof ClipboardItem !== 'undefined') {
      await navigator.clipboard.write([
        new ClipboardItem({
          'text/plain': new Blob([mainText], { type: 'text/plain' }),
          'text/html': new Blob([html], { type: 'text/html' })
        })
      ])
    } else {
      await navigator.clipboard.writeText(mainText)
    }

    richCopied = true
    window.setTimeout(() => {
      richCopied = false
    }, 1200)
  }

  function escapeHtml(value: string): string {
    return value.replace(/[&<>"]/g, (character) => {
      switch (character) {
        case '&':
          return '&amp;'
        case '<':
          return '&lt;'
        case '>':
          return '&gt;'
        default:
          return '&quot;'
      }
    })
  }

  function printableAttachmentHtml(): string {
    return (message.attachments || [])
      .map((attachment) => {
        const imageUrl = attachmentMimeType(attachment).startsWith('image/')
          ? ensureAttachmentObjectUrl(attachment) || attachmentDownloadUrl(attachment)
          : ''
        const description = attachmentDescription(attachment)
        return `
            <div class="attachment-header">
              <strong>${escapeHtml(attachment.name)}</strong>
              <span>${escapeHtml(attachmentMetaLabel(attachment))}</span>
            </div>
            ${imageUrl ? `<img src="${escapeHtml(imageUrl)}" alt="${escapeHtml(attachment.name)}" />` : ''}
            ${description ? `<pre>${escapeHtml(description)}</pre>` : ''}
        `
      })
      .join('')
  }

  function printMessage() {
    const printWindow = window.open('', '_blank')
    if (!printWindow) return

    const roleLabel = isUser ? 'User' : isSystem ? 'System' : isTool ? 'Tool' : 'Assistant'
    const attachmentsHtml = printableAttachmentHtml()
    const doc = printWindow.document
    doc.title = `${escapeHtml(roleLabel)} message`

    // 注入打印样式
    const style = doc.createElement('style')
    style.textContent = `
      body { font-family: sans-serif; padding: 40px; color: #1e293b; background: white; }
      .message-container { max-width: 800px; margin: 0 auto; border: 1px solid #e2e8f0; border-radius: 12px; padding: 24px; }
      .role { font-weight: bold; margin-bottom: 12px; color: #64748b; text-transform: uppercase; font-size: 12px; letter-spacing: 1px; }
      .content { line-height: 1.6; word-break: break-word; }
      @media print {
        body { padding: 0; }
        .message-container { border: none; padding: 0; }
      }
    `
    doc.head.appendChild(style)

    // 构建内容结构
    const container = doc.createElement('div')
    container.className = 'message-container'
    container.innerHTML = `
      <div class="role">${roleLabel}</div>
      <div class="content"></div>
      <div class="attachment"></div>
    `
    const contentPlaceholder = container.querySelector('.content')
    if (contentPlaceholder) contentPlaceholder.innerHTML = html
    const attachmentPlaceholder = container.querySelector('.attachment')
    if (attachmentPlaceholder) attachmentPlaceholder.innerHTML = attachmentsHtml

    doc.body.appendChild(container)

    // 打印并自动关闭
    // printWindow.addEventListener('afterprint', () => {
    //   printWindow.close()
    // })

    // 确保内容加载完成后触发打印
    printWindow.requestAnimationFrame(() => {
      printWindow.focus()
      printWindow.print()
    })
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

  function resourceId(attachment: ChatAttachment): number {
    return attachment.resource._id || 0
  }

  function attachmentCacheKey(attachment: ChatAttachment): string {
    const id = resourceId(attachment)
    return id ? `resource:${id}` : attachment.id
  }

  function attachmentResourceBlob(attachment: ChatAttachment): string {
    const inlineBlob = attachment.resource.blob?.trim()
    if (inlineBlob) {
      return inlineBlob
    }

    const id = resourceId(attachment)
    return id ? (resourceBlobs.get(id) || '').trim() : ''
  }

  function attachmentObjectUrl(attachment: ChatAttachment): string {
    return resourceObjectUrls.get(attachmentCacheKey(attachment)) || ''
  }

  function attachmentMetaLabel(attachment: ChatAttachment): string {
    return [attachmentMimeType(attachment), fileSizeLabel(attachment.size)]
      .filter(Boolean)
      .join(' / ')
  }

  function attachmentDescription(attachment: ChatAttachment): string {
    return (attachment.resource.description || '')
      .trim()
      .replace(/^\[\$system:[^\]]+\]\s*/i, '')
      .trim()
  }

  function attachmentDownloadUrl(attachment: ChatAttachment): string {
    const objectUrl = attachmentObjectUrl(attachment)
    if (objectUrl) {
      return objectUrl
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

  function attachmentHasDownloadData(attachment: ChatAttachment): boolean {
    return Boolean(
      attachmentDownloadUrl(attachment) ||
      attachmentResourceBlob(attachment) ||
      resourceId(attachment)
    )
  }

  function attachmentSaveTitle(attachment: ChatAttachment): string {
    return attachmentHasDownloadData(attachment)
      ? `Save ${attachment.name}`
      : 'No downloadable data'
  }

  function normalizeBase64(value: string): string {
    const payload = value.trim().replace(/^data:[^,]*,/i, '')
    const normalized = payload.replace(/\s/g, '').replace(/-/g, '+').replace(/_/g, '/')
    const remainder = normalized.length % 4
    return remainder ? normalized + '='.repeat(4 - remainder) : normalized
  }

  function base64ToBytes(value: string): Uint8Array {
    const binary = atob(normalizeBase64(value))
    const bytes = new Uint8Array(binary.length)
    for (let index = 0; index < binary.length; index += 1) {
      bytes[index] = binary.charCodeAt(index)
    }
    return bytes
  }

  function bytesToArrayBuffer(bytes: Uint8Array): ArrayBuffer {
    const buffer = new ArrayBuffer(bytes.byteLength)
    new Uint8Array(buffer).set(bytes)
    return buffer
  }

  function ensureAttachmentObjectUrl(
    attachment: ChatAttachment,
    blob = attachmentResourceBlob(attachment)
  ): string {
    const existingUrl = attachmentObjectUrl(attachment)
    if (existingUrl || !blob) {
      return existingUrl
    }

    try {
      const url = URL.createObjectURL(
        new Blob([bytesToArrayBuffer(base64ToBytes(blob))], {
          type: attachmentMimeType(attachment) || 'application/octet-stream'
        })
      )
      resourceObjectUrls = new Map([...resourceObjectUrls, [attachmentCacheKey(attachment), url]])
      return url
    } catch (error) {
      console.warn('Failed to create attachment object URL', resourceId(attachment), error)
      return ''
    }
  }

  function downloadWithAnchor(url: string, filename: string) {
    const anchor = document.createElement('a')
    anchor.href = url
    anchor.download = filename
    anchor.rel = 'noopener'
    document.body.appendChild(anchor)
    anchor.click()
    anchor.remove()
  }

  async function loadAttachmentResource(
    attachment: ChatAttachment,
    options: { retry?: boolean } = {}
  ): Promise<string> {
    const id = resourceId(attachment)
    if (!id) {
      return attachmentResourceBlob(attachment)
    }

    const existingBlob = attachmentResourceBlob(attachment)
    if (existingBlob) {
      return existingBlob
    }
    if (loadingResourceIds.has(id) || (!options.retry && failedResourceIds.has(id))) {
      return ''
    }

    loadingResourceIds = new Set([...loadingResourceIds, id])
    const nextFailed = new Set(failedResourceIds)
    nextFailed.delete(id)
    failedResourceIds = nextFailed

    try {
      const resource = await andaClient.loadResource(attachment.resource)
      const blob = resource?.blob?.trim() || ''
      if (blob) {
        resourceBlobs = new Map([...resourceBlobs, [id, blob]])
        ensureAttachmentObjectUrl(attachment, blob)
        return blob
      }

      failedResourceIds = new Set([...failedResourceIds, id])
      return ''
    } catch (error) {
      failedResourceIds = new Set([...failedResourceIds, id])
      console.warn('Failed to load attachment resource', id, error)
      return ''
    } finally {
      const nextLoading = new Set(loadingResourceIds)
      nextLoading.delete(id)
      loadingResourceIds = nextLoading
    }
  }

  function loadImageAttachmentResources() {
    for (const attachment of message.attachments || []) {
      if (
        attachmentMimeType(attachment).startsWith('image/') &&
        attachmentResourceBlob(attachment) &&
        !attachmentObjectUrl(attachment)
      ) {
        ensureAttachmentObjectUrl(attachment)
      }

      if (
        attachmentMimeType(attachment).startsWith('image/') &&
        resourceId(attachment) &&
        !attachmentResourceBlob(attachment) &&
        !loadingResourceIds.has(resourceId(attachment)) &&
        !failedResourceIds.has(resourceId(attachment))
      ) {
        loadAttachmentResource(attachment).catch(() => undefined)
      }
    }
  }

  async function saveAttachment(attachment: ChatAttachment) {
    let url = attachmentDownloadUrl(attachment)
    if (!url && resourceId(attachment)) {
      await loadAttachmentResource(attachment, { retry: true })
      url = attachmentDownloadUrl(attachment)
    }
    if (!url && attachmentResourceBlob(attachment)) {
      ensureAttachmentObjectUrl(attachment)
      url = attachmentDownloadUrl(attachment)
    }

    if (!url) {
      return
    }

    downloadingAttachmentIds = new Set([...downloadingAttachmentIds, attachment.id])
    try {
      const filename = safeDownloadName(attachment.name)
      if (url.startsWith('blob:') || !chrome.downloads?.download) {
        downloadWithAnchor(url, filename)
        return
      }

      await chrome.downloads.download({
        url,
        filename,
        saveAs: true
      })
    } finally {
      const next = new Set(downloadingAttachmentIds)
      next.delete(attachment.id)
      downloadingAttachmentIds = next
    }
  }

  $effect(() => {
    loadImageAttachmentResources()
  })

  onMount(() => {
    if (expandedDetailMessageIds.has(message.id)) {
      detailsExpanded = true
    }
    hook()
    thinkingHook()
  })

  onDestroy(() => {
    for (const url of resourceObjectUrls.values()) {
      URL.revokeObjectURL(url)
    }
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
          : '-right-3'} z-10 flex items-center gap-1 opacity-0 transition duration-150 group-hover/card:pointer-events-auto group-hover/card:opacity-100 group-focus-within/card:pointer-events-auto group-focus-within/card:opacity-100"
      >
        <button
          type="button"
          class={messageActionButtonClass}
          aria-label="Copy message"
          title="Copy message"
          onclick={copyMessage}
        >
          {#if copied}
            <Check class="size-4" />
          {:else}
            <Copy class="size-4" />
          {/if}
        </button>
        <button
          type="button"
          class={messageActionButtonClass}
          aria-label="Copy rich text"
          title="Copy rich text"
          onclick={copyRichMessage}
        >
          {#if richCopied}
            <Check class="size-4" />
          {:else}
            <Clipboard class="size-4" />
          {/if}
        </button>
        <button
          type="button"
          class={messageActionButtonClass}
          aria-label="Print message"
          title="Print message"
          onclick={printMessage}
        >
          <Printer class="size-4" />
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
                    {#if attachmentMimeType(attachment).startsWith('image/') && attachmentObjectUrl(attachment)}
                      <img
                        src={attachmentObjectUrl(attachment)}
                        alt={attachment.name}
                        class="size-full object-cover"
                      />
                    {:else if attachmentMimeType(attachment).startsWith('image/') && loadingResourceIds.has(resourceId(attachment))}
                      <LoaderCircle class="size-4 animate-spin" />
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
                    disabled={!attachmentHasDownloadData(attachment) ||
                      downloadingAttachmentIds.has(attachment.id) ||
                      loadingResourceIds.has(resourceId(attachment))}
                    aria-label={`Save ${attachment.name}`}
                    title={attachmentSaveTitle(attachment)}
                    onclick={() => saveAttachment(attachment)}
                  >
                    {#if downloadingAttachmentIds.has(attachment.id) || loadingResourceIds.has(resourceId(attachment))}
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
