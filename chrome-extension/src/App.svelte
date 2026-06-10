<script lang="ts">
  import ChatChannelsSidebar from '$lib/anda/ChatChannelsSidebar.svelte'
  import ChatComposer, {
    type ComposerSubmitPayload,
    type ComposerVoicePayload
  } from '$lib/anda/ChatComposer.svelte'
  import ChatMessageItem from '$lib/anda/ChatMessageItem.svelte'
  import ChatSettings from '$lib/anda/ChatSettings.svelte'
  import { andaClient } from '$lib/anda/client/side-panel.svelte'
  import {
    type ChatMessage,
    type MessageGroup,
    type PageAudioResult,
    type PromptSkill
  } from '$lib/anda/client/types'
  import { applyAppearanceTheme } from '$lib/anda/theme'
  import { isImmediatePromptCommand, parsePromptCommand } from '$lib/anda/client/commands'
  import { badgeClass, buttonClass, cardClass, separatorClass } from '$lib/anda/ui'
  import { scrollIntoView } from '$lib/utils/document'
  import {
    Bot,
    ChevronDown,
    ChevronUp,
    CircleAlert,
    Download,
    History,
    LoaderCircle,
    Radio,
    RefreshCw,
    Settings
  } from '@lucide/svelte'
  import { onMount, tick } from 'svelte'

  let settingsOpen = $state(false)
  let setupGuideOpen = $state(false)
  let sideMessagesOpen = $state(false)
  let messagesElement: HTMLElement | null = null
  let sideMessagesElement: HTMLElement | null = $state(null)
  let observedSideMessageCount = 0
  let updateRestarting = $state(false)

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
  const pendingFollowUps = $derived(andaClient.activeChannel?.pendingFollowUps || [])
  const sideMessageCount = $derived(sideMessages.length)
  const visibleSideMessages = $derived.by<ChatMessage[]>(() => displaySideMessages(sideMessages))
  const channels = $derived(andaClient.channelList)
  const activeSource = $derived(andaClient.activeSource)
  const updateState = $derived(andaClient.updateState)
  const updateReady = $derived(
    updateState?.status === 'downloaded' && Boolean(updateState.latest_tag)
  )

  $effect(() => applyAppearanceTheme(andaClient.settings.appearanceTheme))

  onMount(() => {
    andaClient
      .init()
      .then(() => {
        if (!andaClient.settings.token) {
          settingsOpen = true
          setupGuideOpen = true
        }
      })
      .catch((error) => {
        andaClient.status = 'extension unavailable'
        settingsOpen = true
        setupGuideOpen = true
        console.error('Failed to initialize Anda client', error)
      })

    return () => {
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

  function cancelPendingFollowUp(id: string) {
    andaClient.cancelPendingFollowUp(id)
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

  async function installUpdateAndRestart() {
    if (updateRestarting) {
      return
    }
    const latest = updateState?.latest_tag || ''
    if (!window.confirm(chrome.i18n.getMessage('updateRestartConfirm', [latest]))) {
      return
    }
    updateRestarting = true
    try {
      await andaClient.installUpdateAndRestart()
    } finally {
      updateRestarting = false
    }
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
      return group.current ? chrome.i18n.getMessage('currentSession') : 'Conversation'
    }
    const date = new Date(time)
    if (Number.isNaN(date.getTime())) {
      return group.current ? chrome.i18n.getMessage('currentSession') : 'Conversation'
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

      <button
        type="button"
        class={buttonClass('ghost', 'icon')}
        aria-label={chrome.i18n.getMessage('settings')}
        title={chrome.i18n.getMessage('settings')}
        onclick={toggleSettingsPanel}
      >
        <Settings class="size-4" />
      </button>
    </header>

    {#if settingsOpen}
      <ChatSettings bind:open={settingsOpen} bind:setupGuideOpen />
    {/if}

    {#if updateReady}
      <section class="border-b border-amber-900/15 bg-amber-50 px-3 py-2 text-stone-900">
        <div class="flex min-w-0 items-center gap-2">
          <span
            class="grid size-8 shrink-0 place-items-center rounded-md border border-amber-900/10 bg-white/85 text-amber-800 shadow-sm"
          >
            <Download class="size-4" />
          </span>
          <div class="min-w-0 flex-1">
            <p class="truncate text-xs font-bold text-stone-800">
              {chrome.i18n.getMessage('updateReadyTitle')}
            </p>
            <p class="truncate text-xs text-stone-600">
              {chrome.i18n.getMessage('updateReadyBody', [updateState?.latest_tag || ''])}
            </p>
          </div>
          <button
            type="button"
            class={buttonClass('outline', 'xs', 'max-w-32 bg-white/85 text-stone-700 shadow-sm')}
            disabled={updateRestarting}
            onclick={installUpdateAndRestart}
          >
            {#if updateRestarting}
              <LoaderCircle class="size-3 animate-spin" />
            {:else}
              <RefreshCw class="size-3" />
            {/if}
            <span class="truncate">{chrome.i18n.getMessage('installRestartUpdate')}</span>
          </button>
        </div>
      </section>
    {/if}

    <main
      bind:this={messagesElement}
      class="message-scroll scrollbar-slim flex min-h-0 w-full flex-1 flex-col gap-3 overflow-y-auto px-3 py-4"
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
            {syncing ? chrome.i18n.getMessage('syncing') : chrome.i18n.getMessage('ready')}
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
              {chrome.i18n.getMessage('loadHistory')}
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
              <ChatMessageItem {message} />
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
          aria-label={chrome.i18n.getMessage(
            sideMessagesOpen ? 'collapseSideTasks' : 'expandSideTasks'
          )}
          title={chrome.i18n.getMessage(sideMessagesOpen ? 'collapseSideTasks' : 'expandSideTasks')}
          onclick={toggleSideMessagesPanel}
        >
          <span
            class="message-side-icon grid size-6 shrink-0 place-items-center rounded-md border text-emerald-800 shadow-sm"
          >
            <Bot class="size-3.5" />
          </span>
          <span class="message-side-title min-w-0 flex-1 truncate text-xs font-bold">
            {chrome.i18n.getMessage('sideTasksLabel')}
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
                <ChatMessageItem {message} />
              {/each}
            </div>
          </div>
        {/if}
      </section>
    {/if}

    <footer class="message-footer border-t p-2.5 backdrop-blur">
      <ChatComposer
        placeholder={andaClient.settings.token
          ? chrome.i18n.getMessage('placeholderMessage')
          : chrome.i18n.getMessage('placeholderSettings')}
        {sending}
        working={isBusy}
        {stoppable}
        {pendingFollowUps}
        voiceAvailable={andaClient.voiceCapabilities.transcription.length > 0}
        voiceCapabilities={andaClient.voiceCapabilities}
        submitKeyMode={andaClient.settings.submitKeyMode}
        onSend={sendPrompt}
        onCancelFollowUp={cancelPendingFollowUp}
        onStop={stopActiveTask}
        onVoiceSend={sendVoiceTurn}
        onBrowserSpeechStart={startBrowserSpeechRecognition}
        onBrowserSpeechStop={stopBrowserSpeechRecognition}
        onBrowserSpeechCancel={cancelBrowserSpeechRecognition}
        onBrowserAudioStart={startBrowserAudioCapture}
        onBrowserAudioStop={stopBrowserAudioCapture}
        onBrowserAudioCancel={cancelBrowserAudioCapture}
        onLoadSkills={loadPromptSkills}
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
