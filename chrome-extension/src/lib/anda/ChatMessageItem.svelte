<script lang="ts" module>
  const expandedDetailMessageIds = new Set<string>()
</script>

<script lang="ts">
  import { andaClient } from '$lib/anda/client/side-panel.svelte'
  import type {
    ChatAction,
    ChatActionDetail,
    ChatAttachment,
    ChatMessage
  } from '$lib/anda/client/types'
  import { getMessage } from '$lib/i18n'
  import { buttonClass, cardClass, cardContentClass } from '$lib/anda/ui'
  import { renderMarkdown } from '$lib/utils/markdown'
  import {
    Bookmark,
    BookmarkCheck,
    Check,
    CircleCheck,
    CircleX,
    Clipboard,
    Copy,
    CreditCard,
    Download,
    FileText,
    Image,
    ListChecks,
    LoaderCircle,
    Plus,
    Printer,
    ShieldCheck,
    Terminal,
    Wrench
  } from '@lucide/svelte'
  import { onDestroy, onMount, tick } from 'svelte'

  let {
    message,
    quickPromptActive = false,
    onToggleQuickPrompt
  }: {
    message: ChatMessage
    quickPromptActive?: boolean
    onToggleQuickPrompt?: (text: string) => Promise<void> | void
  } = $props()

  let copied = $state(false)
  let richCopied = $state(false)
  let detailsExpanded = $state(false)
  let downloadingAttachmentIds = $state(new Set<string>())
  let respondingActionIds = $state(new Set<string>())
  let actionErrors = $state(new Map<string, string>())
  let resourceBlobs = $state(new Map<number, string>())
  let resourceObjectUrls = $state(new Map<string, string>())
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
  const hasActions = $derived(Boolean(message.actions?.length))
  const hasThinkingText = $derived(Boolean(thinkingText))
  // Only settled assistant messages with a stable server id can be bookmarked
  // (excludes optimistic/local, side, and compacted tool/thinking-only items).
  const canBookmark = $derived(
    isAssistant && hasMainText && !message.pending && /^m-\d+-\d+$/.test(message.id)
  )
  const bookmarked = $derived(canBookmark && andaClient.isBookmarked(message.id))
  const canToggleQuickPrompt = $derived(isUser && hasMainText && Boolean(onToggleQuickPrompt))
  const messageTimeLabel = $derived(timeLabel(message.timestamp))
  const externalUserSenderLabel = $derived(
    message.externalUser?.sender || message.externalUser?.scope || 'External user'
  )
  const externalUserContextLabel = $derived(
    [message.externalUser?.channel, message.externalUser?.space].filter(Boolean).join(' / ')
  )
  const detailLabel = $derived(isTool ? 'tool output' : 'thinking and tools')
  const [html, hook] = $derived.by(() => renderMarkdown(mainText))
  const [thinkingHtml, thinkingHook] = $derived.by(() => renderMarkdown(thinkingText))
  const messageActionButtonClass = buttonClass(
    'ghost',
    'icon-xs',
    'chat-message-action size-5 rounded-sm'
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

    const roleLabel = isUser
      ? 'User'
      : isExternalUser
        ? 'External user'
        : isSystem
          ? 'System'
          : isTool
            ? 'Tool'
            : 'Assistant'
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

  function actionKindLabel(action: ChatAction): string {
    if (isApprovalAction(action)) {
      const toolLabel = actionToolLabel(action)
      return getMessage('actionApprovalKindLabel', toolLabel)
    }
    if (action.kind === 'choice') {
      return getMessage('actionChoiceKindLabel')
    }
    return actionTitle(action) || getMessage('actionFallbackTitle')
  }

  function actionStatusLabel(action: ChatAction): string {
    switch (action.status) {
      case 'pending':
        return getMessage('actionStatusPending')
      case 'approved':
        return getMessage('actionStatusApproved')
      case 'denied':
        return getMessage('actionStatusDenied')
      case 'selected':
        return getMessage('actionStatusSelected')
      case 'expired':
        return getMessage('actionStatusExpired')
      default:
        return action.status || getMessage('actionStatusUnknown')
    }
  }

  function actionResponseLabel(action: ChatAction): string {
    if (action.status === 'selected') {
      const choiceId =
        action.response && typeof action.response === 'object' && !Array.isArray(action.response)
          ? action.response.choice_id
          : undefined
      const choice = action.choices?.find((item) => item.id === choiceId)
      return choice?.label || (typeof choiceId === 'string' ? choiceId : '')
    }
    return ''
  }

  function actionPending(action: ChatAction): boolean {
    return action.status === 'pending'
  }

  function isApprovalAction(action: ChatAction): boolean {
    return action.kind === 'tool_approval' || action.kind === 'shell_command'
  }

  function actionToolName(action: ChatAction): string {
    return (action.tool?.name || '').toLowerCase()
  }

  function isShellApproval(action: ChatAction): boolean {
    const toolName = actionToolName(action)
    return action.kind === 'shell_command' || toolName === 'shell' || toolName.includes('shell')
  }

  function isPaymentApproval(action: ChatAction): boolean {
    const toolName = actionToolName(action)
    return toolName.includes('pay') || toolName.includes('payment')
  }

  function actionToolLabel(action: ChatAction): string {
    if (isShellApproval(action)) {
      return getMessage('shellCommandTool')
    }
    return action.tool?.label || action.tool?.name || getMessage('actionToolFallback')
  }

  function actionTitle(action: ChatAction): string {
    if (isShellApproval(action) && (!action.title || action.title === 'Approve shell command')) {
      return getMessage('shellApprovalTitle')
    }
    return action.title || ''
  }

  function actionMessage(action: ChatAction): string | null | undefined {
    if (
      isShellApproval(action) &&
      (!action.message || action.message === 'The agent wants to run a local shell command.')
    ) {
      return getMessage('shellApprovalMessage')
    }
    return action.message
  }

  function actionApproveLabel(action: ChatAction): string {
    const label = action.approval?.approveLabel
    return label && label !== 'Approve' ? label : getMessage('actionApprove')
  }

  function actionDenyLabel(action: ChatAction): string {
    const label = action.approval?.denyLabel
    return label && label !== 'Deny' ? label : getMessage('actionDeny')
  }

  function actionDetailLabel(detail: ChatActionDetail): string {
    switch (detail.label) {
      case 'Command':
        return getMessage('actionDetailCommand')
      case 'Workspace':
        return getMessage('actionDetailWorkspace')
      case 'Approval reason':
        return getMessage('actionDetailApprovalReason')
      case 'Mode':
        return getMessage('actionDetailMode')
      case 'Environment keys':
        return getMessage('actionDetailEnvironmentKeys')
      default:
        return detail.label
    }
  }

  function detailText(detail: ChatActionDetail): string {
    const value = detail.value
    if (typeof value === 'string') {
      if (detail.label === 'Mode') {
        if (value === 'background') {
          return getMessage('actionBackground')
        }
        if (value === 'foreground') {
          return getMessage('actionForeground')
        }
      }
      return value
    }
    if (value === null) {
      return ''
    }
    return JSON.stringify(value, null, 2)
  }

  function detailIsBlock(detail: ChatActionDetail): boolean {
    return detail.format === 'code' || detail.format === 'json' || detail.format === 'list'
  }

  function setActionResponding(actionId: string, value: boolean) {
    const next = new Set(respondingActionIds)
    if (value) {
      next.add(actionId)
    } else {
      next.delete(actionId)
    }
    respondingActionIds = next
  }

  function setActionError(actionId: string, error: string | null) {
    const next = new Map(actionErrors)
    if (error) {
      next.set(actionId, error)
    } else {
      next.delete(actionId)
    }
    actionErrors = next
  }

  function errorLabel(error: unknown): string {
    return error instanceof Error ? error.message : String(error || getMessage('actionFailed'))
  }

  async function respondApprovalAction(action: ChatAction, approve: boolean) {
    if (!actionPending(action) || respondingActionIds.has(action.id)) {
      return
    }
    setActionResponding(action.id, true)
    setActionError(action.id, null)
    try {
      await andaClient.respondAction({ actionId: action.id, approve })
    } catch (error) {
      setActionError(action.id, errorLabel(error))
    } finally {
      setActionResponding(action.id, false)
    }
  }

  async function selectChoiceAction(action: ChatAction, choiceId: string) {
    if (!actionPending(action) || respondingActionIds.has(action.id)) {
      return
    }
    setActionResponding(action.id, true)
    setActionError(action.id, null)
    try {
      await andaClient.respondAction({ actionId: action.id, choiceId })
    } catch (error) {
      setActionError(action.id, errorLabel(error))
    } finally {
      setActionResponding(action.id, false)
    }
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
  class="grid min-w-0 w-full gap-1 {isUser
    ? 'justify-items-end'
    : isTool || (!hasMainText && !hasAttachments && !hasActions)
      ? 'justify-items-center'
      : 'justify-items-start'}"
>
  {#if hasThinkingText && (isTool || (!hasMainText && !hasAttachments && !hasActions))}
    <button
      type="button"
      class={buttonClass(
        'outline',
        'xs',
        'chat-message-muted-button rounded-full shadow-sm hover:border-emerald-200 hover:text-emerald-700'
      )}
      onclick={toggleDetails}
    >
      <Wrench class="size-3" />
      <span class="text-xs">{detailsExpanded ? `Hide ${detailLabel}` : `Show ${detailLabel}`}</span>
    </button>
  {/if}

  {#if hasMainText || hasAttachments || hasActions}
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
              <span class="min-w-0 max-w-48 truncate" title={externalUserSenderLabel}>
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
          <div class="md-content w-full min-w-0 text-pretty wrap-break-word">{@html html}</div>
        {/if}

        {#if message.attachments?.length}
          <div class="{hasMainText ? 'mt-2' : ''} grid min-w-0 gap-1.5">
            {#each message.attachments as attachment (attachment.id)}
              <div
                class="chat-message-attachment min-w-0 max-w-full overflow-hidden rounded-md border p-1.5 text-xs"
              >
                <div class="flex min-w-0 items-center gap-2">
                  <div
                    class="chat-message-attachment-icon grid size-9 shrink-0 place-items-center overflow-hidden rounded-sm border text-emerald-700"
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
                    class="chat-message-attachment-description mt-1.5 max-h-44 overflow-x-hidden overflow-y-auto rounded-sm border px-2 py-1.5 whitespace-pre-wrap text-xs leading-relaxed wrap-break-word"
                  >
                    {attachmentDescription(attachment)}
                  </div>
                {/if}
              </div>
            {/each}
          </div>
        {/if}

        {#if message.actions?.length}
          <div class="{hasMainText || hasAttachments ? 'mt-2' : ''} grid min-w-0 gap-2">
            {#each message.actions as action (action.id)}
              <div
                class="chat-action-card grid min-w-0 gap-2 rounded-lg border px-3 py-2 text-xs shadow-2xs"
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
                    <div class="truncate text-sm font-semibold" title={actionTitle(action) || actionKindLabel(action)}>
                      {actionTitle(action) || actionKindLabel(action)}
                    </div>
                    <div class="chat-action-meta truncate">
                      {actionKindLabel(action)} · {actionStatusLabel(action)}
                      {#if actionResponseLabel(action)}
                        · {actionResponseLabel(action)}
                      {/if}
                    </div>
                  </div>
                </div>

                {#if actionMessage(action)}
                  <div class="chat-action-message leading-relaxed whitespace-pre-wrap wrap-break-word">
                    {actionMessage(action)}
                  </div>
                {/if}

                {#if action.summary}
                  <div class="chat-action-summary rounded-md border px-2 py-1.5 wrap-break-word">
                    {action.summary}
                  </div>
                {/if}

                {#if action.details?.length}
                  <div class="grid min-w-0 gap-1.5">
                    {#each action.details as detail, detailIndex (`${action.id}-${detail.label}-${detailIndex}`)}
                      <div class="chat-action-detail min-w-0 rounded-md border px-2 py-1.5">
                        <div class="chat-action-meta mb-1 text-[10px] font-semibold uppercase">
                          {actionDetailLabel(detail)}
                        </div>
                        {#if detailIsBlock(detail)}
                          <pre class="min-w-0 overflow-x-auto whitespace-pre-wrap"><code>{detailText(detail)}</code></pre>
                        {:else}
                          <div class="wrap-break-word">{detailText(detail)}</div>
                        {/if}
                      </div>
                    {/each}
                  </div>
                {:else if action.command}
                  <pre class="chat-action-command min-w-0 overflow-x-auto rounded-md border px-2 py-1.5"><code>{action.command}</code></pre>
                  {#if action.workspace}
                    <div class="chat-action-meta truncate" title={action.workspace}>
                      {action.workspace}
                      {#if action.background}
                        · {getMessage('actionBackground')}
                      {/if}
                    </div>
                  {/if}
                {/if}

                {#if actionErrors.has(action.id)}
                  <div class="chat-action-error rounded-md px-2 py-1">
                    {actionErrors.get(action.id)}
                  </div>
                {/if}

                {#if actionPending(action)}
                  {#if isApprovalAction(action)}
                    <div class="flex min-w-0 flex-wrap gap-2">
                      <button
                        type="button"
                        class={buttonClass('default', 'xs', 'chat-action-approve')}
                        disabled={respondingActionIds.has(action.id)}
                        onclick={() => respondApprovalAction(action, true)}
                      >
                        {#if respondingActionIds.has(action.id)}
                          <LoaderCircle class="size-3 animate-spin" />
                        {:else}
                          <CircleCheck class="size-3" />
                        {/if}
                        <span>{actionApproveLabel(action)}</span>
                      </button>
                      <button
                        type="button"
                        class={buttonClass('outline', 'xs', 'chat-action-deny')}
                        disabled={respondingActionIds.has(action.id)}
                        onclick={() => respondApprovalAction(action, false)}
                      >
                        <CircleX class="size-3" />
                        <span>{actionDenyLabel(action)}</span>
                      </button>
                    </div>
                  {:else if action.choices?.length}
                    <div class="grid min-w-0 gap-1.5">
                      {#each action.choices as choice (choice.id)}
                        <button
                          type="button"
                          class={buttonClass(
                            'outline',
                            'sm',
                            'chat-action-choice-button h-auto min-w-0 justify-start whitespace-normal px-2 py-1.5 text-left'
                          )}
                          disabled={respondingActionIds.has(action.id)}
                          onclick={() => selectChoiceAction(action, choice.id)}
                        >
                          <span class="min-w-0">
                            <span class="block font-medium">{choice.label}</span>
                            {#if choice.description}
                              <span class="chat-action-meta block text-xs font-normal">
                                {choice.description}
                              </span>
                            {/if}
                          </span>
                        </button>
                      {/each}
                    </div>
                  {/if}
                {/if}
              </div>
            {/each}
          </div>
        {/if}

        {#if hasThinkingText}
          <div
            class="mt-1.5 flex min-h-5 items-center gap-2 {hasThinkingText && !isAssistant
              ? 'chat-message-detail-divider border-t pt-1.5'
              : ''}"
          >
            {#if hasThinkingText}
              <button
                type="button"
                class={buttonClass(
                  'ghost',
                  'xs',
                  'chat-message-details-button h-auto min-w-0 px-1.5 py-0.5 font-semibold'
                )}
                onclick={toggleDetails}
              >
                <Wrench class="size-3 shrink-0" />
                <span class="truncate text-xs">
                  {detailsExpanded ? 'Hide thinking and tools' : 'Show thinking and tools'}
                </span>
              </button>
            {/if}
          </div>
        {/if}

        {#if hasThinkingText && detailsExpanded}
          <div
            class="chat-message-thinking md-content mt-1 w-full min-w-0 text-xs leading-relaxed text-pretty wrap-break-word"
          >
            {@html thinkingHtml}
          </div>
        {/if}
      </div>
    </div>

    <div
      class="chat-message-meta flex min-h-5 max-w-[92%] items-center gap-1 px-0.5 text-[10px] leading-none {isUser
        ? 'justify-end'
        : isTool
          ? 'justify-center'
          : 'justify-start'}"
    >
      {#if mainText}
        <button
          type="button"
          class={messageActionButtonClass}
          aria-label="Copy message"
          title="Copy message"
          onclick={copyMessage}
        >
          {#if copied}
            <Check class="size-3.5" />
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
          onclick={() => onToggleQuickPrompt?.(mainText)}
        >
          {#if quickPromptActive}
            <Check class="size-3.5" />
          {:else}
            <Plus class="size-3.5" />
          {/if}
        </button>
      {/if}
      {#if !isUser}
        {#if mainText}
          <button
            type="button"
            class={messageActionButtonClass}
            aria-label="Copy rich text"
            title="Copy rich text"
            onclick={copyRichMessage}
          >
            {#if richCopied}
              <Check class="size-3.5" />
            {:else}
              <Clipboard class="size-3.5" />
            {/if}
          </button>
        {/if}
        {#if hasMainText || hasAttachments}
          <button
            type="button"
            class={messageActionButtonClass}
            aria-label="Print message"
            title="Print message"
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
            onclick={() => andaClient.toggleBookmark(message)}
          >
            {#if bookmarked}
              <BookmarkCheck class="size-3.5" />
            {:else}
              <Bookmark class="size-3.5" />
            {/if}
          </button>
        {/if}
      {/if}
      {#if messageTimeLabel}
        <span class="chat-message-time px-1">{messageTimeLabel}</span>
      {/if}
    </div>
  {/if}

  {#if hasThinkingText && !hasMainText && detailsExpanded}
    <div
      class={cardClass(
        'chat-message-thinking-only relative max-w-[92%] min-w-0 gap-0 rounded-lg border-dashed py-0 text-xs leading-relaxed shadow-2xs'
      )}
    >
      <div class={cardContentClass('px-3 py-2')}>
        <div class="md-content w-full min-w-0 text-pretty wrap-break-word opacity-80">
          {@html thinkingHtml}
        </div>
        {#if messageTimeLabel}
          <div class="chat-message-time mt-1 text-right text-[10px]">
            {messageTimeLabel}
          </div>
        {/if}
      </div>
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

  .chat-message-card-tool,
  .chat-message-thinking-only {
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

  .chat-message-muted-button,
  .chat-message-action,
  .chat-message-details-button {
    color: var(--message-muted, #737373);
  }

  .chat-message-muted-button {
    border-color: var(--message-border, #e6e6e6);
    background: color-mix(in srgb, var(--message-bg, #ffffff) 76%, var(--message-surface, #f7f7f7));
  }

  .chat-message-muted-button:hover,
  .chat-message-action:hover,
  .chat-message-details-button:hover {
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

  .chat-action-approve {
    background: #047857;
    color: #ffffff;
  }

  .chat-action-approve:hover {
    background: #065f46;
  }

  .chat-action-deny {
    border-color: color-mix(in srgb, var(--message-border, #e6e6e6) 70%, #991b1b);
    color: #991b1b;
  }

  .chat-action-error {
    background: color-mix(in srgb, var(--message-bg, #ffffff) 70%, #fee2e2);
    color: #991b1b;
  }

  .chat-action-choice-button {
    border-color: color-mix(in srgb, var(--message-border, #e6e6e6) 82%, #047857);
  }

  .chat-message-detail-divider {
    border-color: var(--message-border, #e6e6e6);
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

  :global(.dark) .chat-action-approve {
    background: #059669;
  }

  :global(.dark) .chat-action-approve:hover {
    background: #047857;
  }

  :global(.dark) .chat-action-deny {
    color: #fca5a5;
    border-color: rgba(248, 113, 113, 0.28);
  }

  :global(.dark) .chat-action-error {
    background: rgba(127, 29, 29, 0.35);
    color: #fecaca;
  }
</style>
