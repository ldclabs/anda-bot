<script lang="ts">
  import { getMessage } from '$lib/i18n'
  import ChatChannelsSidebar from '$lib/anda/ChatChannelsSidebar.svelte'
  import ChatComposer, {
    type ComposerSubmitPayload,
    type ComposerVoicePayload
  } from '$lib/anda/ChatComposer.svelte'
  import ChatMessageItem from '$lib/anda/ChatMessageItem.svelte'
  import ChatSettings from '$lib/anda/ChatSettings.svelte'
  import { andaClient } from '$lib/anda/client/side-panel.svelte'
  import {
    type ApprovalMode,
    type BookmarkedMessage,
    type ChatAttachment,
    type ChatMessage,
    type MessageGroup,
    type PageAudioResult,
    type PromptSkill
  } from '$lib/anda/client/types'
  import {
    isPageElementAttachmentRequest,
    pageElementAttachmentMessageType,
    pageElementAttachmentRequestStorageKey,
    pageElementInfoToAttachment,
    type PageElementAttachmentRequest
  } from '$lib/anda/page-element'
  import {
    bookmarkJumpRequestMaxAgeMs,
    bookmarkJumpRequestStorageKey,
    isBookmarkJumpRequest,
    type BookmarkJumpRequest
  } from '$lib/anda/bookmark-jump'
  import {
    isPromptDraftRequest,
    promptDraftRequestMaxAgeMs,
    promptDraftRequestStorageKey,
    type PromptDraftRequest
  } from '$lib/anda/prompt-draft'
  import { applyAppearanceTheme } from '$lib/anda/theme'
  import { isImmediatePromptCommand, parsePromptCommand } from '$lib/anda/client/commands'
  import { badgeClass, buttonClass, cardClass, separatorClass } from '$lib/anda/ui'
  import { scrollIntoView } from '$lib/utils/document'
  import {
    Bot,
    ChevronDown,
    ChevronUp,
    CircleAlert,
    History,
    LayoutDashboard,
    LoaderCircle,
    Radio,
    Settings
  } from '@lucide/svelte'
  import { onMount, tick } from 'svelte'

  let settingsOpen = $state(false)
  let setupGuideOpen = $state(false)
  let sideMessagesOpen = $state(false)
  let messagesElement: HTMLElement | null = null
  let sideMessagesElement: HTMLElement | null = $state(null)
  let observedSideMessageCount = 0
  let lastBookmarkJumpRequestId = ''
  let sidePanelReadyForBookmarkJumps = false
  let queuedBookmarkJumpRequest: BookmarkJumpRequest | null = null
  let lastPageElementRequestId = ''
  let sidePanelReadyForPageElements = false
  let queuedPageElementRequest: PageElementAttachmentRequest | null = null
  let pageElementComposerAttachment: ChatAttachment | null = $state(null)
  let promptDraftRequest: PromptDraftRequest | null = $state(null)
  let lastPromptDraftRequestId = ''
  let skillsRevision = $state(0)

  const status = $derived(andaClient.status)
  const syncing = $derived(andaClient.activeChannel?.syncing || false)
  const sending = $derived(andaClient.sending || andaClient.activeChannel?.sending || false)
  const stoppable = $derived(
    sending || ['sending', 'submitted', 'working'].includes(andaClient.status)
  )
  const isBusy = $derived(
    sending ||
      syncing ||
      ['sending', 'submitted', 'working', 'connecting', 'reconnecting'].includes(andaClient.status)
  )
  const statusIsWarning = $derived(
    andaClient.status.includes('failed') || andaClient.systemMessage?.kind === 'error'
  )
  const hasPreviousConversations = $derived(
    andaClient.activeChannel?.hasPreviousConversations || false
  )
  const loadingPrevious = $derived(andaClient.activeChannel?.loadingPrevious || false)
  const visibleMessageGroups = $derived.by<MessageGroup[]>(() =>
    displayMessageGroups(andaClient.activeChannel?.messageGroups || [])
  )
  const sideMessages = $derived(andaClient.activeChannel?.sideMessages || [])
  const sideMessageCount = $derived(sideMessages.length)
  const visibleSideMessages = $derived.by<ChatMessage[]>(() => displaySideMessages(sideMessages))
  const channels = $derived(andaClient.channelList)
  const activeSource = $derived(andaClient.activeSource)

  $effect(() => applyAppearanceTheme(andaClient.settings.appearanceTheme))

  let bookmarkConversationKey = $state('')
  $effect(() => {
    const conversations = visibleMessageGroups
      .map((group) => group._id)
      .filter((conversation) => conversation > 0)
    const nextKey = conversations.join(',')
    if (nextKey && nextKey !== bookmarkConversationKey) {
      bookmarkConversationKey = nextKey
      void andaClient.loadConversationBookmarks(conversations)
    }
  })

  onMount(() => {
    const handleStorageChange = (
      changes: Record<string, { newValue?: unknown }>,
      areaName: string
    ) => {
      if (areaName === 'local') {
        consumeBookmarkJumpRequest(changes[bookmarkJumpRequestStorageKey]?.newValue)
        consumePromptDraftRequest(changes[promptDraftRequestStorageKey]?.newValue)
      }
      if (areaName === 'session') {
        consumePageElementAttachmentRequest(
          changes[pageElementAttachmentRequestStorageKey]?.newValue
        )
      }
    }
    const handleRuntimeMessage = (
      message: unknown,
      _sender: unknown,
      sendResponse: (response?: unknown) => void
    ) => {
      const requestMessage =
        message && typeof message === 'object'
          ? (message as { type?: string; pageElementRequest?: unknown })
          : null
      if (requestMessage?.type !== pageElementAttachmentMessageType) {
        return false
      }
      consumePageElementAttachmentRequest(requestMessage.pageElementRequest)
      sendResponse({ ok: true })
      return false
    }

    chrome.storage.onChanged.addListener(handleStorageChange)
    chrome.runtime.onMessage.addListener(handleRuntimeMessage)
    const handleSkillsChanged = () => {
      skillsRevision += 1
    }
    andaClient.addEventListener('skills-changed', handleSkillsChanged)

    andaClient
      .init()
      .then(() => {
        sidePanelReadyForBookmarkJumps = true
        sidePanelReadyForPageElements = true
        if (!andaClient.settings.token) {
          settingsOpen = true
          setupGuideOpen = true
        }
        flushQueuedBookmarkJumpRequest()
        flushQueuedPageElementRequest()
        void chrome.storage.local
          .get([bookmarkJumpRequestStorageKey])
          .then((stored) => consumeBookmarkJumpRequest(stored[bookmarkJumpRequestStorageKey]))
        void chrome.storage.local
          .get([promptDraftRequestStorageKey])
          .then((stored) => consumePromptDraftRequest(stored[promptDraftRequestStorageKey]))
        void chrome.storage.session
          ?.get([pageElementAttachmentRequestStorageKey])
          .then((stored) =>
            consumePageElementAttachmentRequest(stored[pageElementAttachmentRequestStorageKey])
          )
      })
      .catch((error) => {
        andaClient.status = 'extension unavailable'
        settingsOpen = true
        setupGuideOpen = true
        console.error('Failed to initialize Anda client', error)
      })

    return () => {
      chrome.storage.onChanged.removeListener(handleStorageChange)
      chrome.runtime.onMessage.removeListener(handleRuntimeMessage)
      andaClient.removeEventListener('skills-changed', handleSkillsChanged)
      andaClient.destroy()
    }
  })

  let prevLastMessageId = $state('')
  const lastMessageId = $derived.by(() => {
    const lastGroup = visibleMessageGroups[visibleMessageGroups.length - 1]
    const lastMessage = lastGroup?.messages[lastGroup.messages.length - 1]
    return lastMessage?.id || ''
  })
  $effect(() => {
    if (lastMessageId && prevLastMessageId !== lastMessageId) {
      prevLastMessageId = lastMessageId
      scrollIntoView(lastMessageId, 'smooth', 'start')
    }
  })

  $effect(() => {
    if (sideMessageCount > observedSideMessageCount) {
      sideMessagesOpen = true
      tick().then(scrollSideMessagesToEnd)
    }
    observedSideMessageCount = sideMessageCount
  })

  async function loadPreviousConversations() {
    if (loadingPrevious || !hasPreviousConversations) {
      return
    }
    const beforeHeight = messagesElement?.scrollHeight || 0
    const beforeTop = messagesElement?.scrollTop || 0
    try {
      const loaded = await andaClient.activeChannel?.loadPreviousConversations()
      await tick()
      if (loaded && messagesElement) {
        messagesElement.scrollTop = messagesElement.scrollHeight - beforeHeight + beforeTop
      }
    } catch (error) {
      console.error('Failed to load previous conversations', error)
    }
  }

  function toggleSettingsPanel() {
    settingsOpen = !settingsOpen
    if (settingsOpen) {
      setupGuideOpen = !andaClient.settings.token.trim()
    }
  }

  function openDashboardPage() {
    const url = new URL('dashboard.html#brain', window.location.href).toString()
    chrome.tabs.create({ url, active: true }).catch(() => {
      window.open(url, '_blank', 'noopener,noreferrer')
    })
  }

  function toggleSideMessagesPanel() {
    sideMessagesOpen = !sideMessagesOpen
    if (sideMessagesOpen) {
      tick().then(scrollSideMessagesToEnd)
    }
  }

  function scrollSideMessagesToEnd() {
    if (sideMessagesElement) {
      sideMessagesElement.scrollTop = sideMessagesElement.scrollHeight
    }
  }

  async function switchChannel(source: string) {
    await andaClient.switchChannel(source)
  }

  function consumeBookmarkJumpRequest(value: unknown) {
    if (!isBookmarkJumpRequest(value) || value.id === lastBookmarkJumpRequestId) {
      return
    }

    if (Date.now() - value.createdAt > bookmarkJumpRequestMaxAgeMs) {
      void chrome.storage.local.remove(bookmarkJumpRequestStorageKey)
      return
    }

    lastBookmarkJumpRequestId = value.id
    void chrome.storage.local.remove(bookmarkJumpRequestStorageKey)

    if (!sidePanelReadyForBookmarkJumps) {
      queuedBookmarkJumpRequest = value
      return
    }

    void jumpToBookmark(value.bookmark)
  }

  function flushQueuedBookmarkJumpRequest() {
    const request = queuedBookmarkJumpRequest
    queuedBookmarkJumpRequest = null
    if (request) {
      void jumpToBookmark(request.bookmark)
    }
  }

  function consumePageElementAttachmentRequest(value: unknown) {
    if (!isPageElementAttachmentRequest(value) || value.id === lastPageElementRequestId) {
      return
    }

    lastPageElementRequestId = value.id
    void chrome.storage.session?.remove(pageElementAttachmentRequestStorageKey)

    if (!sidePanelReadyForPageElements) {
      queuedPageElementRequest = value
      return
    }

    attachPageElementRequest(value)
  }

  function flushQueuedPageElementRequest() {
    const request = queuedPageElementRequest
    queuedPageElementRequest = null
    if (request) {
      attachPageElementRequest(request)
    }
  }

  function consumePromptDraftRequest(value: unknown) {
    if (!isPromptDraftRequest(value) || value.id === lastPromptDraftRequestId) {
      return
    }

    if (Date.now() - value.createdAt > promptDraftRequestMaxAgeMs) {
      void chrome.storage.local.remove(promptDraftRequestStorageKey)
      return
    }

    lastPromptDraftRequestId = value.id
    promptDraftRequest = value
    void chrome.storage.local.remove(promptDraftRequestStorageKey)
  }

  function attachPageElementRequest(request: PageElementAttachmentRequest) {
    pageElementComposerAttachment = pageElementInfoToAttachment(request)
    andaClient.systemMessage = {
      kind: 'info',
      text: getMessage('pageElementAttached') || 'Content attached to the message.'
    }
  }

  async function jumpToBookmark(bookmark: BookmarkedMessage) {
    if (bookmark.source && bookmark.source !== activeSource) {
      await switchChannel(bookmark.source)
    }

    await tick()
    if (scrollToBookmarkMessage(bookmark.message_id)) {
      return
    }

    for (let attempt = 0; attempt < 12; attempt += 1) {
      if (!andaClient.activeChannel?.hasPreviousConversations) {
        break
      }
      const loaded = await andaClient.activeChannel.loadPreviousConversations()
      await tick()
      if (scrollToBookmarkMessage(bookmark.message_id) || !loaded) {
        return
      }
    }

    andaClient.systemMessage = {
      kind: 'info',
      text: getMessage('bookmarkNotLocated')
    }
  }

  function scrollToBookmarkMessage(messageId: string): boolean {
    const element = document.getElementById(messageId)
    if (!element) {
      return false
    }
    element.classList.remove('bookmark-jump-highlight')
    void element.getBoundingClientRect()
    element.classList.add('bookmark-jump-highlight')
    window.setTimeout(() => {
      element.classList.remove('bookmark-jump-highlight')
    }, 1800)
    scrollIntoView(messageId, 'smooth', 'center')
    return true
  }

  async function openFolderChannel() {
    if (!andaClient.settings.token) {
      settingsOpen = true
      setupGuideOpen = true
    }
    await andaClient.openWorkspaceChannel()
  }

  async function deleteChannel(source: string) {
    await andaClient.deleteChannel(source)
  }

  async function toggleQuickPrompt(text: string) {
    await andaClient.toggleQuickPrompt(text)
  }

  async function useQuickPrompt(text: string) {
    await andaClient.useQuickPrompt(text)
  }

  async function removeQuickPrompt(text: string) {
    await andaClient.removeQuickPrompt(text)
  }

  async function clearQuickPrompts() {
    await andaClient.clearQuickPrompts()
  }

  async function changeApprovalMode(mode: ApprovalMode) {
    await andaClient.saveApprovalMode(mode)
  }

  async function sendPrompt(payload: ComposerSubmitPayload) {
    const command = parsePromptCommand(payload.text)
    if (sending && !isImmediatePromptCommand(command)) {
      return
    }
    if (!andaClient.settings.token) {
      settingsOpen = true
    }
    await andaClient.sendPrompt(payload.text, payload.attachments)
  }

  async function stopActiveTask() {
    if (!andaClient.settings.token) {
      settingsOpen = true
      return
    }
    await andaClient.stopActiveTask()
  }

  async function sendVoiceTurn(payload: ComposerVoicePayload) {
    if (sending) {
      return
    }
    if (!andaClient.settings.token) {
      settingsOpen = true
    }
    await andaClient.sendVoiceTurn(payload)
  }

  async function loadPromptSkills(): Promise<PromptSkill[]> {
    return andaClient.listPromptSkills()
  }

  async function startBrowserSpeechRecognition(language: string) {
    await andaClient.startBrowserSpeechRecognition(language)
  }

  async function stopBrowserSpeechRecognition() {
    return andaClient.stopBrowserSpeechRecognition()
  }

  async function cancelBrowserSpeechRecognition() {
    await andaClient.cancelBrowserSpeechRecognition()
  }

  async function startBrowserAudioCapture(mimeType?: string) {
    await andaClient.startBrowserAudioCapture(mimeType)
  }

  async function stopBrowserAudioCapture(): Promise<PageAudioResult> {
    return andaClient.stopBrowserAudioCapture()
  }

  async function cancelBrowserAudioCapture() {
    await andaClient.cancelBrowserAudioCapture()
  }

  function displayMessages(sourceMessages: ChatMessage[]): ChatMessage[] {
    const compacted: ChatMessage[] = []
    let detailRun: ChatMessage[] = []

    const flushDetails = () => {
      if (!detailRun.length) {
        return
      }
      if (detailRun.length === 1) {
        const [message] = detailRun
        const detailText = thinkingOnlyText(message)
        compacted.push({
          ...message,
          id: detailRunId(message),
          text: '',
          thinkingText: detailText || message.thinkingText
        })
        detailRun = []
        return
      }

      const first = detailRun[0]
      const last = detailRun[detailRun.length - 1]
      const attachments = detailRun.flatMap((message) => message.attachments || [])
      compacted.push({
        id: detailRunId(first),
        role: 'assistant',
        text: '',
        thinkingText: detailRun.map(thinkingOnlyText).filter(Boolean).join('\n\n---\n\n'),
        timestamp: last.timestamp || first.timestamp,
        conversation: first.conversation,
        attachments: attachments.length ? attachments : undefined
      })
      detailRun = []
    }

    for (const message of sourceMessages) {
      if (thinkingOnlyText(message)) {
        detailRun = [...detailRun, message]
        continue
      }
      flushDetails()
      compacted.push(message)
    }
    flushDetails()
    return compacted
  }

  function displayMessageGroups(sourceGroups: MessageGroup[]): MessageGroup[] {
    return sourceGroups
      .map((group) => ({ ...group, messages: displayMessages(group.messages) }))
      .filter((group) => group.messages.length)
  }

  function displaySideMessages(sourceMessages: ChatMessage[]): ChatMessage[] {
    return displayMessages(
      sourceMessages.map((message, index) => ({
        ...message,
        id: `side-${index}-${message.id}`
      }))
    )
  }

  function detailRunId(message: ChatMessage): string {
    return `${message.id}-detail-run`
  }

  function thinkingOnlyText(message: ChatMessage): string {
    const mainText = message.text.trim()
    const thinkingText = (message.thinkingText || '').trim()
    if (mainText && message.role !== 'tool') {
      return ''
    }
    return [thinkingText, message.role === 'tool' ? mainText : ''].filter(Boolean).join('\n\n')
  }

  function statusIconClass() {
    if (['connected', 'ready', 'idle', 'completed'].includes(status)) {
      return 'text-emerald-700'
    }
    if (statusIsWarning) {
      return 'text-amber-700'
    }
    return 'text-stone-500'
  }

  function groupLabel(group: MessageGroup): string {
    const time = group.createdAt || group.updatedAt || group.messages[0]?.timestamp
    if (!time) {
      return group.current ? getMessage('currentSession') : 'Conversation'
    }
    const date = new Date(time)
    if (Number.isNaN(date.getTime())) {
      return group.current ? getMessage('currentSession') : 'Conversation'
    }
    return date.toLocaleString([], {
      month: 'short',
      day: 'numeric',
      hour: '2-digit',
      minute: '2-digit'
    })
  }
