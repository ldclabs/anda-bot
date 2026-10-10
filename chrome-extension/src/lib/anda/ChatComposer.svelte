<script lang="ts" module>
  import { getMessage } from '$lib/i18n'
  import type {
    ChatAttachment,
    ApprovalMode,
    PageAudioResult,
    PromptSkill,
    QuickPrompt,
    SubmitKeyMode,
    VoiceCapabilities,
    VoiceRecordingInput
  } from '$lib/anda/client/types'
  import type { PromptDraftRequest } from '$lib/anda/prompt-draft'

  export interface ComposerSubmitPayload {
    text: string
    attachments: ChatAttachment[]
  }

  interface ApprovalModeOption {
    value: ApprovalMode
    icon: 'ask' | 'auto' | 'full' | 'custom'
  }

  const approvalModeOptions: ApprovalModeOption[] = [
    {
      value: 'request_approval',
      icon: 'ask'
    },
    {
      value: 'on_risk',
      icon: 'auto'
    },
    {
      value: 'full_access',
      icon: 'full'
    },
    {
      value: 'custom',
      icon: 'custom'
    }
  ]

  export type ComposerVoicePayload = VoiceRecordingInput
</script>

<script lang="ts">
  import AttachmentList from '$lib/anda/composer/AttachmentList.svelte'
  import { fileToAttachment } from '$lib/anda/composer/attachments'
  import McpResourcePicker, {
    type McpResourceSource
  } from '$lib/anda/composer/McpResourcePicker.svelte'
  import DropdownMenu from '$lib/anda/DropdownMenu.svelte'
  import { isImmediatePromptCommand, parsePromptCommand } from '$lib/anda/client/commands'
  import {
    buildPromptCommandSuggestions,
    firstEnabledPromptCommandIndex,
    promptSkillsCacheMs,
    readPromptCommandContext,
    type PromptCommandSuggestion
  } from '$lib/anda/composer/prompt-commands'
  import PromptCommandPanel from '$lib/anda/composer/PromptCommandPanel.svelte'
  import Modal from '$lib/anda/Modal.svelte'
  import { isMacPlatform, speechRecognitionSupported } from '$lib/anda/composer/voice'
  import { VoiceRecorder } from '$lib/anda/composer/recorder.svelte'
  import VoicePanel from '$lib/anda/composer/VoicePanel.svelte'
  import {
    alertClass,
    alertDescriptionClass,
    buttonClass,
    textareaClass,
    tooltipArrowClass,
    tooltipContentClass
  } from '$lib/anda/ui'
  import {
    ArrowUp,
    Keyboard,
    Check,
    LoaderCircle,
    Mic,
    Plus,
    Settings,
    Shield,
    ShieldAlert,
    ShieldCheck,
    Square,
    Trash2,
    Volume2,
    VolumeX,
    X
  } from '@lucide/svelte'
  import { Tooltip } from 'bits-ui'
  import { onDestroy, onMount, tick, untrack, type Snippet } from 'svelte'

  let {
    disabled = false,
    connectAction,
    sending = false,
    placeholder = getMessage('placeholderMessage'),
    working = false,
    stoppable = false,
    voiceAvailable = false,
    voiceEnabled = true,
    voiceCapabilities = { transcription: [], daemonTts: [], chromeTts: false },
    onSend,
    onStop,
    onVoiceSend,
    onBrowserSpeechStart,
    onBrowserSpeechStop,
    onBrowserSpeechCancel,
    onBrowserAudioStart,
    onBrowserAudioStop,
    onBrowserAudioCancel,
    onLoadSkills,
    quickPrompts = [],
    onUseQuickPrompt,
    onRemoveQuickPrompt,
    onClearQuickPrompts,
    approvalMode = 'on_risk',
    onApprovalModeChange,
    submitKeyMode = 'enter',
    incomingAttachment = null,
    incomingDraft = null,
    initialDraft,
    onDraftChange,
    skillsRevision = 0,
    mcpResources,
    actions
  }: {
    disabled?: boolean
    connectAction?: { label: string; run: () => void }
    sending?: boolean
    placeholder?: string
    working?: boolean
    stoppable?: boolean
    voiceAvailable?: boolean
    voiceEnabled?: boolean
    voiceCapabilities?: VoiceCapabilities
    submitKeyMode?: SubmitKeyMode
    onSend: (payload: ComposerSubmitPayload) => Promise<void> | void
    onStop?: () => Promise<void> | void
    onVoiceSend?: (payload: ComposerVoicePayload) => Promise<void> | void
    onBrowserSpeechStart?: (language: string) => Promise<void>
    onBrowserSpeechStop?: () => Promise<string>
    onBrowserSpeechCancel?: () => Promise<void>
    onBrowserAudioStart?: (mimeType?: string) => Promise<void>
    onBrowserAudioStop?: () => Promise<PageAudioResult>
    onBrowserAudioCancel?: () => Promise<void>
    onLoadSkills?: () => Promise<PromptSkill[]>
    quickPrompts?: QuickPrompt[]
    onUseQuickPrompt?: (prompt: QuickPrompt) => Promise<void> | void
    onRemoveQuickPrompt?: (prompt: QuickPrompt) => Promise<void> | void
    onClearQuickPrompts?: () => Promise<void> | void
    approvalMode?: ApprovalMode
    onApprovalModeChange?: (mode: ApprovalMode) => Promise<void> | void
    incomingAttachment?: ChatAttachment | null
    incomingDraft?: PromptDraftRequest | null
    initialDraft?: ComposerSubmitPayload
    onDraftChange?: (draft: ComposerSubmitPayload) => void
    skillsRevision?: number
    /** The connected MCP servers' resources: with it, + also attaches one. */
    mcpResources?: McpResourceSource
    /** Extra controls at the head of the toolbar's right side, before the
     * voice and send buttons (the desktop puts its model picker here). */
    actions?: Snippet
  } = $props()

  let text = $state(untrack(() => initialDraft?.text || ''))
  let attachments = $state<ChatAttachment[]>(untrack(() => initialDraft?.attachments || []))
  $effect(() => {
    const draft = { text, attachments }
    untrack(() => onDraftChange?.(draft))
  })
  let attachmentError = $state('')
  let stopPending = $state(false)
  let preparingAttachments = $state(false)
  let inputMode = $state<'text' | 'voice'>('text')
  let ttsEnabled = $state(false)
  let browserSpeechAvailable = $state(speechRecognitionSupported())

  // The four capture paths (page/local x speech/audio) live in VoiceRecorder;
  // this component only renders its state and forwards the two verbs.
  const recorder = new VoiceRecorder({
    capabilities: () => voiceCapabilities,
    ttsEnabled: () => ttsEnabled,
    send: async (input) => {
      await onVoiceSend?.(input)
    },
    page: () => ({
      startSpeech: onBrowserSpeechStart,
      stopSpeech: onBrowserSpeechStop,
      cancelSpeech: onBrowserSpeechCancel,
      startAudio: onBrowserAudioStart,
      stopAudio: onBrowserAudioStop,
      cancelAudio: onBrowserAudioCancel
    })
  })
  let textareaElement: HTMLTextAreaElement | null = $state(null)
  let fileInputElement: HTMLInputElement | null = $state(null)
  let textareaFocused = $state(false)
  let caretIndex = $state(0)
  let activePromptCommandIndex = $state(0)
  let promptCommandDismissedKey = $state('')
  let promptCommandSelectionKey = $state('')
  let promptSkills = $state<PromptSkill[]>([])
  let promptSkillsLoading = $state(false)
  let promptSkillsLoadedAt = $state(0)
  let promptSkillsError = $state('')
  let approvalMenuOpen = $state(false)
  let clearQuickPromptsOpen = $state(false)
  let approvalMenuElement: HTMLDivElement | null = $state(null)
  let lastIncomingAttachmentId = ''
  let lastIncomingDraftId = ''

  const hasDraft = $derived(Boolean(text.trim()) || attachments.length > 0)
  const draftCommand = $derived(parsePromptCommand(text))
  const draftBypassesSending = $derived(isImmediatePromptCommand(draftCommand))
  const canSend = $derived(
    hasDraft && !disabled && !preparingAttachments && (!sending || draftBypassesSending)
  )
  const submitTitle = $derived(
    submitKeyMode === 'modifier-enter'
      ? isMacPlatform()
        ? getMessage('sendWithCmdEnter')
        : getMessage('sendWithCtrlEnter')
      : getMessage('sendWithEnter')
  )
  const showStopButton = $derived(Boolean(onStop) && stoppable && inputMode === 'text' && !hasDraft)
  const stopTitle = $derived(
    getMessage(stopPending ? 'stoppingTask' : 'stopTask') ||
      (stopPending ? 'Stopping current task' : 'Stop current task')
  )
  const canUseBrowserSpeech = $derived(
    voiceEnabled && (Boolean(onBrowserSpeechStart && onBrowserSpeechStop) || browserSpeechAvailable)
  )
  const canUseAndaVoice = $derived(
    voiceEnabled && (voiceAvailable || voiceCapabilities.transcription.length > 0)
  )
  const canUseSelectedVoiceProvider = $derived(
    recorder.provider === 'chrome' ? canUseBrowserSpeech : canUseAndaVoice
  )
  const selectedVoiceTtsAvailable = $derived(
    recorder.provider === 'chrome'
      ? voiceCapabilities.chromeTts
      : voiceCapabilities.daemonTts.length > 0
  )
  const canUseVoice = $derived(canUseBrowserSpeech || canUseAndaVoice)
  const canRecordVoice = $derived(
    canUseSelectedVoiceProvider &&
      !disabled &&
      !sending &&
      !preparingAttachments &&
      recorder.stage !== 'processing'
  )
  const voiceProviderLabel = $derived(
    recorder.provider === 'chrome' ? getMessage('browserVoiceProviderLabel') : 'Anda'
  )
  const voiceProviderTitle = $derived(
    recorder.provider === 'chrome' ? getMessage('useBrowserVoice') : getMessage('useAndaVoice')
  )
  const voiceStatus = $derived(
    recorder.stage === 'recording'
      ? getMessage('listening')
      : recorder.stage === 'processing' || sending
        ? getMessage('working')
        : getMessage('ready')
  )
  const voiceOrbStyle = $derived(`--voice-level: ${recorder.level.toFixed(3)}`)
  const promptCommandContext = $derived(readPromptCommandContext(text, caretIndex))
  const promptCommandSuggestions = $derived(
    buildPromptCommandSuggestions(
      promptCommandContext,
      promptSkills,
      promptSkillsLoading,
      promptSkillsError
    )
  )
  const promptCommandPanelOpen = $derived(
    textareaFocused &&
      inputMode === 'text' &&
      !disabled &&
      !sending &&
      promptCommandContext.open &&
      promptCommandContext.key !== promptCommandDismissedKey &&
      promptCommandSuggestions.length > 0
  )
  const promptCommandPanelTitle = $derived(
    promptCommandContext.mode === 'skill'
      ? getMessage('promptSkillsLabel')
      : getMessage('promptCommandsLabel')
  )
  const currentApprovalMode = $derived(
    approvalModeOptions.find((option) => option.value === approvalMode) || approvalModeOptions[1]
  )
  const CurrentApprovalIcon = $derived(approvalModeIcon(currentApprovalMode))

  let workingPersisted = $state(false)
  let workingTimeout: number | undefined

  $effect(() => {
    if (working || sending || recorder.stage === 'processing') {
      if (workingTimeout) {
        clearTimeout(workingTimeout)
        workingTimeout = undefined
      }
      workingPersisted = true
    } else if (workingPersisted) {
      workingTimeout = window.setTimeout(() => {
        workingPersisted = false
      }, 800)
    }
  })

  $effect(() => {
    if (!canUseVoice && inputMode === 'voice') {
      void recorder.cancel()
      inputMode = 'text'
    }
  })

  $effect(() => {
    const attachment = incomingAttachment
    if (!attachment || attachment.id === lastIncomingAttachmentId) {
      return
    }
    lastIncomingAttachmentId = attachment.id
    attachmentError = ''
    if (inputMode === 'voice') {
      void recorder.cancel()
      inputMode = 'text'
    }
    if (!attachments.some((item) => item.id === attachment.id)) {
      attachments = [...attachments, attachment]
    }
    void tick().then(() => {
      textareaElement?.focus()
      resizeTextarea()
    })
  })

  $effect(() => {
    const draft = incomingDraft
    if (!draft || draft.id === lastIncomingDraftId) {
      return
    }
    lastIncomingDraftId = draft.id
    text = draft.text
    attachments = []
    attachmentError = ''
    promptCommandDismissedKey = ''
    caretIndex = text.length
    if (inputMode === 'voice') {
      void recorder.cancel()
      inputMode = 'text'
    }
    void tick().then(() => {
      resizeTextarea()
      textareaElement?.focus()
      textareaElement?.setSelectionRange(text.length, text.length)
      updateTextareaCaret()
    })
  })

  $effect(() => {
    if (!promptCommandPanelOpen) {
      return
    }
    const nextSelectionKey = `${promptCommandContext.key}:${promptCommandSuggestions.map((suggestion) => suggestion.id).join('|')}`
    if (promptCommandSelectionKey !== nextSelectionKey) {
      promptCommandSelectionKey = nextSelectionKey
      activePromptCommandIndex = firstEnabledPromptCommandIndex(promptCommandSuggestions)
      return
    }
    if (activePromptCommandIndex >= promptCommandSuggestions.length) {
      activePromptCommandIndex = firstEnabledPromptCommandIndex(promptCommandSuggestions)
    }
  })

  $effect(() => {
    // A skills-changed event drops the cache so the next open refetches.
    // `ensurePromptSkillsLoaded` reads the very cache state it writes, so the
    // body stays untracked: tracking it makes this effect retrigger itself
    // after every load, which reloads forever and keeps the panel rerendering.
    skillsRevision
    untrack(() => {
      promptSkillsLoadedAt = 0
      if (promptCommandPanelOpen) {
        void ensurePromptSkillsLoaded()
      }
    })
  })

  $effect(() => {
    // Skills surface in both `/` and `$` completion, so load them whenever
    // the panel opens (cached for promptSkillsCacheMs).
    if (promptCommandPanelOpen) {
      untrack(() => void ensurePromptSkillsLoaded())
    }
  })

  $effect(() => {
    if (recorder.stage === 'idle') {
      if (!recorder.providerSelected && canUseAndaVoice && recorder.provider !== 'anda') {
        recorder.provider = 'anda'
      }
      if (recorder.provider === 'anda' && !canUseAndaVoice && canUseBrowserSpeech) {
        recorder.provider = 'chrome'
      }
      if (recorder.provider === 'chrome' && !canUseBrowserSpeech && canUseAndaVoice) {
        recorder.provider = 'anda'
      }
    }
    if (ttsEnabled && !selectedVoiceTtsAvailable) {
      ttsEnabled = false
    }
  })

  onMount(() => {
    browserSpeechAvailable = speechRecognitionSupported()
    document.addEventListener('pointerdown', handleDocumentPointerDown)
  })

  onDestroy(() => {
    clearTimeout(workingTimeout)
    document.removeEventListener('pointerdown', handleDocumentPointerDown)
    void recorder.cancel()
  })

  function handleDocumentPointerDown(event: PointerEvent) {
    if (!approvalMenuOpen) {
      return
    }
    const target = event.target
    if (target instanceof Node && approvalMenuElement?.contains(target)) {
      return
    }
    approvalMenuOpen = false
  }

  function isSubmitEvent(event: KeyboardEvent): boolean {
    if (
      disabled ||
      (sending && !draftBypassesSending) ||
      preparingAttachments ||
      event.isComposing
    ) {
      return false
    }
    if (event.keyCode === 229) {
      return false
    }
    const isEnter = event.key === 'Enter' || event.code === 'Enter' || event.keyCode === 13
    if (!isEnter) {
      return false
    }
    if (submitKeyMode === 'modifier-enter') {
      const submitModifierPressed = isMacPlatform() ? event.metaKey : event.ctrlKey
      return submitModifierPressed && !event.shiftKey && !event.altKey
    }
    return !event.shiftKey && !event.metaKey && !event.ctrlKey && !event.altKey
  }

  function movePromptCommandSelection(delta: number) {
    if (promptCommandSuggestions.length === 0) {
      return
    }
    let nextIndex = activePromptCommandIndex
    for (let step = 0; step < promptCommandSuggestions.length; step += 1) {
      nextIndex =
        (nextIndex + delta + promptCommandSuggestions.length) % promptCommandSuggestions.length
      if (!promptCommandSuggestions[nextIndex]?.disabled) {
        activePromptCommandIndex = nextIndex
        return
      }
    }
  }

  async function ensurePromptSkillsLoaded() {
    const now = Date.now()
    if (
      promptSkillsLoading ||
      (promptSkillsLoadedAt > 0 && now - promptSkillsLoadedAt < promptSkillsCacheMs)
    ) {
      return
    }

    promptSkillsLoading = true
    promptSkillsError = ''
    try {
      promptSkills = onLoadSkills ? await onLoadSkills() : []
    } catch (error) {
      promptSkills = []
      promptSkillsError = error instanceof Error ? error.message : String(error)
    } finally {
      promptSkillsLoadedAt = Date.now()
      promptSkillsLoading = false
    }
  }

  async function applyPromptCommandSuggestion(suggestion: PromptCommandSuggestion | undefined) {
    if (!suggestion || suggestion.disabled || !promptCommandContext.open) {
      return
    }

    const prefix = text.slice(0, promptCommandContext.replaceStart)
    const suffix = text.slice(promptCommandContext.replaceEnd)
    text = `${prefix}${suggestion.insertText}${suffix}`
    const nextCaret = prefix.length + suggestion.insertText.length
    promptCommandDismissedKey = ''
    await tick()
    textareaElement?.focus()
    textareaElement?.setSelectionRange(nextCaret, nextCaret)
    textareaFocused = true
    caretIndex = nextCaret
    resizeTextarea()
  }

  async function applyQuickPrompt(prompt: QuickPrompt) {
    if (disabled || inputMode !== 'text') {
      return
    }
    text = prompt.text
    const nextCaret = text.length
    promptCommandDismissedKey = ''
    caretIndex = nextCaret
    await onUseQuickPrompt?.(prompt)
    await tick()
    textareaElement?.focus()
    textareaElement?.setSelectionRange(nextCaret, nextCaret)
    textareaFocused = true
    caretIndex = nextCaret
    resizeTextarea()
  }

  async function removeQuickPrompt(prompt: QuickPrompt) {
    await onRemoveQuickPrompt?.(prompt)
  }

  async function clearQuickPrompts() {
    clearQuickPromptsOpen = false
    await onClearQuickPrompts?.()
  }

  async function selectApprovalMode(mode: ApprovalMode) {
    approvalMenuOpen = false
    if (mode === approvalMode) {
      return
    }
    await onApprovalModeChange?.(mode)
  }

  function approvalModeLabel(option: ApprovalModeOption): string {
    switch (option.value) {
      case 'request_approval':
        return getMessage('approvalModeRequestApprovalLabel')
      case 'full_access':
        return getMessage('approvalModeFullAccessLabel')
      case 'custom':
        return getMessage('approvalModeCustomLabel')
      default:
        return getMessage('approvalModeOnRiskLabel')
    }
  }

  function approvalModeDescription(option: ApprovalModeOption): string {
    switch (option.value) {
      case 'request_approval':
        return getMessage('approvalModeRequestApprovalDescription')
      case 'full_access':
        return getMessage('approvalModeFullAccessDescription')
      case 'custom':
        return getMessage('approvalModeCustomDescription')
      default:
        return getMessage('approvalModeOnRiskDescription')
    }
  }

  function approvalModeIcon(option: ApprovalModeOption) {
    switch (option.icon) {
      case 'ask':
        return ShieldAlert
      case 'full':
        return ShieldCheck
      case 'custom':
        return Settings
      default:
        return Shield
    }
  }

  function handlePromptCommandKeydown(event: KeyboardEvent): boolean {
    if (
      !promptCommandPanelOpen ||
      event.metaKey ||
      event.ctrlKey ||
      event.altKey ||
      event.isComposing
    ) {
      return false
    }

    if (event.key === 'ArrowDown') {
      event.preventDefault()
      movePromptCommandSelection(1)
      return true
    }
    if (event.key === 'ArrowUp') {
      event.preventDefault()
      movePromptCommandSelection(-1)
      return true
    }
    if ((event.key === 'Enter' && !event.shiftKey) || event.key === 'Tab') {
      event.preventDefault()
      void applyPromptCommandSuggestion(promptCommandSuggestions[activePromptCommandIndex])
      return true
    }
    if (event.key === 'Escape') {
      event.preventDefault()
      promptCommandDismissedKey = promptCommandContext.key
      return true
    }
    return false
  }

  function updateTextareaCaret() {
    if (!textareaElement) {
      return
    }
    caretIndex = textareaElement.selectionStart ?? text.length
  }

  function handleTextareaInput() {
    promptCommandDismissedKey = ''
    updateTextareaCaret()
    resizeTextarea()
  }

  function handleTextareaFocus() {
    textareaFocused = true
    updateTextareaCaret()
  }

  function handleTextareaBlur() {
    window.setTimeout(() => {
      textareaFocused = false
    }, 80)
  }

  async function submitMessage() {
    if (connectAction && !disabled) {
      connectAction.run()
      return
    }
    if (!canSend) {
      return
    }
    const payload: ComposerSubmitPayload = {
      text: text.trim(),
      attachments
    }
    // Clear optimistically: a send can take a while (e.g. /side runs inline on
    // the daemon) and the draft lingering in the box reads as "not sent".
    const draftText = text
    const draftAttachments = attachments
    text = ''
    attachments = []
    attachmentError = ''
    inputMode = 'text'
    promptCommandDismissedKey = ''
    caretIndex = 0
    await tick()
    resizeTextarea()
    try {
      await onSend(payload)
    } catch (_error) {
      // The send never reached the daemon; restore the draft for a retry
      // unless the user has already started typing something new.
      if (!text.trim() && attachments.length === 0) {
        text = draftText
        attachments = draftAttachments
        await tick()
        resizeTextarea()
      }
    }
  }

  async function stopTask() {
    if (!onStop || stopPending) {
      return
    }
    stopPending = true
    try {
      await onStop()
    } finally {
      stopPending = false
    }
  }

  function handleKeydown(event: KeyboardEvent) {
    if (event.key === 'Escape' && approvalMenuOpen) {
      approvalMenuOpen = false
      return
    }
    if (handlePromptCommandKeydown(event)) {
      return
    }
    if (isSubmitEvent(event)) {
      event.preventDefault()
      void submitMessage()
      return
    }
  }

  function resizeTextarea() {
    if (!textareaElement) {
      return
    }
    textareaElement.style.height = 'auto'
    textareaElement.style.height = `${Math.min(textareaElement.scrollHeight, 150)}px`
  }

  function openFileDialog() {
    if (disabled || preparingAttachments) {
      return
    }
    fileInputElement?.click()
  }

  async function handleFileInput(event: Event) {
    const input = event.currentTarget as HTMLInputElement
    await addFiles(input.files)
    input.value = ''
  }

  async function handleDrop(event: DragEvent) {
    event.preventDefault()
    if (disabled) {
      return
    }
    await addFiles(event.dataTransfer?.files || null)
  }

  function handleDragover(event: DragEvent) {
    if (!disabled) {
      event.preventDefault()
    }
  }

  async function handlePaste(event: ClipboardEvent) {
    if (disabled) return
    const items = event.clipboardData?.items
    if (!items) return

    const files: File[] = []
    for (let i = 0; i < items.length; i++) {
      if (items[i].kind === 'file') {
        const file = items[i].getAsFile()
        if (file) files.push(file)
      }
    }

    if (files.length > 0) {
      event.preventDefault()
      await addFiles(files)
    }
  }

  async function addFiles(fileList: FileList | File[] | null) {
    if (!fileList || fileList.length === 0) {
      return
    }
    attachmentError = ''
    preparingAttachments = true
    // A file that cannot be attached (too large, unreadable) is reported
    // without dropping the others picked with it.
    const nextAttachments: ChatAttachment[] = []
    const errors: string[] = []
    for (const file of Array.from(fileList)) {
      try {
        nextAttachments.push(await fileToAttachment(file))
      } catch (error) {
        errors.push(error instanceof Error ? error.message : String(error))
      }
    }
    const existingIds = new Set(attachments.map((attachment) => attachment.id))
    attachments = [
      ...attachments,
      ...nextAttachments.filter((attachment) => !existingIds.has(attachment.id))
    ]
    attachmentError = errors.join('\n')
    preparingAttachments = false
  }

  function removeAttachment(id: string) {
    attachments = attachments.filter((attachment) => attachment.id !== id)
  }

  let resourcePickerOpen = $state(false)
  const attachMenuItems = [
    { value: 'files', label: getMessage('attachFiles') },
    { value: 'mcp', label: getMessage('attachMcpResource') }
  ] as const

  function attachFrom(choice: 'files' | 'mcp') {
    if (choice === 'mcp') {
      attachmentError = ''
      resourcePickerOpen = true
    } else {
      openFileDialog()
    }
  }

  function addAttachments(items: ChatAttachment[]) {
    const existingIds = new Set(attachments.map((attachment) => attachment.id))
    attachments = [...attachments, ...items.filter((item) => !existingIds.has(item.id))]
  }

  function toggleInputMode() {
    if (inputMode === 'voice') {
      void recorder.cancel()
      inputMode = 'text'
      void tick().then(() => textareaElement?.focus())
      return
    }
    if (canUseVoice) {
      inputMode = 'voice'
      recorder.error = ''
    }
  }
