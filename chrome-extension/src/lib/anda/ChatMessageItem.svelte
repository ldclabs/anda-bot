<script lang="ts">
  import type { Activity } from './memory/api'
  import ChatDetailRow from './ChatDetailRow.svelte'
  import DropdownMenu from './DropdownMenu.svelte'
  import { memoryActivityLabel } from './memory/labels'
  import { getClientPlatform } from '$lib/anda/client/platform'
  import { useAndaClient } from '$lib/anda/client/context'
  const andaClient = useAndaClient()
  import type { ChatAttachment, ChatMessage } from '$lib/anda/client/types'
  import {
    actionChoiceText,
    actionDetailIsBlock,
    actionDetailUrl,
    actionDetailLabel,
    actionDetailText,
    actionKindLabel,
    actionMessage,
    actionPending,
    actionSelectedChoice,
    actionStatusLabel,
    actionTitle,
    isApprovalAction,
    isPaymentApproval,
    isShellApproval
  } from '$lib/anda/chat/action-view'
  import {
    attachmentCacheKey,
    attachmentDescription,
    attachmentDownloadUrl,
    attachmentHasDownloadData,
    attachmentMetaLabel,
    attachmentMimeType,
    attachmentObjectUrl,
    attachmentResourceBlob,
    attachmentResourceId,
    safeDownloadName,
    type AttachmentCaches
  } from '$lib/anda/chat/attachment-view'
  import { copyCodeFromClick, enhanceCodeBlocks } from '$lib/anda/chat/code-blocks'
  import { transcriptEntrances } from '$lib/anda/chat/entrance'
  import { isProcessStep } from '$lib/anda/chat/message-display'
  import {
    firstLine,
    runtimeNotices,
    toolCallLabel,
    toolCallStatus,
    toolCallSummary,
    toolDetailSections,
    toolKind,
    type ToolDetailSection,
    type ToolKind
  } from '$lib/anda/chat/tool-view'
  import { base64ToBytes } from '$lib/utils/base64'
  import { escapeHtml, formatFileSize, formatTimestamp } from '$lib/utils/format'
  import { getMessage } from '$lib/i18n'
  import { buttonClass, cardClass, cardContentClass } from '$lib/anda/ui'
  import { renderMarkdown } from '$lib/utils/markdown'
  import {
    Bell,
    Bookmark,
    BookmarkCheck,
    Bot,
    BrainCircuit,
    Check,
    CircleCheck,
    Clipboard,
    Copy,
    CreditCard,
    Download,
    FileText,
    Globe,
    Image,
    Layers,
    Lightbulb,
    ListChecks,
    LoaderCircle,
    MoreHorizontal,
    Plus,
    Printer,
    ShieldCheck,
    Terminal,
    Wrench
  } from '@lucide/svelte'
  import { onDestroy, onMount, untrack, type Component } from 'svelte'

  let {
    message,
    memoryActivity,
    quickPromptActive = false,
    onToggleQuickPrompt,
    compactActions = false,
    groupTools = false,
    live = false
  }: {
    message: ChatMessage
    memoryActivity?: Activity
    quickPromptActive?: boolean
    onToggleQuickPrompt?: (text: string) => Promise<void> | void
    /** Keeps copy and bookmark in the footer and moves rich-text copy and
     * printing into a "More" menu (the desktop transcript, which also shows
     * the footer only on hover). */
    compactActions?: boolean
    /** Folds two or more tool calls into one expandable summary row. */
    groupTools?: boolean
    /** Part of the turn that is still running: unfinished rows animate. */
    live?: boolean
  } = $props()

  // A message that just arrived enters (once, see chat/entrance.ts); the class
  // comes off once the entrance has played so later re-renders stay still.
  const entranceDelay = untrack(() => transcriptEntrances.claim(message))
  let entering = $state(entranceDelay !== null)
  onMount(() => {
    if (!entering) return
    const timer = window.setTimeout(() => (entering = false), 1800 + (entranceDelay || 0))
    return () => clearTimeout(timer)
  })
  let contentElement: HTMLElement | null = $state(null)
  let bookmarkPopped = $state(false)
  let quickPromptPopped = $state(false)

  let copied = $state(false)
  let articleElement: HTMLElement
  let visible = $state(false)
  let disposed = false
  let richCopied = $state(false)
  let downloadingAttachmentIds = $state(new Set<string>())
  let resourceBlobs = $state(new Map<number, string>())
  let resourceObjectUrls = $state(new Map<string, string>())
  // What the attachment presenters need to resolve bytes for this message.
  const caches = $derived<AttachmentCaches>({
    resourceBlobs,
    objectUrls: resourceObjectUrls
  })
  let loadingResourceIds = $state(new Set<number>())
  let failedResourceIds = $state(new Set<number>())
  const isUser = $derived(message.role === 'user')
  const isExternalUser = $derived(message.role === 'external_user')
  const isSystem = $derived(message.role === 'system')
  const isTool = $derived(message.role === 'tool')
  const isAssistant = $derived(!isUser && !isExternalUser && !isSystem && !isTool)
  const mainText = $derived(message.text.trim())
  const thinkingText = $derived((message.thinkingText || '').trim())
  const hasMainText = $derived(Boolean(mainText))
  const hasAttachments = $derived(Boolean(message.attachments?.length))
  const hasImageAttachments = $derived(
    Boolean(
      message.attachments?.some((attachment) => attachmentMimeType(attachment).startsWith('image/'))
    )
  )
  const hasActions = $derived(Boolean(message.actions?.length))
  const hasThinkingText = $derived(Boolean(thinkingText))
  const tools = $derived(message.tools || [])
  // Runtime-injected notices arrive as tool messages that carry text, not calls.
  const notices = $derived(isTool && !tools.length ? runtimeNotices(thinkingText) : [])
  const groupedTools = $derived(groupTools && tools.length > 1)
  const toolStatuses = $derived(groupedTools ? tools.map(toolCallStatus) : [])
  const toolGroupStatus = $derived(
    toolStatuses.includes('error')
      ? ('error' as const)
      : toolStatuses.includes('running')
        ? ('running' as const)
        : ('ok' as const)
  )
  // While a call runs the summary names it; afterwards, the distinct tools used.
  const toolGroupSummary = $derived.by(() => {
    if (!groupedTools) return ''
    const running = tools.findLastIndex((_, index) => toolStatuses[index] === 'running')
    if (running >= 0) {
      return toolCallLabel(tools[running]!)
    }
    const names = [...new Set(tools.map((tool) => tool.name))]
    return names.length > 4 ? `${names.slice(0, 4).join(', ')}, …` : names.join(', ')
  })
  const hasCard = $derived(hasMainText || hasAttachments || hasActions)
  // Narration that led to tool calls: the turn's final answer carries the footer.
  const processStep = $derived(isProcessStep(message))
  // Only settled assistant messages with a stable server id can be bookmarked
  // (excludes optimistic/local, side, and runtime-notice items).
  const canBookmark = $derived(
    isAssistant && hasMainText && !message.pending && /^m-\d+-\d+$/.test(message.id)
  )
  const bookmarked = $derived(canBookmark && andaClient.bookmarks.isBookmarked(message.id))
  const canToggleQuickPrompt = $derived(isUser && hasMainText && Boolean(onToggleQuickPrompt))
  const messageTimeLabel = $derived(formatTimestamp(message.timestamp))
  const externalUserSenderLabel = $derived(
    message.externalUser?.sender || message.externalUser?.scope || getMessage('roleExternalUser')
  )
  const externalUserContextLabel = $derived(
    [message.externalUser?.channel, message.externalUser?.space].filter(Boolean).join(' / ')
  )
  const html = $derived(renderMarkdown(mainText))
  // Reasoning still streaming in: nothing else has arrived for this step yet.
  const thinkingLive = $derived(live && !hasMainText && !tools.length)
  function rowDelay(index: number): number {
    return entering ? (entranceDelay || 0) + 60 + Math.min(index, 8) * 45 : 0
  }

  $effect(() => {
    void html
    const element = contentElement
    if (!element) return
    enhanceCodeBlocks(element, getMessage('copyCode'))
    if (untrack(() => entering)) {
      for (const [index, child] of Array.from(element.children).entries()) {
        ;(child as HTMLElement).style.setProperty('--i', String(index))
      }
    }
  })
  const messageActionButtonClass = buttonClass(
    'ghost',
    'icon-xs',
    'chat-message-action relative size-5 rounded-sm'
  )
  type MoreAction = 'copy-rich' | 'print'
  const moreActions = $derived([
    ...(mainText ? [{ value: 'copy-rich' as const, label: getMessage('copyRichText') }] : []),
    ...(hasMainText || hasAttachments
      ? [{ value: 'print' as const, label: getMessage('printMessage') }]
      : [])
  ] satisfies { value: MoreAction; label: string }[])
  function runMoreAction(action: MoreAction) {
    if (action === 'copy-rich') void copyRichMessage()
    else printMessage()
  }

  async function copyMessage() {
    if (!navigator.clipboard || !mainText) {
      return
    }
    await navigator.clipboard.writeText(mainText)
    copied = true
    window.setTimeout(() => {
      copied = false
    }, 1400)
  }

  function toggleBookmark() {
    bookmarkPopped = !bookmarked
    void andaClient.bookmarks.toggle(message)
  }

  function toggleQuickPrompt() {
    quickPromptPopped = !quickPromptActive
    void onToggleQuickPrompt?.(mainText)
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

  function printableAttachmentHtml(): string {
    return (message.attachments || [])
      .map((attachment) => {
        const imageUrl = attachmentMimeType(attachment).startsWith('image/')
          ? ensureAttachmentObjectUrl(attachment) || attachmentDownloadUrl(attachment, caches)
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
    const native = getClientPlatform()
    const printWindow = native?.printHtml ? null : window.open('', '_blank')
    if (!printWindow && !native?.printHtml) return

    const roleLabel = getMessage(
      isUser
        ? 'roleUser'
        : isExternalUser
          ? 'roleExternalUser'
          : isSystem
            ? 'roleSystem'
            : isTool
              ? 'roleTool'
              : 'roleAssistant'
    )
    const attachmentsHtml = printableAttachmentHtml()
    const doc = printWindow?.document || document.implementation.createHTMLDocument('Print')
    doc.title = roleLabel

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
      <div class="role">${escapeHtml(roleLabel)}</div>
      <div class="content"></div>
      <div class="attachment"></div>
    `
    const contentPlaceholder = container.querySelector('.content')
    if (contentPlaceholder) contentPlaceholder.innerHTML = html
    const attachmentPlaceholder = container.querySelector('.attachment')
    if (attachmentPlaceholder) attachmentPlaceholder.innerHTML = attachmentsHtml

    doc.body.appendChild(container)

    if (native?.printHtml) {
      void native.printHtml('<!doctype html>' + doc.documentElement.outerHTML)
      return
    }

    // 打印并自动关闭
    // printWindow.addEventListener('afterprint', () => {
    //   printWindow.close()
    // })

    // 确保内容加载完成后触发打印
    printWindow?.requestAnimationFrame(() => {
      printWindow.focus()
      printWindow.print()
    })
  }

  const toolIcons: Record<ToolKind, Component<{ class?: string }>> = {
    shell: Terminal,
    file: FileText,
    memory: BrainCircuit,
    web: Globe,
    agent: Bot,
    tool: Wrench
  }

  function toolSectionLabel(section: ToolDetailSection): string {
    const label =
      section.kind === 'input'
        ? getMessage('toolInput')
        : section.kind === 'output'
          ? getMessage('toolOutput')
          : 'stderr'
    return section.meta ? `${label} · ${section.meta}` : label
  }

  function attachmentSaveTitle(attachment: ChatAttachment): string {
    return attachmentHasDownloadData(attachment, caches)
      ? getMessage('saveAttachment', attachment.name)
      : getMessage('noDownloadData')
  }

  function ensureAttachmentObjectUrl(
    attachment: ChatAttachment,
    blob = attachmentResourceBlob(attachment, caches)
  ): string {
    if (disposed) return ''
    const existingUrl = attachmentObjectUrl(attachment, caches)
    if (existingUrl || !blob) {
      return existingUrl
    }

    try {
      const url = URL.createObjectURL(
        new Blob([base64ToBytes(blob)], {
          type: attachmentMimeType(attachment) || 'application/octet-stream'
        })
      )
      resourceObjectUrls = new Map([...resourceObjectUrls, [attachmentCacheKey(attachment), url]])
      return url
    } catch (error) {
      console.warn(
        'Failed to create attachment object URL',
        attachmentResourceId(attachment),
        error
      )
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
    const id = attachmentResourceId(attachment)
    if (!id) {
      return attachmentResourceBlob(attachment, caches)
    }

    const existingBlob = attachmentResourceBlob(attachment, caches)
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
      if (disposed) return ''
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
        attachmentResourceBlob(attachment, caches) &&
        !attachmentObjectUrl(attachment, caches)
      ) {
        ensureAttachmentObjectUrl(attachment)
      }

      if (
        attachmentMimeType(attachment).startsWith('image/') &&
        attachmentResourceId(attachment) &&
        !attachmentResourceBlob(attachment, caches) &&
        !loadingResourceIds.has(attachmentResourceId(attachment)) &&
        !failedResourceIds.has(attachmentResourceId(attachment))
      ) {
        loadAttachmentResource(attachment).catch(() => undefined)
      }
    }
  }

  async function saveAttachment(attachment: ChatAttachment) {
    let url = attachmentDownloadUrl(attachment, caches)
    if (!url && attachmentResourceId(attachment)) {
      await loadAttachmentResource(attachment, { retry: true })
      url = attachmentDownloadUrl(attachment, caches)
    }
    if (!url && attachmentResourceBlob(attachment, caches)) {
      ensureAttachmentObjectUrl(attachment)
      url = attachmentDownloadUrl(attachment, caches)
    }

    if (!url) {
      return
    }

    downloadingAttachmentIds = new Set([...downloadingAttachmentIds, attachment.id])
    try {
      const filename = safeDownloadName(attachment.name)
      if (url.startsWith('blob:') || typeof chrome === 'undefined' || !chrome.downloads?.download) {
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
    if (visible) loadImageAttachmentResources()
  })

  // Only image previews load lazily, so only messages with images watch for
  // visibility instead of every message in a long transcript.
  $effect(() => {
    if (visible || !hasImageAttachments) return
    const observer = new IntersectionObserver((entries) => {
      if (entries.some((entry) => entry.isIntersecting)) {
        visible = true
        observer.disconnect()
      }
    })
    observer.observe(articleElement)
    return () => observer.disconnect()
  })

  onDestroy(() => {
    disposed = true
    for (const url of resourceObjectUrls.values()) {
      URL.revokeObjectURL(url)
    }
  })
</script>

<article
  bind:this={articleElement}
  id={message.id}
  class="grid w-full min-w-0 gap-1 {isUser ? 'justify-items-end' : 'justify-items-start'}"
  class:chat-message-step={processStep}
  class:chat-enter={entering}
  style:--enter-delay={entering ? `${entranceDelay}ms` : undefined}
>
  {#if hasThinkingText && !isTool}
    <div class="chat-message-rows grid w-full max-w-[92%] min-w-0">
      <ChatDetailRow
        rowKey={`${message.id}:thinking`}
        icon={Lightbulb}
        name={getMessage('thinkingProcess')}
        summary={firstLine(thinkingText)}
        live={thinkingLive}
        animate={entering || live}
        enterDelay={rowDelay(0)}
      >
        <div
          class="chat-message-thinking md-content w-full min-w-0 text-xs leading-relaxed text-pretty wrap-break-word"
        >
          {@html renderMarkdown(thinkingText)}
        </div>
      </ChatDetailRow>
    </div>
  {/if}

  {#if hasCard}
    <div
      class={cardClass(
        `relative max-w-[92%] min-w-0 gap-0 overflow-hidden rounded-lg py-0 leading-relaxed shadow-2xs ${
          isUser
            ? 'chat-message-card-user rounded-br-none'
            : isExternalUser
              ? 'chat-message-card-external rounded-bl-none'
              : isSystem
                ? 'chat-message-card-system rounded-bl-none'
                : isTool
                  ? 'chat-message-card-tool'
                  : 'chat-message-card-assistant rounded-none bg-transparent shadow-none ring-0'
        }`
      )}
    >
      <div class={cardContentClass(`min-w-0 ${isAssistant ? 'px-0 py-0' : 'px-3 py-2'}`)}>
        {#if isExternalUser}
          <div
            class="{hasMainText || hasAttachments
              ? 'mb-1.5'
              : ''} flex min-w-0 flex-wrap items-center gap-x-2 gap-y-1 text-xs leading-none"
          >
            <span class="chat-external-sender inline-flex min-w-0 items-center gap-1 font-semibold">
              <span class="chat-external-dot size-1.5 shrink-0 rounded-full"></span>
              <span class="max-w-48 min-w-0 truncate" title={externalUserSenderLabel}>
                {externalUserSenderLabel}
              </span>
            </span>
            {#if externalUserContextLabel}
              <span class="chat-external-context min-w-0 truncate" title={externalUserContextLabel}>
                {externalUserContextLabel}
              </span>
            {/if}
          </div>
        {/if}

        {#if hasMainText}
          <!-- Clicks reach the copy buttons the code blocks get (real buttons, so keyboard works). -->
          <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
          <div
            bind:this={contentElement}
            class="md-content w-full min-w-0 text-pretty wrap-break-word"
            onclick={copyCodeFromClick}
          >
            {@html html}
          </div>
        {/if}

        {#if message.attachments?.length}
          <div class="chat-enter-block {hasMainText ? 'mt-2' : ''} grid min-w-0 gap-1.5">
            {#each message.attachments as attachment (attachment.id)}
              <div
                class="chat-message-attachment max-w-full min-w-0 overflow-hidden rounded-md border p-1.5 text-xs"
              >
                <div class="flex min-w-0 items-center gap-2">
                  <div
                    class="chat-message-attachment-icon grid size-9 shrink-0 place-items-center overflow-hidden rounded-sm border text-emerald-700"
                  >
                    {#if attachmentMimeType(attachment).startsWith('image/') && attachmentObjectUrl(attachment, caches)}
                      <img
                        src={attachmentObjectUrl(attachment, caches)}
                        alt={attachment.name}
                        class="size-full object-cover"
                      />
                    {:else if attachmentMimeType(attachment).startsWith('image/') && loadingResourceIds.has(attachmentResourceId(attachment))}
                      <LoaderCircle class="size-4 animate-spin" />
                    {:else if attachmentMimeType(attachment).startsWith('image/')}
                      <Image class="size-4" />
                    {:else}
                      <FileText class="size-4" />
                    {/if}
                  </div>

                  <div class="min-w-0 flex-1">
                    <div
                      class="chat-message-attachment-name truncate font-medium"
                      title={attachment.name}
                    >
                      {attachment.name}
                    </div>
                    {#if attachmentMetaLabel(attachment)}
                      <div class="chat-message-attachment-meta truncate text-[10px]">
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
                    disabled={!attachmentHasDownloadData(attachment, caches) ||
                      downloadingAttachmentIds.has(attachment.id) ||
                      loadingResourceIds.has(attachmentResourceId(attachment))}
                    aria-label={getMessage('saveAttachment', attachment.name)}
                    title={attachmentSaveTitle(attachment)}
                    onclick={() => saveAttachment(attachment)}
                  >
                    {#if downloadingAttachmentIds.has(attachment.id) || loadingResourceIds.has(attachmentResourceId(attachment))}
                      <LoaderCircle class="size-3 animate-spin" />
                    {:else}
                      <Download class="size-3" />
                    {/if}
                  </button>
                </div>

                {#if attachmentDescription(attachment)}
                  <div
                    class="chat-message-attachment-description mt-1.5 max-h-44 overflow-x-hidden overflow-y-auto rounded-sm border px-2 py-1.5 text-xs leading-relaxed wrap-break-word whitespace-pre-wrap"
                  >
                    {attachmentDescription(attachment)}
                  </div>
                {/if}
              </div>
            {/each}
          </div>
        {/if}

        {#if message.actions?.length}
          <div
            class="chat-enter-block {hasMainText || hasAttachments
              ? 'mt-2'
              : ''} grid min-w-0 gap-2"
          >
            {#each message.actions as action (action.id)}
              {@const pending = actionPending(action)}
              {@const selectedChoice = actionSelectedChoice(action)}
              <!-- The record of an action; a pending one is answered in the dock above the composer. -->
              <div
                class="chat-action-card grid min-w-0 gap-2 rounded-lg border px-3 py-2 text-xs shadow-2xs"
                class:chat-action-card-pending={pending}
              >
                <div class="flex min-w-0 items-center gap-2">
                  <div
                    class="chat-action-icon grid size-8 shrink-0 place-items-center rounded-md border"
                  >
                    {#if isShellApproval(action)}
                      <Terminal class="size-4" />
                    {:else if isPaymentApproval(action)}
                      <CreditCard class="size-4" />
                    {:else if isApprovalAction(action)}
                      <ShieldCheck class="size-4" />
                    {:else}
                      <ListChecks class="size-4" />
                    {/if}
                  </div>
                  <div class="min-w-0 flex-1">
                    <div
                      class="truncate text-sm font-semibold"
                      title={actionTitle(action) || actionKindLabel(action)}
                    >
                      {actionTitle(action) || actionKindLabel(action)}
                    </div>
                    <div class="chat-action-meta truncate">
                      {actionKindLabel(action)} · {pending
                        ? getMessage('actionWaiting')
                        : actionStatusLabel(action)}
                    </div>
                  </div>
                </div>

                {#if actionMessage(action)}
                  <div
                    class="chat-action-message leading-relaxed wrap-break-word whitespace-pre-wrap"
                  >
                    {actionMessage(action)}
                  </div>
                {/if}

                {#if !pending}
                  {#if action.summary}
                    <div class="chat-action-summary rounded-md border px-2 py-1.5 wrap-break-word">
                      {action.summary}
                    </div>
                  {/if}

                  {#if action.details?.length}
                    <div class="grid min-w-0 gap-1.5">
                      {#each action.details as detail, detailIndex (`${action.id}-${detail.label}-${detailIndex}`)}
                        {@const link = actionDetailUrl(detail)}
                        <div class="chat-action-detail min-w-0 rounded-md border px-2 py-1.5">
                          <div class="chat-action-meta mb-1 text-[10px] font-semibold uppercase">
                            {actionDetailLabel(detail)}
                          </div>
                          {#if link}
                            <a
                              class="wrap-break-word underline underline-offset-2"
                              href={link}
                              target="_blank"
                              rel="noopener noreferrer">{link}</a
                            >
                          {:else if actionDetailIsBlock(detail)}
                            <pre class="min-w-0 overflow-x-auto whitespace-pre-wrap"><code
                                >{actionDetailText(detail)}</code
                              ></pre>
                          {:else}
                            <div class="wrap-break-word">{actionDetailText(detail)}</div>
                          {/if}
                        </div>
                      {/each}
                    </div>
                  {:else if action.command}
                    <pre
                      class="chat-action-command min-w-0 overflow-x-auto rounded-md border px-2 py-1.5"><code
                        >{action.command}</code
                      ></pre>
                    {#if action.workspace}
                      <div class="chat-action-meta truncate" title={action.workspace}>
                        {action.workspace}
                        {#if action.background}
                          · {getMessage('actionBackground')}
                        {/if}
                      </div>
                    {/if}
                  {/if}

                  {#if selectedChoice}
                    <div
                      class="chat-action-choice-selected flex min-w-0 items-start gap-2 rounded-md border px-2 py-1.5"
                    >
                      <CircleCheck class="mt-0.5 size-3.5 shrink-0" />
                      <span class="min-w-0">
                        <span class="block font-medium">{selectedChoice.label}</span>
                        {#if selectedChoice.description}
                          <span class="chat-action-meta block text-xs font-normal">
                            {selectedChoice.description}
                          </span>
                        {/if}
                        {#if actionChoiceText(action)}
                          <span
                            class="chat-action-choice-text mt-1 block rounded-md border px-2 py-1.5 text-xs font-normal wrap-break-word whitespace-pre-wrap"
                          >
                            {actionChoiceText(action)}
                          </span>
                        {/if}
                      </span>
                    </div>
                  {/if}
                {/if}
              </div>
            {/each}
          </div>
        {/if}
      </div>
    </div>
  {/if}

  {#snippet toolRows()}
    {#each tools as tool, index (index)}
      <ChatDetailRow
        rowKey={`${message.id}:tool:${index}`}
        icon={toolIcons[toolKind(tool.name)]}
        name={tool.name}
        summary={toolCallSummary(tool)}
        mono
        status={toolCallStatus(tool)}
        live={live && toolCallStatus(tool) === 'running'}
        animate={entering || live}
        enterDelay={rowDelay(index + 1)}
      >
        <div class="grid min-w-0 gap-1.5">
          {#each toolDetailSections(tool) as section, sectionIndex (sectionIndex)}
            <div class="grid min-w-0 gap-0.5">
              <div class="chat-tool-section-label text-[10px] font-semibold">
                {toolSectionLabel(section)}
              </div>
              <pre
                class="chat-tool-section-text max-h-72 min-w-0 overflow-auto rounded-md border px-2 py-1.5 font-mono text-[11px] leading-relaxed wrap-break-word whitespace-pre-wrap">{section.text}</pre>
            </div>
          {/each}
        </div>
      </ChatDetailRow>
    {/each}
  {/snippet}
  {#if tools.length || notices.length}
    <div class="chat-message-rows grid w-full max-w-[92%] min-w-0">
      {#if groupedTools}
        <ChatDetailRow
          rowKey={`${message.id}:tools`}
          icon={Layers}
          name={getMessage('toolCallsCount', String(tools.length))}
          summary={toolGroupSummary}
          status={toolGroupStatus}
          live={live && toolGroupStatus === 'running'}
          animate={entering || live}
          enterDelay={rowDelay(1)}
        >
          <div class="grid min-w-0">{@render toolRows()}</div>
        </ChatDetailRow>
      {:else}
        {@render toolRows()}
      {/if}
      {#each notices as notice, index (index)}
        <ChatDetailRow
          rowKey={`${message.id}:notice:${index}`}
          icon={Bell}
          name={notice.kind || getMessage('runtimeNotice')}
          summary={firstLine(notice.body)}
          animate={entering}
          enterDelay={rowDelay(index)}
        >
          <pre
            class="chat-tool-section-text max-h-72 min-w-0 overflow-auto rounded-md border px-2 py-1.5 font-mono text-[11px] leading-relaxed wrap-break-word whitespace-pre-wrap">{notice.body}</pre>
        </ChatDetailRow>
      {/each}
    </div>
  {/if}

  {#if hasCard && !processStep}
    <div
      class="chat-message-meta flex min-h-5 max-w-[92%] items-center gap-1 px-0.5 text-[10px] leading-none {isUser
        ? 'justify-end'
        : 'justify-start'}"
    >
      {#if mainText}
        <button
          type="button"
          class={messageActionButtonClass}
          aria-label={getMessage('copyMessage')}
          title={getMessage('copyMessage')}
          onclick={copyMessage}
        >
          {#if copied}
            <Check class="chat-action-pop size-3.5" />
            <span class="chat-action-burst" aria-hidden="true"></span>
          {:else}
            <Copy class="size-3.5" />
          {/if}
        </button>
      {/if}
      {#if canToggleQuickPrompt}
        <button
          type="button"
          class={messageActionButtonClass}
          class:chat-message-bookmarked={quickPromptActive}
          aria-label={quickPromptActive
            ? getMessage('removeQuickPrompt')
            : getMessage('addQuickPrompt')}
          aria-pressed={quickPromptActive}
          title={quickPromptActive ? getMessage('removeQuickPrompt') : getMessage('addQuickPrompt')}
          onclick={toggleQuickPrompt}
        >
          {#if quickPromptActive}
            <Check class="size-3.5 {quickPromptPopped ? 'chat-action-pop' : ''}" />
            {#if quickPromptPopped}
              <span class="chat-action-burst" aria-hidden="true"></span>
            {/if}
          {:else}
            <Plus class="size-3.5" />
          {/if}
        </button>
      {/if}
      {#if !isUser}
        {#if mainText && !compactActions}
          <button
            type="button"
            class={messageActionButtonClass}
            aria-label={getMessage('copyRichText')}
            title={getMessage('copyRichText')}
            onclick={copyRichMessage}
          >
            {#if richCopied}
              <Check class="chat-action-pop size-3.5" />
              <span class="chat-action-burst" aria-hidden="true"></span>
            {:else}
              <Clipboard class="size-3.5" />
            {/if}
          </button>
        {/if}
        {#if (hasMainText || hasAttachments) && !compactActions}
          <button
            type="button"
            class={messageActionButtonClass}
            aria-label={getMessage('printMessage')}
            title={getMessage('printMessage')}
            onclick={printMessage}
          >
            <Printer class="size-3.5" />
          </button>
        {/if}
        {#if canBookmark}
          <button
            type="button"
            class={messageActionButtonClass}
            class:chat-message-bookmarked={bookmarked}
            aria-label={bookmarked ? getMessage('removeBookmark') : getMessage('bookmark')}
            aria-pressed={bookmarked}
            title={bookmarked ? getMessage('removeBookmark') : getMessage('bookmark')}
            onclick={toggleBookmark}
          >
            {#if bookmarked}
              <BookmarkCheck class="size-3.5 {bookmarkPopped ? 'chat-action-pop' : ''}" />
              {#if bookmarkPopped}
                <span class="chat-action-burst" aria-hidden="true"></span>
              {/if}
            {:else}
              <Bookmark class="size-3.5" />
            {/if}
          </button>
        {/if}
      {/if}
      {#if compactActions && !isUser && moreActions.length}
        <DropdownMenu
          class="{messageActionButtonClass} justify-center"
          items={moreActions}
          onSelect={runMoreAction}
          ariaLabel={getMessage('moreMessageActions')}
          title={getMessage('moreMessageActions')}
        >
          {#snippet trigger()}<MoreHorizontal class="size-3.5" />{/snippet}
        </DropdownMenu>
      {/if}
      {#if memoryActivity}
        <span
          class="px-1 text-[11px] text-muted-foreground"
          title={getMessage('memoryEvidenceHint')}
        >
          {memoryActivityLabel(memoryActivity.state)}{memoryActivity.stale
            ? ` · ${getMessage('memoryStale')}`
            : ''}
        </span>
      {/if}
      {#if messageTimeLabel}
        <span class="chat-message-time px-1">{messageTimeLabel}</span>
      {/if}
    </div>
  {/if}
</article>

<style>
  .chat-message-card-user {
    background: var(--message-user-bubble, #f4f4f4);
    color: var(--message-text, #171717);
    box-shadow:
      inset 0 0 0 1px color-mix(in srgb, var(--message-border, #e6e6e6) 72%, transparent),
      0 1px 2px rgba(0, 0, 0, 0.03);
  }

  .chat-message-card-assistant {
    color: var(--message-text, #171717);
  }

  .chat-message-card-tool {
    border-color: var(--message-border, #e6e6e6);
    background: var(--message-surface, #f7f7f7);
    color: var(--message-muted, #737373);
  }

  .chat-message-card-external {
    border-color: rgba(13, 148, 136, 0.24);
    background: color-mix(in srgb, var(--message-bg, #ffffff) 72%, #ccfbf1);
    color: #115e59;
  }

  .chat-message-card-system {
    border-color: rgba(180, 83, 9, 0.24);
    background: color-mix(in srgb, var(--message-bg, #ffffff) 74%, #fef3c7);
    color: #78350f;
  }

  .chat-external-sender {
    color: #0f766e;
  }

  .chat-external-dot {
    background: #14b8a6;
  }

  .chat-external-context {
    color: rgba(15, 118, 110, 0.72);
  }

  .chat-message-action {
    color: var(--message-muted, #737373);
  }

  .chat-message-action:hover {
    background: var(--message-surface-hover, #eeeeee);
    color: var(--message-text, #171717);
  }

  .chat-message-action.chat-message-bookmarked,
  .chat-message-action.chat-message-bookmarked:hover {
    color: #047857;
  }

  :global(.dark) .chat-message-action.chat-message-bookmarked,
  :global(.dark) .chat-message-action.chat-message-bookmarked:hover {
    color: #34d399;
  }

  .chat-message-attachment {
    border-color: var(--message-border, #e6e6e6);
    background: color-mix(in srgb, var(--message-bg, #ffffff) 68%, var(--message-surface, #f7f7f7));
    color: var(--message-muted, #737373);
  }

  .chat-message-attachment-icon {
    border-color: var(--message-border, #e6e6e6);
    background: var(--message-surface-strong, #f4f4f4);
  }

  .chat-message-attachment-name {
    color: var(--message-text, #171717);
  }

  .chat-message-attachment-meta,
  .chat-message-thinking,
  .chat-message-meta,
  .chat-message-time {
    color: var(--message-muted, #737373);
  }

  .chat-message-attachment-description {
    border-color: var(--message-border, #e6e6e6);
    background: var(--message-surface-strong, #f4f4f4);
    color: color-mix(in srgb, var(--message-text, #171717) 82%, transparent);
  }

  .chat-action-card {
    border-color: color-mix(in srgb, var(--message-border, #e6e6e6) 78%, #0f766e);
    background: color-mix(in srgb, var(--message-bg, #ffffff) 76%, #ecfdf5);
    color: var(--message-text, #171717);
  }

  /* Waiting on the dock above the composer. */
  .chat-action-card-pending {
    border-style: dashed;
  }

  .chat-action-detail,
  .chat-action-summary,
  .chat-action-icon,
  .chat-action-command {
    border-color: var(--message-border, #e6e6e6);
    background: color-mix(in srgb, var(--message-bg, #ffffff) 72%, var(--message-surface, #f7f7f7));
  }

  .chat-action-meta {
    color: var(--message-muted, #737373);
  }

  .chat-action-message {
    color: color-mix(in srgb, var(--message-text, #171717) 86%, transparent);
  }

  .chat-action-command,
  .chat-action-detail,
  .chat-action-summary {
    color: color-mix(in srgb, var(--message-text, #171717) 88%, transparent);
  }

  .chat-action-choice-text {
    border-color: var(--message-border, #e6e6e6);
    background: color-mix(in srgb, var(--message-bg, #ffffff) 74%, var(--message-surface, #f7f7f7));
    color: color-mix(in srgb, var(--message-text, #171717) 90%, transparent);
  }

  .chat-action-choice-selected {
    border-color: color-mix(in srgb, var(--message-border, #e6e6e6) 58%, #047857);
    background: color-mix(in srgb, var(--message-bg, #ffffff) 68%, #ecfdf5);
    color: #065f46;
  }

  /* Steps of one turn sit closer together than separate messages. */
  .chat-message-step {
    margin-bottom: -0.5rem;
  }

  /* Rows start flush with the prose: the row padding hangs into the gutter. */
  .chat-message-rows {
    margin-left: -0.375rem;
  }

  .chat-tool-section-label {
    color: var(--message-muted, #737373);
  }

  .chat-tool-section-text {
    border-color: var(--message-border, #e6e6e6);
    background: var(--message-surface-strong, #f4f4f4);
    color: color-mix(in srgb, var(--message-text, #171717) 86%, transparent);
  }

  :global(.dark) .chat-tool-section-text {
    background: color-mix(in srgb, var(--message-bg, #2a2a2a) 72%, #171717);
  }

  :global(.dark) .chat-message-card-external {
    border-color: rgba(45, 212, 191, 0.22);
    background: color-mix(in srgb, var(--message-bg, #2a2a2a) 82%, #0f766e);
    color: #ccfbf1;
  }

  :global(.dark) .chat-message-card-system {
    border-color: rgba(251, 191, 36, 0.24);
    background: color-mix(in srgb, var(--message-bg, #2a2a2a) 82%, #92400e);
    color: #fde68a;
  }

  :global(.dark) .chat-external-sender {
    color: #99f6e4;
  }

  :global(.dark) .chat-external-dot {
    background: #2dd4bf;
  }

  :global(.dark) .chat-external-context {
    color: rgba(153, 246, 228, 0.68);
  }

  :global(.dark) .chat-action-card {
    border-color: rgba(45, 212, 191, 0.18);
    background: color-mix(in srgb, var(--message-bg, #2a2a2a) 84%, #064e3b);
  }

  :global(.dark) .chat-action-detail,
  :global(.dark) .chat-action-summary,
  :global(.dark) .chat-action-icon,
  :global(.dark) .chat-action-command {
    border-color: rgba(255, 255, 255, 0.1);
    background: color-mix(in srgb, var(--message-bg, #2a2a2a) 72%, #171717);
  }

  :global(.dark) .chat-action-choice-text {
    border-color: rgba(255, 255, 255, 0.12);
    background: rgba(15, 23, 42, 0.28);
    color: #d1fae5;
  }

  :global(.dark) .chat-action-choice-selected {
    border-color: rgba(45, 212, 191, 0.28);
    background: color-mix(in srgb, var(--message-bg, #2a2a2a) 78%, #064e3b);
    color: #99f6e4;
  }

  /* Entrances, for messages that just arrived (chat/entrance.ts). A prompt
     springs up from the composer; an answer settles in block by block. */
  .chat-enter .chat-message-card-user {
    transform-origin: 100% 100%;
    animation: chat-bubble-in 640ms var(--anda-spring, ease-out) var(--enter-delay, 0ms) both;
  }

  .chat-enter .chat-message-card-external,
  .chat-enter .chat-message-card-system,
  .chat-enter .chat-message-card-tool {
    transform-origin: 0% 100%;
    animation: chat-bubble-in 640ms var(--anda-spring, ease-out) var(--enter-delay, 0ms) both;
  }

  .chat-enter .chat-message-card-assistant .md-content > :global(*) {
    animation: chat-block-in 720ms var(--anda-ease-out, ease-out)
      calc(var(--enter-delay, 0ms) + min(var(--i, 0), 12) * 55ms) both;
  }

  .chat-enter .chat-message-card-assistant .chat-enter-block {
    animation: chat-block-in 720ms var(--anda-ease-out, ease-out)
      calc(var(--enter-delay, 0ms) + 140ms) both;
  }

  .chat-enter .chat-message-meta {
    animation: chat-meta-in 480ms ease-out calc(var(--enter-delay, 0ms) + 320ms) both;
  }

  @keyframes chat-bubble-in {
    from {
      opacity: 0;
      transform: translateY(18px) scale(0.9);
      filter: blur(4px);
    }
    40% {
      filter: blur(0);
    }
  }

  @keyframes chat-block-in {
    from {
      opacity: 0;
      transform: translateY(10px);
      filter: blur(6px);
    }
  }

  @keyframes chat-meta-in {
    from {
      opacity: 0;
    }
  }

  /* Copy, bookmark and quick-prompt confirmations pop with a small burst. */
  .chat-message-action :global(.chat-action-pop) {
    animation: chat-action-pop 480ms var(--anda-spring, ease-out) both;
  }

  .chat-action-burst {
    --burst: #10b981;
    position: absolute;
    top: 50%;
    left: 50%;
    width: 3px;
    height: 3px;
    margin: -1.5px;
    border-radius: 999px;
    pointer-events: none;
    animation: chat-action-burst 560ms var(--anda-ease-out, ease-out) both;
  }

  :global(.dark) .chat-action-burst {
    --burst: #34d399;
  }

  @keyframes chat-action-pop {
    from {
      opacity: 0;
      transform: scale(0.3) rotate(-25deg);
    }
  }

  @keyframes chat-action-burst {
    0% {
      box-shadow:
        0 0 0 0 var(--burst),
        0 0 0 0 var(--burst),
        0 0 0 0 var(--burst),
        0 0 0 0 var(--burst),
        0 0 0 0 var(--burst),
        0 0 0 0 var(--burst),
        0 0 0 0 var(--burst),
        0 0 0 0 var(--burst);
    }
    55% {
      box-shadow:
        0 -9px 0 0 var(--burst),
        6.4px -6.4px 0 0 var(--burst),
        9px 0 0 0 var(--burst),
        6.4px 6.4px 0 0 var(--burst),
        0 9px 0 0 var(--burst),
        -6.4px 6.4px 0 0 var(--burst),
        -9px 0 0 0 var(--burst),
        -6.4px -6.4px 0 0 var(--burst);
    }
    100% {
      box-shadow:
        0 -13px 0 -1.5px var(--burst),
        9.2px -9.2px 0 -1.5px var(--burst),
        13px 0 0 -1.5px var(--burst),
        9.2px 9.2px 0 -1.5px var(--burst),
        0 13px 0 -1.5px var(--burst),
        -9.2px 9.2px 0 -1.5px var(--burst),
        -13px 0 0 -1.5px var(--burst),
        -9.2px -9.2px 0 -1.5px var(--burst);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .chat-enter .chat-message-card-user,
    .chat-enter .chat-message-card-external,
    .chat-enter .chat-message-card-system,
    .chat-enter .chat-message-card-tool,
    .chat-enter .chat-message-card-assistant .md-content > :global(*),
    .chat-enter .chat-message-card-assistant .chat-enter-block,
    .chat-enter .chat-message-meta,
    .chat-message-action :global(.chat-action-pop) {
      animation: none;
    }

    .chat-action-burst {
      display: none;
    }
  }
</style>