</script>

<svelte:head>
  <title>Anda Bot</title>
</svelte:head>

<div class="flex h-screen min-w-80 overflow-hidden bg-background text-foreground">
  <ChatChannelsSidebar
    {channels}
    {activeSource}
    {sending}
    onSelect={switchChannel}
    onOpenFolder={openFolderChannel}
    onDelete={deleteChannel}
  />

  <div class="message-panel flex min-w-0 flex-1 flex-col overflow-hidden">
    <header class="message-header grid h-12 grid-cols-[1fr_auto] items-center gap-3 border-b px-3">
      <div class="min-w-0 text-center">
        <span
          class={badgeClass(
            'secondary',
            'message-status-badge mx-auto max-w-full gap-1.5 rounded-full text-xs'
          )}
        >
          {#if isBusy}
            <LoaderCircle class="size-3 shrink-0 animate-spin text-emerald-700" />
          {:else if statusIsWarning}
            <CircleAlert class={`size-3 shrink-0 ${statusIconClass()}`} />
          {:else}
            <Radio class={`size-3 shrink-0 ${statusIconClass()}`} />
          {/if}
          <span class="truncate">{status}</span>
        </span>
        {#if andaClient.systemMessage || activeSource}
          <p class="message-active-source truncate text-xs font-bold">
            {andaClient.systemMessage?.text || activeSource}
          </p>
        {/if}
      </div>

      <div class="flex items-center gap-1">
        <button
          type="button"
          class={buttonClass('ghost', 'icon')}
          aria-label={getMessage('dashboardTitle')}
          title={getMessage('dashboardTitle')}
          onclick={openDashboardPage}
        >
          <LayoutDashboard class="size-4" />
        </button>
        <button
          type="button"
          class={buttonClass('ghost', 'icon')}
          aria-label={getMessage('settings')}
          title={getMessage('settings')}
          onclick={toggleSettingsPanel}
        >
          <Settings class="size-4" />
        </button>
      </div>
    </header>

    {#if settingsOpen}
      <ChatSettings bind:open={settingsOpen} bind:setupGuideOpen />
    {/if}

    <main
      bind:this={messagesElement}
      class="message-scroll scrollbar-slim flex min-h-0 w-full flex-1 flex-col gap-3 overflow-x-hidden overflow-y-auto px-3 py-4"
    >
      {#if !andaClient.activeChannel || andaClient.activeChannel.messageGroups.length === 0}
        <div class="message-empty m-auto grid max-w-64 place-items-center gap-2 text-center">
          <div
            class={cardClass('message-empty-icon grid size-11 place-items-center rounded-md p-0')}
          >
            {#if syncing}
              <LoaderCircle class="size-5 animate-spin text-emerald-800" />
            {:else}
              <Bot class="size-5 text-emerald-800" />
            {/if}
          </div>
          <div class="message-empty-title text-xs font-semibold">
            {syncing ? getMessage('syncing') : getMessage('ready')}
          </div>
        </div>
      {:else}
        {#if hasPreviousConversations}
          <div class="flex justify-center">
            <button
              type="button"
              class={buttonClass('outline', 'xs', 'message-muted-button shadow-sm')}
              disabled={loadingPrevious}
              onclick={loadPreviousConversations}
            >
              {#if loadingPrevious}
                <LoaderCircle class="size-3 animate-spin" />
              {:else}
                <History class="size-3" />
              {/if}
              {getMessage('loadHistory')}
            </button>
          </div>
        {/if}

        {#each visibleMessageGroups as group (group._id)}
          <section class="grid w-full gap-4">
            {#if visibleMessageGroups.length > 1}
              <div
                class="message-group-divider flex items-center justify-center gap-2 py-1 text-[10px] font-semibold"
              >
                <div
                  class={separatorClass('message-separator flex-1')}
                  data-orientation="horizontal"
                ></div>
                <span class="max-w-[70%] truncate">{groupLabel(group)}</span>
                <span
                  class={badgeClass(
                    'secondary',
                    'message-group-status rounded-full px-1.5 text-[10px]'
                  )}
                >
                  {group.status}
                </span>
                <div
                  class={separatorClass('message-separator flex-1')}
                  data-orientation="horizontal"
                ></div>
              </div>
            {/if}

            {#each group.messages as message (message.id)}
              <ChatMessageItem
                {message}
                quickPromptActive={andaClient.isQuickPrompt(message.text)}
                onToggleQuickPrompt={toggleQuickPrompt}
              />
            {/each}
          </section>
        {/each}
      {/if}
    </main>

    {#if sideMessageCount > 0}
      <section class="message-side-tasks max-h-3/4 border-t backdrop-blur">
        <button
          type="button"
          class={buttonClass(
            'ghost',
            'default',
            'message-side-toggle flex h-10 w-full gap-2 px-3 text-left transition'
          )}
          aria-expanded={sideMessagesOpen}
          aria-label={getMessage(sideMessagesOpen ? 'collapseSideTasks' : 'expandSideTasks')}
          title={getMessage(sideMessagesOpen ? 'collapseSideTasks' : 'expandSideTasks')}
          onclick={toggleSideMessagesPanel}
        >
          <span
            class="message-side-icon grid size-6 shrink-0 place-items-center rounded-md border text-emerald-800 shadow-sm"
          >
            <Bot class="size-3.5" />
          </span>
          <span class="message-side-title min-w-0 flex-1 truncate text-xs font-bold">
            {getMessage('sideTasksLabel')}
          </span>
          <span
            class={badgeClass(
              'outline',
              'message-side-count rounded-full px-1.5 text-[10px] text-emerald-800'
            )}
          >
            {sideMessageCount}
          </span>
          {#if sideMessagesOpen}
            <ChevronDown class="message-side-chevron size-4 shrink-0" />
          {:else}
            <ChevronUp class="message-side-chevron size-4 shrink-0" />
          {/if}
        </button>

        {#if sideMessagesOpen}
          <div
            bind:this={sideMessagesElement}
            class="message-side-body scrollbar-slim overflow-y-auto border-t px-3 py-3"
          >
            <div class="grid gap-2">
              {#each visibleSideMessages as message (message.id)}
                <ChatMessageItem
                  {message}
                  quickPromptActive={andaClient.isQuickPrompt(message.text)}
                  onToggleQuickPrompt={toggleQuickPrompt}
                />
              {/each}
            </div>
          </div>
        {/if}
      </section>
    {/if}

    <footer class="message-footer border-t p-2.5 backdrop-blur">
      <ChatComposer
        placeholder={andaClient.settings.token
          ? getMessage('placeholderMessage')
          : getMessage('placeholderSettings')}
        {sending}
        working={isBusy}
        {stoppable}
        voiceAvailable={andaClient.voiceCapabilities.transcription.length > 0}
        voiceCapabilities={andaClient.voiceCapabilities}
        approvalMode={andaClient.settings.approvalMode || 'on_risk'}
        onApprovalModeChange={changeApprovalMode}
        submitKeyMode={andaClient.settings.submitKeyMode}
        onSend={sendPrompt}
        onStop={stopActiveTask}
        onVoiceSend={sendVoiceTurn}
        onBrowserSpeechStart={startBrowserSpeechRecognition}
        onBrowserSpeechStop={stopBrowserSpeechRecognition}
        onBrowserSpeechCancel={cancelBrowserSpeechRecognition}
        onBrowserAudioStart={startBrowserAudioCapture}
        onBrowserAudioStop={stopBrowserAudioCapture}
        onBrowserAudioCancel={cancelBrowserAudioCapture}
        onLoadSkills={loadPromptSkills}
        {skillsRevision}
        quickPrompts={andaClient.quickPrompts}
        incomingAttachment={pageElementComposerAttachment}
        incomingDraft={promptDraftRequest}
        onUseQuickPrompt={(prompt) => useQuickPrompt(prompt.text)}
        onRemoveQuickPrompt={(prompt) => removeQuickPrompt(prompt.text)}
        onClearQuickPrompts={clearQuickPrompts}
      />
    </footer>
  </div>
</div>

<style>
  .message-panel {
    --message-bg: #ffffff;
    --message-user-bubble: #f4f4f4;
    --message-surface: #f7f7f7;
    --message-surface-strong: #f4f4f4;
    --message-surface-hover: #eeeeee;
    --message-border: #e6e6e6;
    --message-border-soft: #eeeeee;
    --message-text: #171717;
    --message-muted: #737373;
    --message-muted-soft: #a0a0a0;

    background: var(--message-bg);
    color: var(--message-text);
    color-scheme: light;
  }

  .message-header,
  .message-footer {
    border-color: var(--message-border);
    background: color-mix(in srgb, var(--message-bg) 94%, transparent);
  }

  .message-scroll {
    background: var(--message-bg);
  }

  .message-status-badge,
  .message-group-status,
  .message-side-count {
    border-color: var(--message-border);
    background: var(--message-surface-strong);
    color: var(--message-muted);
  }

  .message-active-source,
  .message-empty-title,
  .message-side-title {
    color: var(--message-text);
  }

  .message-empty,
  .message-group-divider,
  .message-side-chevron {
    color: var(--message-muted);
  }

  .message-empty-icon,
  .message-muted-button,
  .message-side-icon {
    border-color: var(--message-border);
    background: var(--message-surface-strong);
  }

  .message-muted-button {
    color: var(--message-muted);
  }

  .message-muted-button:hover,
  .message-side-toggle:hover {
    background: var(--message-surface-hover);
    color: var(--message-text);
  }

  .message-separator {
    background: var(--message-border-soft);
  }

  :global(.bookmark-jump-highlight) {
    border-radius: 0.75rem;
    animation: bookmark-jump-highlight 1800ms ease-out;
  }

  @keyframes bookmark-jump-highlight {
    0%,
    45% {
      background: color-mix(in srgb, #047857 14%, transparent);
      box-shadow: 0 0 0 3px color-mix(in srgb, #047857 18%, transparent);
    }
    100% {
      background: transparent;
      box-shadow: 0 0 0 0 transparent;
    }
  }

  .message-side-tasks {
    border-color: var(--message-border);
    background: color-mix(in srgb, var(--message-surface) 88%, transparent);
  }

  .message-side-body {
    border-color: var(--message-border);
  }

  :global(.dark) .message-panel {
    --message-bg: #2a2a2a;
    --message-user-bubble: #343434;
    --message-surface: #303030;
    --message-surface-strong: #343434;
    --message-surface-hover: #3a3a3a;
    --message-border: #424242;
    --message-border-soft: #3a3a3a;
    --message-text: #f4f4f4;
    --message-muted: #adadad;
    --message-muted-soft: #858585;

    color-scheme: dark;
  }
</style>