</script>

<form
  class="contents"
  onsubmit={(event) => {
    event.preventDefault()
    void submitMessage()
  }}
  onpaste={handlePaste}
  ondrop={handleDrop}
  ondragover={handleDragover}
>
  <input
    bind:this={fileInputElement}
    type="file"
    multiple
    class="hidden"
    onchange={handleFileInput}
  />
  {#if mcpResources}
    <McpResourcePicker
      bind:open={resourcePickerOpen}
      source={mcpResources}
      onAttach={addAttachments}
    />
  {/if}

  <div
    class="composer-shell"
    class:composer-working={workingPersisted}
    aria-busy={workingPersisted}
  >
    <AttachmentList {attachments} onRemove={removeAttachment} />

    {#if attachmentError}
      <div
        role="alert"
        class={alertClass(
          'rounded-md border-amber-200 bg-amber-50 px-2 py-1 text-xs text-amber-800'
        )}
      >
        <div class={alertDescriptionClass('text-xs whitespace-pre-line text-amber-800')}>
          {attachmentError}
        </div>
      </div>
    {/if}

    {#if inputMode === 'voice'}
      <VoicePanel
        voiceStage={recorder.stage}
        {sending}
        {canRecordVoice}
        {voiceOrbStyle}
        {voiceStatus}
        voiceProvider={recorder.provider}
        {canUseBrowserSpeech}
        {canUseAndaVoice}
        voiceTranscript={recorder.transcript}
        onToggleRecording={() => recorder.toggle()}
        onSelectVoiceProvider={(provider) => recorder.selectProvider(provider)}
      />
      {#if recorder.error}
        <div
          role="alert"
          class={alertClass(
            'rounded-md border-amber-200 bg-amber-50 px-2 py-1 text-xs text-amber-800'
          )}
        >
          <div class={alertDescriptionClass('text-xs text-amber-800')}>
            {recorder.error}
          </div>
        </div>
      {/if}
    {:else}
      {#if quickPrompts.length}
        <div class="quick-prompts-row" aria-label={getMessage('quickPromptsLabel')}>
          <div class="quick-prompts-scroll">
            {#each quickPrompts as prompt (prompt.id)}
              <span class="quick-prompt-chip">
                <button
                  type="button"
                  class="quick-prompt-main"
                  aria-label={getMessage('useQuickPrompt', prompt.text)}
                  title={getMessage('useQuickPrompt', prompt.text)}
                  {disabled}
                  onclick={() => applyQuickPrompt(prompt)}
                >
                  {prompt.text}
                </button>
                <button
                  type="button"
                  class="quick-prompt-remove"
                  disabled={disabled || !onRemoveQuickPrompt}
                  aria-label={getMessage('removeQuickPromptItem', prompt.text)}
                  title={getMessage('removeQuickPromptItem', prompt.text)}
                  onclick={() => removeQuickPrompt(prompt)}
                >
                  <X class="size-3" />
                </button>
              </span>
            {/each}
          </div>
          {#if onClearQuickPrompts && quickPrompts.length > 1}
            <button
              type="button"
              class={buttonClass('ghost', 'icon-xs', 'quick-prompts-clear composer-icon-button')}
              aria-label={getMessage('clearQuickPrompts')}
              title={getMessage('clearQuickPrompts')}
              onclick={() => (clearQuickPromptsOpen = true)}
            >
              <Trash2 class="size-3" />
            </button>
          {/if}
        </div>
      {/if}
      <div class="prompt-input-wrap">
        {#if promptCommandPanelOpen}
          <PromptCommandPanel
            title={promptCommandPanelTitle}
            suggestions={promptCommandSuggestions}
            activeIndex={activePromptCommandIndex}
            onApply={applyPromptCommandSuggestion}
          />
        {/if}
        <textarea
          bind:this={textareaElement}
          bind:value={text}
          rows={1}
          {placeholder}
          spellcheck="true"
          {disabled}
          aria-haspopup="listbox"
          class={textareaClass(
            'composer-textarea max-h-38 min-h-10 resize-none rounded-none border-0 bg-transparent px-1.5 py-1.5 leading-5 shadow-none focus-visible:ring-0 disabled:opacity-60 dark:bg-transparent'
          )}
          onkeydown={handleKeydown}
          oninput={handleTextareaInput}
          onfocus={handleTextareaFocus}
          onblur={handleTextareaBlur}
          onclick={updateTextareaCaret}
          onkeyup={updateTextareaCaret}
          onselect={updateTextareaCaret}></textarea>
      </div>
    {/if}

    <div class="composer-toolbar">
      <div class="composer-toolbar-group">
        {#if mcpResources}
          <DropdownMenu
            class={buttonClass('ghost', 'icon-sm', 'composer-icon-button rounded-full')}
            items={attachMenuItems}
            onSelect={attachFrom}
            disabled={disabled || preparingAttachments}
            ariaLabel={getMessage('attachMenu')}
            title={getMessage('attachMenu')}
          >
            {#snippet trigger()}
              {#if preparingAttachments}
                <LoaderCircle class="size-4 animate-spin" />
              {:else}
                <Plus class="size-4.5" />
              {/if}
            {/snippet}
          </DropdownMenu>
        {:else}
          <button
            type="button"
            class={buttonClass('ghost', 'icon-sm', 'composer-icon-button rounded-full')}
            disabled={disabled || preparingAttachments}
            aria-label={getMessage('attachFiles')}
            title={getMessage('attachFiles')}
            onclick={openFileDialog}
          >
            {#if preparingAttachments}
              <LoaderCircle class="size-4 animate-spin" />
            {:else}
              <Plus class="size-4.5" />
            {/if}
          </button>
        {/if}

        <div bind:this={approvalMenuElement} class="approval-mode-wrap">
          <button
            type="button"
            class={buttonClass(
              approvalMenuOpen ? 'secondary' : 'ghost',
              'sm',
              'approval-mode-button composer-icon-button h-8 min-w-0 gap-1.5 rounded-full px-2.5 font-normal'
            )}
            {disabled}
            aria-haspopup="menu"
            aria-expanded={approvalMenuOpen}
            title={approvalModeDescription(currentApprovalMode)}
            onclick={() => (approvalMenuOpen = !approvalMenuOpen)}
          >
            <CurrentApprovalIcon class="size-4" />
            <span class="truncate">{approvalModeLabel(currentApprovalMode)}</span>
          </button>

          {#if approvalMenuOpen}
            <div
              class="approval-mode-menu"
              role="menu"
              aria-label={getMessage('approvalModeMenuAria')}
            >
              <div class="approval-mode-menu-eyebrow">{getMessage('approvalModeMenuPrompt')}</div>
              {#each approvalModeOptions as option (option.value)}
                {@const OptionIcon = approvalModeIcon(option)}
                <button
                  type="button"
                  class="approval-mode-item"
                  class:approval-mode-item-active={option.value === approvalMode}
                  role="menuitemradio"
                  aria-checked={option.value === approvalMode}
                  onclick={() => selectApprovalMode(option.value)}
                >
                  <OptionIcon class="approval-mode-item-icon size-4" />
                  <span class="min-w-0 flex-1">
                    <span class="block truncate text-sm font-semibold">
                      {approvalModeLabel(option)}
                    </span>
                    <span class="approval-mode-item-description block truncate text-xs">
                      {approvalModeDescription(option)}
                    </span>
                  </span>
                  {#if option.value === approvalMode}
                    <Check class="size-4" />
                  {/if}
                </button>
              {/each}
            </div>
          {/if}
        </div>
      </div>

      <div class="composer-toolbar-group">
        {@render actions?.()}

        {#if canUseVoice}
          <button
            type="button"
            class={buttonClass(
              inputMode === 'voice' ? 'secondary' : 'ghost',
              'icon-sm',
              'composer-icon-button rounded-full'
            )}
            disabled={disabled || sending}
            aria-label={inputMode === 'voice'
              ? getMessage('switchToKeyboard')
              : getMessage('switchToVoice')}
            title={inputMode === 'voice' ? getMessage('keyboardInput') : getMessage('voiceInput')}
            onclick={toggleInputMode}
          >
            {#if inputMode === 'voice'}
              <Keyboard class="size-4" />
            {:else}
              <Mic class="size-4" />
            {/if}
          </button>
        {/if}

        {#if inputMode === 'voice'}
          <button
            type="button"
            class={buttonClass(
              ttsEnabled ? 'secondary' : 'ghost',
              'icon-sm',
              'composer-icon-button rounded-full'
            )}
            disabled={disabled ||
              sending ||
              recorder.stage === 'recording' ||
              !selectedVoiceTtsAvailable}
            aria-label={ttsEnabled ? getMessage('disablePlayback') : getMessage('enablePlayback')}
            title={selectedVoiceTtsAvailable
              ? `${voiceProviderLabel} ${ttsEnabled ? getMessage('playbackOn') : getMessage('playbackOff')}`
              : `${voiceProviderLabel} ${getMessage('playbackUnavailable')}`}
            onclick={() => (ttsEnabled = !ttsEnabled)}
          >
            {#if ttsEnabled}
              <Volume2 class="size-4" />
            {:else}
              <VolumeX class="size-4" />
            {/if}
          </button>
        {:else}
          <Tooltip.Provider delayDuration={0}>
            <Tooltip.Root>
              <Tooltip.Trigger>
                {#snippet child({ props })}
                  {#if connectAction}
                    <button
                      type="button"
                      {disabled}
                      class={buttonClass('default', 'sm', 'rounded-full')}
                      onclick={connectAction.run}>{connectAction.label}</button
                    >
                  {:else if showStopButton}
                    <button
                      {...props}
                      type="button"
                      disabled={stopPending}
                      class={buttonClass('default', 'icon-sm', 'rounded-full')}
                      aria-label={stopTitle}
                      onclick={stopTask}
                    >
                      {#if stopPending}
                        <LoaderCircle class="size-4 animate-spin" />
                      {:else}
                        <Square class="size-3 fill-current" />
                      {/if}
                    </button>
                  {:else}
                    <button
                      {...props}
                      type="submit"
                      disabled={!canSend}
                      class={buttonClass('default', 'icon-sm', 'rounded-full disabled:opacity-30')}
                      aria-label={getMessage('send')}
                    >
                      {#if sending}
                        <LoaderCircle class="size-4 animate-spin" />
                      {:else}
                        <ArrowUp class="size-4" />
                      {/if}
                    </button>
                  {/if}
                {/snippet}
              </Tooltip.Trigger>
              <Tooltip.Portal>
                <Tooltip.Content side="top" sideOffset={6} class={tooltipContentClass()}>
                  {showStopButton ? stopTitle : submitTitle}
                  <Tooltip.Arrow>
                    {#snippet child({ props })}
                      <div class={tooltipArrowClass()} {...props}></div>
                    {/snippet}
                  </Tooltip.Arrow>
                </Tooltip.Content>
              </Tooltip.Portal>
            </Tooltip.Root>
          </Tooltip.Provider>
        {/if}
      </div>
    </div>
  </div>
</form>

{#snippet clearQuickPromptsActions()}
  <button
    type="button"
    class={buttonClass('outline', 'sm')}
    onclick={() => (clearQuickPromptsOpen = false)}
  >
    {getMessage('cancel')}
  </button>
  <button type="button" class={buttonClass('destructive', 'sm')} onclick={clearQuickPrompts}>
    {getMessage('clearQuickPrompts')}
  </button>
{/snippet}

<Modal
  alert
  bind:open={clearQuickPromptsOpen}
  title={getMessage('clearQuickPrompts')}
  contentClass="min-h-0 sm:max-w-sm"
  footer={clearQuickPromptsActions}
>
  <p class="text-sm leading-relaxed text-muted-foreground">
    {getMessage('clearQuickPromptsConfirm')}
  </p>
</Modal>

<style>
  /* One card: the text box on top, one toolbar row under it. */
  .composer-shell {
    position: relative;
    isolation: isolate;
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
    border: 1px solid var(--message-border, #e6e6e6);
    border-radius: 1.25rem;
    background: var(--message-bg, #ffffff);
    padding: 0.625rem 0.625rem 0.5rem;
    color: var(--message-text, #171717);
    box-shadow: 0 6px 24px rgba(0, 0, 0, 0.05);
    transition:
      border-color 180ms ease-out,
      box-shadow 180ms ease-out;
  }

  .composer-shell::before,
  .composer-shell::after {
    position: absolute;
    content: '';
    pointer-events: none;
    opacity: 0;
    transition: opacity 300ms ease-in-out;
    z-index: 0;
  }

  .composer-shell::before {
    inset: -1px;
    border-radius: inherit;
    background: linear-gradient(90deg, #10b981, #3b82f6, #f59e0b, #10b981);
    background-size: 300% 100%;
    mask:
      linear-gradient(#fff 0 0) content-box,
      linear-gradient(#fff 0 0);
    mask-composite: exclude;
    padding: 1.5px;
  }

  .composer-shell::after {
    inset: -1px;
    border-radius: inherit;
    background: linear-gradient(
      90deg,
      rgba(16, 185, 129, 0.4),
      rgba(59, 130, 246, 0.4),
      rgba(245, 158, 11, 0.4),
      rgba(16, 185, 129, 0.4)
    );
    background-size: 300% 100%;
    filter: blur(4px);
    mask:
      linear-gradient(#fff 0 0) content-box,
      linear-gradient(#fff 0 0);
    mask-composite: exclude;
    padding: 3px;
  }

  .composer-shell > :global(*) {
    position: relative;
    z-index: 1;
  }

  .composer-shell.composer-working {
    border-color: transparent;
  }

  .composer-shell.composer-working::before,
  .composer-shell.composer-working::after {
    opacity: 1;
    animation: composer-border-flow 4s linear infinite;
  }

  .composer-toolbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.5rem;
  }

  .composer-toolbar-group {
    display: flex;
    min-width: 0;
    align-items: center;
    gap: 0.25rem;
  }

  /* The right-hand controls keep their size; the left group gives way. */
  .composer-toolbar-group:last-child {
    flex-shrink: 0;
  }

  .prompt-input-wrap {
    position: relative;
    min-width: 0;
  }

  .quick-prompts-row {
    display: flex;
    min-width: 0;
    align-items: flex-start;
    gap: 0.375rem;
  }

  .quick-prompts-scroll {
    display: flex;
    min-width: 0;
    flex: 1;
    flex-wrap: wrap;
    gap: 0.375rem;
    overflow: visible;
  }

  .quick-prompt-chip {
    display: inline-flex;
    max-width: min(15rem, 100%);
    flex: 0 0 auto;
    align-items: center;
    overflow: hidden;
    border: 1px solid var(--message-border, #e6e6e6);
    border-radius: 999px;
    background: color-mix(in srgb, var(--message-bg, #ffffff) 88%, var(--message-surface, #f7f7f7));
    color: var(--message-muted, #737373);
    box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--message-bg, #ffffff) 70%, transparent);
  }

  .quick-prompt-main,
  .quick-prompt-remove {
    border: 0;
    background: transparent;
    color: inherit;
    outline: none;
    transition:
      background-color 120ms ease-out,
      color 120ms ease-out;
  }

  .quick-prompt-main {
    min-width: 0;
    max-width: 13rem;
    overflow: hidden;
    padding: 0.25rem 0.55rem 0.25rem 0.65rem;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 0.75rem;
    font-weight: 500;
    line-height: 1rem;
  }

  .quick-prompt-remove {
    display: inline-grid;
    width: 1.35rem;
    height: 1.35rem;
    flex: 0 0 auto;
    place-items: center;
  }

  .quick-prompt-main:hover,
  .quick-prompt-main:focus-visible,
  .quick-prompt-remove:hover,
  .quick-prompt-remove:focus-visible {
    background: var(--message-surface-hover, #eeeeee);
    color: var(--message-text, #171717);
  }

  .quick-prompt-main:disabled,
  .quick-prompt-remove:disabled {
    cursor: not-allowed;
    opacity: 0.55;
  }

  .quick-prompts-clear {
    flex: 0 0 auto;
  }

  /* In a narrow toolbar the label truncates. No ancestor may hide overflow:
     the menu opens above the button. */
  .approval-mode-wrap {
    position: relative;
    display: flex;
    min-width: 0;
  }

  .approval-mode-button {
    flex-shrink: 1;
    max-width: 12rem;
  }

  .approval-mode-menu {
    position: absolute;
    bottom: calc(100% + 0.5rem);
    left: 0;
    z-index: 20;
    display: grid;
    width: min(20rem, calc(100vw - 2rem));
    gap: 0.125rem;
    border: 1px solid var(--message-border, #e6e6e6);
    border-radius: 0.75rem;
    background: color-mix(in srgb, var(--message-bg, #ffffff) 94%, var(--message-surface, #f7f7f7));
    padding: 0.45rem;
    color: var(--message-text, #171717);
    box-shadow: 0 18px 44px rgba(0, 0, 0, 0.16);
  }

  .approval-mode-menu-eyebrow {
    padding: 0.35rem 0.55rem 0.25rem;
    color: var(--message-muted, #737373);
    font-size: 0.75rem;
    font-weight: 600;
    line-height: 1rem;
  }

  .approval-mode-item {
    display: flex;
    min-width: 0;
    width: 100%;
    align-items: center;
    gap: 0.65rem;
    border: 0;
    border-radius: 0.5rem;
    background: transparent;
    padding: 0.55rem;
    color: inherit;
    text-align: left;
    outline: none;
  }

  .approval-mode-item:hover,
  .approval-mode-item:focus-visible,
  .approval-mode-item-active {
    background: var(--message-surface-hover, #eeeeee);
  }

  .approval-mode-item-icon {
    flex: 0 0 auto;
    color: #0f766e;
  }

  .approval-mode-item-description {
    color: var(--message-muted, #737373);
  }

  :global(.composer-textarea) {
    color: var(--message-text, #171717);
  }

  :global(.composer-textarea::placeholder) {
    color: var(--message-muted-soft, #a0a0a0);
  }

  :global(.composer-icon-button) {
    color: var(--message-muted, #737373);
  }

  :global(.composer-icon-button:hover) {
    background: var(--message-surface-hover, #eeeeee);
    color: var(--message-text, #171717);
  }

  @keyframes composer-border-flow {
    0% {
      background-position: 0% 50%;
    }
    100% {
      background-position: 300% 50%;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .composer-shell.composer-working::before,
    .composer-shell.composer-working::after {
      animation: none;
    }
  }
</style>
