<script lang="ts">
  import ChatGptUsage from '$lib/anda/chatgpt/ChatGptUsage.svelte'
  import { getMessage } from '$lib/i18n'
  import { connectionKey } from '$lib/service-worker/settings'
  import ChatChannelsSidebar from '$lib/anda/ChatChannelsSidebar.svelte'
  import ChatComposer, {
    type ComposerSubmitPayload,
    type ComposerVoicePayload
  } from '$lib/anda/ChatComposer.svelte'
  import ChatMessageItem from '$lib/anda/ChatMessageItem.svelte'
  import ChatWorkingIndicator from '$lib/anda/ChatWorkingIndicator.svelte'
  import AndaMark from '$lib/anda/AndaMark.svelte'
  import ActionDock from '$lib/anda/ActionDock.svelte'
  import { actionPending } from '$lib/anda/chat/action-view'
  import { displayMessages } from '$lib/anda/chat/message-display'
  import { runningToolLabel } from '$lib/anda/chat/tool-view'
  import { ConversationMemoryActivity } from '$lib/anda/memory/activity-store.svelte'
  import ChatSettings from '$lib/anda/ChatSettings.svelte'
  import { andaClient } from '$lib/anda/client/side-panel.svelte'
  import { provideAndaClient } from '$lib/anda/client/context'
  provideAndaClient(andaClient)
  import { type ChatAttachment, type ChatMessage, type MessageGroup } from '$lib/anda/client/types'
  import {
    isPageElementAttachmentRequest,
    pageElementAttachmentRequestStorageKey,
    pageElementInfoToAttachment,
    type PageElementAttachmentRequest
  } from '$lib/anda/page-element'
  import {
    bookmarkJumpRequestMaxAgeMs,
    bookmarkJumpRequestStorageKey,
    isBookmarkJumpRequest,
    type BookmarkJumpRequest,
    type MessageLocation
  } from '$lib/anda/bookmark-jump'
  import {
    isPromptDraftRequest,
    promptDraftRequestMaxAgeMs,
    promptDraftRequestStorageKey,
    type PromptDraftRequest
  } from '$lib/anda/prompt-draft'
  import { statusLabel } from '$lib/anda/chat/status'
  import { applyAppearanceTheme } from '$lib/anda/theme'
  import { isImmediatePromptCommand, parsePromptCommand } from '$lib/anda/client/commands'
  import { badgeClass, buttonClass, separatorClass } from '$lib/anda/ui'
  import { scrollIntoView } from '$lib/utils/document'
  import { formatTimestamp } from '$lib/utils/format'
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

  const memoryActivity = new ConversationMemoryActivity()
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
  // The running turn's rows animate; the newest group can be an empty placeholder.
  const liveGroup = $derived(
    stoppable ? visibleMessageGroups.findLast((group) => group.messages.length)?._id : undefined
  )
  const currentStep = $derived(
    stoppable ? runningToolLabel(visibleMessageGroups.at(-1)?.messages || []) : ''
  )
  const sideMessages = $derived(andaClient.activeChannel?.sideMessages || [])
  const sideMessageCount = $derived(sideMessages.length)
  const visibleSideMessages = $derived.by<ChatMessage[]>(() => displaySideMessages(sideMessages))
  const pendingActions = $derived(
    [
      ...(andaClient.activeChannel?.messageGroups || []).flatMap((group) => group.messages),
      ...sideMessages
    ].flatMap((message) => (message.actions || []).filter(actionPending))
  )
  const channels = $derived(andaClient.channelList)
  const activeSource = $derived(andaClient.activeSource)

  $effect(() => {
    const id = andaClient.activeChannel?.conversationId || 0
    memoryActivity.configure(
      andaClient.settings,
      Number.isSafeInteger(id) && id > 0 ? String(id) : '',
      sending || status === 'working' || status === 'submitted'
    )
  })
  onMount(() => {
    const changed = () => memoryActivity.visibilityChanged()
    document.addEventListener('visibilitychange', changed)
    return () => {
      memoryActivity.stop()
      document.removeEventListener('visibilitychange', changed)
    }
  })
  $effect(() => applyAppearanceTheme(andaClient.settings.appearanceTheme))

  let bookmarkConversationKey = ''
  $effect(() => {
    const conversations = visibleMessageGroups
      .map((group) => group._id)
      .filter((conversation) => conversation > 0)
    const nextKey = `${connectionKey(andaClient.settings)}:${conversations.join(',')}`
    if (conversations.length && nextKey !== bookmarkConversationKey) {
      bookmarkConversationKey = nextKey
      void andaClient.bookmarks.loadConversations(conversations)
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
    chrome.storage.onChanged.addListener(handleStorageChange)
    const handleSkillsChanged = () => {
      skillsRevision += 1
    }
    andaClient.skills.addEventListener('skills-changed', handleSkillsChanged)

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
      andaClient.skills.removeEventListener('skills-changed', handleSkillsChanged)
      andaClient.destroy()
    }
  })

  const lastMessage = $derived.by(() => {
    const lastGroup = visibleMessageGroups[visibleMessageGroups.length - 1]
    return lastGroup?.messages[lastGroup.messages.length - 1]
  })
  // A new last message is followed only when the reader has reached the end of
  // the previous one, so reading further up is never yanked to the bottom. A
  // prompt the user just sent, and a channel switch, are always followed.
  let followed = { source: '', id: '' }
  $effect(() => {
    const id = lastMessage?.id || ''
    const source = activeSource || ''
    if (!id || (id === followed.id && source === followed.source)) {
      return
    }
    const previous = followed
    followed = { source, id }
    if (source === previous.source && lastMessage?.role !== 'user' && !endInView(previous.id)) {
      return
    }
    scrollIntoView(id, 'smooth', 'start')
  })

  function endInView(messageId: string): boolean {
    const element = messageId ? document.getElementById(messageId) : null
    if (!element || !messagesElement) {
      return true
    }
    return (
      element.getBoundingClientRect().bottom <= messagesElement.getBoundingClientRect().bottom + 48
    )
  }

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

  async function jumpToBookmark(bookmark: MessageLocation) {
    if (bookmark.source && bookmark.source !== activeSource) {
      await andaClient.switchChannel(bookmark.source)
      if (andaClient.activeSource !== bookmark.source) {
        return
      }
    }

    await tick()
    if (scrollToBookmarkMessage(bookmark.message_id)) {
      return
    }

    // A verified memory source may be in an older, independent conversation.
    // Loading its exact ID only adds it to the display; sends still target the
    // channel's current conversation.
    if (Number.isSafeInteger(bookmark.conversation) && bookmark.conversation > 0) {
      try {
        if (await andaClient.activeChannel?.loadConversationForJump(bookmark.conversation)) {
          await tick()
          if (scrollToBookmarkMessage(bookmark.message_id)) return
        }
      } catch {
        // The existing ancestor walk can still locate a message if the direct
        // read is temporarily unavailable.
      }
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

  const toggleQuickPrompt = (text: string) => andaClient.quickPrompts.toggle(text)

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
    if (sending) return
    if (!andaClient.settings.token) {
      settingsOpen = true
    }
    await andaClient.sendVoiceTurn(payload)
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
    return (
      formatTimestamp(time, 'dateTime') ||
      (group.current ? getMessage('currentSession') : `#${group._id}`)
    )
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
    onSelect={(source) => andaClient.switchChannel(source)}
    onOpenFolder={openFolderChannel}
    onDelete={(source) => andaClient.deleteChannel(source)}
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
          <span class="truncate">{statusLabel(status)}</span>
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
          <div class="message-empty-mark">
            <span class="message-empty-aura" aria-hidden="true"></span>
            <AndaMark track working={syncing} class="message-empty-panda" />
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
          <!-- Keeps a reading width when the chat fills a wide tab; the side panel is narrower. -->
          <section class="mx-auto grid w-full max-w-3xl gap-4">
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
                  {statusLabel(group.status)}
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
                memoryActivity={memoryActivity.messages[message.id]}
                quickPromptActive={andaClient.quickPrompts.has(message.text)}
                onToggleQuickPrompt={toggleQuickPrompt}
                live={group._id === liveGroup}
              />
            {/each}
          </section>
        {/each}
        {#if stoppable}
          <div class="mx-auto w-full max-w-3xl">
            <ChatWorkingIndicator label={getMessage('working')} step={currentStep} />
          </div>
        {/if}
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
                  quickPromptActive={andaClient.quickPrompts.has(message.text)}
                  onToggleQuickPrompt={toggleQuickPrompt}
                />
              {/each}
            </div>
          </div>
        {/if}
      </section>
    {/if}

    <footer class="message-footer border-t p-2.5 backdrop-blur">
      <ChatGptUsage model={andaClient.modelState.activeModel} />
      <ActionDock
        pending={pendingActions}
        onReply={(text) => sendPrompt({ text, attachments: [] })}
      />
      <ChatComposer
        placeholder={andaClient.settings.token
          ? getMessage('placeholderMessage')
          : getMessage('placeholderSettings')}
        {sending}
        working={isBusy}
        {stoppable}
        voiceAvailable={andaClient.voice.capabilities.transcription.length > 0}
        voiceCapabilities={andaClient.voice.capabilities}
        approvalMode={andaClient.settings.approvalMode || 'on_risk'}
        onApprovalModeChange={(mode) => andaClient.saveApprovalMode(mode)}
        submitKeyMode={andaClient.settings.submitKeyMode}
        onSend={sendPrompt}
        onStop={stopActiveTask}
        onVoiceSend={sendVoiceTurn}
        onBrowserSpeechStart={(language) => andaClient.voice.startSpeechRecognition(language)}
        onBrowserSpeechStop={() => andaClient.voice.stopSpeechRecognition()}
        onBrowserSpeechCancel={() => andaClient.voice.cancelSpeechRecognition()}
        onBrowserAudioStart={(mimeType) => andaClient.voice.startAudioCapture(mimeType)}
        onBrowserAudioStop={() => andaClient.voice.stopAudioCapture()}
        onBrowserAudioCancel={() => andaClient.voice.cancelAudioCapture()}
        onLoadSkills={() => andaClient.skills.listPrompts()}
        mcpResources={andaClient.settings.token ? andaClient.mcp : undefined}
        {skillsRevision}
        quickPrompts={andaClient.quickPrompts.items}
        incomingAttachment={pageElementComposerAttachment}
        incomingDraft={promptDraftRequest}
        onUseQuickPrompt={(prompt) => andaClient.quickPrompts.use(prompt.text)}
        onRemoveQuickPrompt={(prompt) => andaClient.quickPrompts.remove(prompt.text)}
        onClearQuickPrompts={() => andaClient.quickPrompts.clear()}
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
    --chat-accent: #10b981;

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
    /* Text fades out under the header and into the footer instead of being cut. */
    mask-image: linear-gradient(
      to bottom,
      transparent,
      #000 12px,
      #000 calc(100% - 16px),
      transparent
    );
  }

  /* Anda waits on an empty chat: its eyes follow the pointer, a tap rolls it. */
  .message-empty-mark {
    position: relative;
    margin-bottom: 0.25rem;
    animation:
      message-empty-rise 700ms var(--anda-spring-soft) both,
      message-empty-float 6s ease-in-out 700ms infinite;
  }

  .message-empty-mark :global(.message-empty-panda) {
    position: relative;
    width: 3.25rem;
    height: auto;
  }

  .message-empty-aura {
    position: absolute;
    inset: -45% -60%;
    border-radius: 50%;
    background: conic-gradient(
      from var(--anda-comet-angle),
      #10b98166,
      #3b82f644,
      #f59e0b44,
      #10b98166
    );
    filter: blur(22px);
    opacity: 0.45;
    pointer-events: none;
    animation: message-empty-aura 14s linear infinite;
  }

  :global(.dark) .message-empty-aura {
    opacity: 0.3;
  }

  @keyframes message-empty-rise {
    from {
      opacity: 0;
      transform: translateY(10px);
      filter: blur(4px);
    }
  }

  @keyframes message-empty-float {
    50% {
      translate: 0 -4px;
    }
  }

  @keyframes message-empty-aura {
    to {
      --anda-comet-angle: 360deg;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .message-empty-mark,
    .message-empty-aura {
      animation: none;
    }
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
    --chat-accent: #34d399;

    color-scheme: dark;
  }
</style>
