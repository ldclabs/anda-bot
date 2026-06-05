<script lang="ts" module>
  import type {
    ChatAttachment,
    ChatMessage,
    PageAudioResult,
    PromptSkill,
    SubmitKeyMode,
    VoiceCapabilities,
    VoiceProvider,
    VoiceRecordingInput
  } from '$lib/anda/client/types'

  export interface ComposerSubmitPayload {
    text: string
    attachments: ChatAttachment[]
  }

  export type ComposerVoicePayload = VoiceRecordingInput
</script>

<script lang="ts">
  import AttachmentList from '$lib/anda/composer/AttachmentList.svelte'
  import { fileToAttachment } from '$lib/anda/composer/attachments'
  import {
    buildPromptCommandSuggestions,
    firstEnabledPromptCommandIndex,
    promptSkillsCacheMs,
    readPromptCommandContext,
    type PromptCommandSuggestion
  } from '$lib/anda/composer/prompt-commands'
  import PromptCommandPanel from '$lib/anda/composer/PromptCommandPanel.svelte'
  import {
    audioCaptureErrorMessage,
    audioExtensionForMime,
    blobToBase64,
    chromeSpeechErrorMessage,
    isMacPlatform,
    isPermissionError,
    preferredRecordingMimeType,
    speechRecognitionConstructor,
    speechRecognitionErrorMessage,
    speechRecognitionSupported,
    type BrowserSpeechRecognition,
    type BrowserSpeechRecognitionError,
    type BrowserSpeechRecognitionEvent
  } from '$lib/anda/composer/voice'
  import VoicePanel from '$lib/anda/composer/VoicePanel.svelte'
  import {
    alertClass,
    alertDescriptionClass,
    buttonClass,
    cardClass,
    inputClass,
    inputGroupClass,
    textareaClass,
    tooltipArrowClass,
    tooltipContentClass
  } from '$lib/anda/ui'
  import {
    Keyboard,
    LoaderCircle,
    Mic,
    Paperclip,
    SendHorizontal,
    Volume2,
    VolumeX
  } from '@lucide/svelte'
  import { Tooltip } from 'bits-ui'
  import { onDestroy, onMount, tick } from 'svelte'

  let {
    disabled = false,
    sending = false,
    placeholder = chrome.i18n.getMessage('placeholderMessage'),
    working = false,
    pendingFollowUps = [],
    voiceAvailable = false,
    voiceCapabilities = { transcription: [], daemonTts: [], chromeTts: false },
    onSend,
    onVoiceSend,
    onBrowserSpeechStart,
    onBrowserSpeechStop,
    onBrowserSpeechCancel,
    onBrowserAudioStart,
    onBrowserAudioStop,
    onBrowserAudioCancel,
    onLoadSkills,
    submitKeyMode = 'enter'
  }: {
    disabled?: boolean
    sending?: boolean
    placeholder?: string
    working?: boolean
    pendingFollowUps?: ChatMessage[]
    voiceAvailable?: boolean
    voiceCapabilities?: VoiceCapabilities
    submitKeyMode?: SubmitKeyMode
    onSend: (payload: ComposerSubmitPayload) => Promise<void> | void
    onVoiceSend?: (payload: ComposerVoicePayload) => Promise<void> | void
    onBrowserSpeechStart?: (language: string) => Promise<void>
    onBrowserSpeechStop?: () => Promise<string>
    onBrowserSpeechCancel?: () => Promise<void>
    onBrowserAudioStart?: (mimeType?: string) => Promise<void>
    onBrowserAudioStop?: () => Promise<PageAudioResult>
    onBrowserAudioCancel?: () => Promise<void>
    onLoadSkills?: () => Promise<PromptSkill[]>
  } = $props()

  let text = $state('')
  let attachments = $state<ChatAttachment[]>([])
  let attachmentError = $state('')
  let preparingAttachments = $state(false)
  let inputMode = $state<'text' | 'voice'>('text')
  let voiceStage = $state<'idle' | 'recording' | 'processing'>('idle')
  let voiceError = $state('')
  let voiceTranscript = $state('')
  let voiceLevel = $state(0)
  let voiceProvider = $state<VoiceProvider>('anda')
  let voiceProviderSelected = $state(false)
  let ttsEnabled = $state(false)
  let browserSpeechAvailable = $state(speechRecognitionSupported())
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
  let speechRecognition: BrowserSpeechRecognition | null = null
  let speechRecognitionMode: 'local' | 'page' | null = null
  let speechFinalTranscript = ''
  let ignoreNextRecognition = false
  let speechRecognitionStopRequested = false
  let speechRecognitionFatalError = ''
  let mediaRecorder: MediaRecorder | null = null
  let mediaStream: MediaStream | null = null
  let audioRecordingMode: 'local' | 'page' | null = null
  let audioChunks: Blob[] = []
  let audioContext: AudioContext | null = null
  let analyserNode: AnalyserNode | null = null
  let levelAnimationFrame: number | null = null
  let ignoreNextRecording = false

  const canSend = $derived(
    (Boolean(text.trim()) || attachments.length > 0) &&
      !disabled &&
      !sending &&
      !preparingAttachments
  )
  const submitTitle = $derived(
    submitKeyMode === 'modifier-enter'
      ? isMacPlatform()
        ? chrome.i18n.getMessage('sendWithCmdEnter')
        : chrome.i18n.getMessage('sendWithCtrlEnter')
      : chrome.i18n.getMessage('sendWithEnter')
  )
  const canUseBrowserSpeech = $derived(
    Boolean(onBrowserSpeechStart && onBrowserSpeechStop) || browserSpeechAvailable
  )
  const canUseAndaVoice = $derived(voiceAvailable || voiceCapabilities.transcription.length > 0)
  const canUseSelectedVoiceProvider = $derived(
    voiceProvider === 'chrome' ? canUseBrowserSpeech : canUseAndaVoice
  )
  const selectedVoiceTtsAvailable = $derived(
    voiceProvider === 'chrome'
      ? voiceCapabilities.chromeTts
      : voiceCapabilities.daemonTts.length > 0
  )
  const canUseVoice = $derived(canUseBrowserSpeech || canUseAndaVoice)
  const canRecordVoice = $derived(
    canUseSelectedVoiceProvider &&
      !disabled &&
      !sending &&
      !preparingAttachments &&
      voiceStage !== 'processing'
  )
  const voiceProviderLabel = $derived(voiceProvider === 'chrome' ? 'Chrome' : 'Anda')
  const voiceProviderTitle = $derived(
    voiceProvider === 'chrome'
      ? chrome.i18n.getMessage('useChromeVoice')
      : chrome.i18n.getMessage('useAndaVoice')
  )
  const voiceStatus = $derived(
    voiceStage === 'recording'
      ? chrome.i18n.getMessage('listening')
      : voiceStage === 'processing' || sending
        ? chrome.i18n.getMessage('working')
        : chrome.i18n.getMessage('ready')
  )
  const voiceOrbStyle = $derived(`--voice-level: ${voiceLevel.toFixed(3)}`)
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
      ? chrome.i18n.getMessage('promptSkillsLabel')
      : chrome.i18n.getMessage('promptCommandsLabel')
  )

  let workingPersisted = $state(false)
  let workingTimeout: number | undefined

  $effect(() => {
    if (working || sending || voiceStage === 'processing') {
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

  const composerWorking = $derived(workingPersisted)

  $effect(() => {
    if (!canUseVoice && inputMode === 'voice') {
      void cancelRecording()
      inputMode = 'text'
    }
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
    if (promptCommandPanelOpen && promptCommandContext.mode === 'skill') {
      void ensurePromptSkillsLoaded()
    }
  })

  $effect(() => {
    if (voiceStage === 'idle') {
      if (!voiceProviderSelected && canUseAndaVoice && voiceProvider !== 'anda') {
        voiceProvider = 'anda'
      }
      if (voiceProvider === 'anda' && !canUseAndaVoice && canUseBrowserSpeech) {
        voiceProvider = 'chrome'
      }
      if (voiceProvider === 'chrome' && !canUseBrowserSpeech && canUseAndaVoice) {
        voiceProvider = 'anda'
      }
    }
    if (ttsEnabled && !selectedVoiceTtsAvailable) {
      ttsEnabled = false
    }
  })

  onMount(() => {
    browserSpeechAvailable = speechRecognitionSupported()
  })

  onDestroy(() => {
    void cancelRecording()
  })

  function isSubmitEvent(event: KeyboardEvent): boolean {
    if (disabled || sending || preparingAttachments || event.isComposing) {
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
    if (!canSend) {
      return
    }
    const payload: ComposerSubmitPayload = {
      text: text.trim(),
      attachments
    }
    await onSend(payload)
    text = ''
    attachments = []
    attachmentError = ''
    inputMode = 'text'
    promptCommandDismissedKey = ''
    caretIndex = 0
    await tick()
    resizeTextarea()
  }

  function handleKeydown(event: KeyboardEvent) {
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
      await addFiles(files as unknown as FileList)
    }
  }

  async function addFiles(fileList: FileList | File[] | null) {
    if (
      !fileList ||
      (fileList instanceof FileList ? fileList.length === 0 : (fileList as File[]).length === 0)
    ) {
      return
    }
    attachmentError = ''
    preparingAttachments = true
    try {
      const nextAttachments: ChatAttachment[] = []
      const filesArray = fileList instanceof FileList ? Array.from(fileList) : fileList
      for (const file of filesArray) {
        nextAttachments.push(await fileToAttachment(file))
      }
      const existingIds = new Set(attachments.map((attachment) => attachment.id))
      attachments = [
        ...attachments,
        ...nextAttachments.filter((attachment) => !existingIds.has(attachment.id))
      ]
    } catch (error) {
      attachmentError = error instanceof Error ? error.message : String(error)
    } finally {
      preparingAttachments = false
    }
  }

  function removeAttachment(id: string) {
    attachments = attachments.filter((attachment) => attachment.id !== id)
  }

  function pendingFollowUpPreview(message: ChatMessage): string {
    const text = message.text.trim()
    if (text) {
      return text
    }
    return (message.attachments || []).map((attachment) => attachment.name).join(', ')
  }

  function toggleInputMode() {
    if (inputMode === 'voice') {
      void cancelRecording()
      inputMode = 'text'
      void tick().then(() => textareaElement?.focus())
      return
    }
    if (canUseVoice) {
      inputMode = 'voice'
      voiceError = ''
    }
  }

  async function toggleRecording() {
    if (!canRecordVoice) {
      return
    }
    if (voiceStage === 'recording') {
      stopRecording()
      return
    }
    await startRecording()
  }

  async function startRecording() {
    voiceTranscript = ''
    if (voiceProvider === 'chrome' && canUseBrowserSpeech) {
      const started = await startSpeechRecognition()
      if (started) {
        return
      }
      const chromeError = voiceError
      if (canUseAndaVoice) {
        voiceProvider = 'anda'
        voiceError = ''
        await startAndaRecording()
        return
      }
      voiceError = chromeSpeechErrorMessage(chromeError)
      return
    }
    if (canUseAndaVoice) {
      voiceProvider = 'anda'
      await startAndaRecording()
      return
    }
    voiceError = 'Selected voice service is unavailable.'
  }

  async function startAndaRecording() {
    if (onBrowserAudioStart && onBrowserAudioStop) {
      const started = await startPageAudioRecording()
      if (started) {
        return
      }
      if (isPermissionError(voiceError)) {
        return
      }
    }
    await startAudioRecording()
  }

  async function startSpeechRecognition(): Promise<boolean> {
    if (onBrowserSpeechStart && onBrowserSpeechStop) {
      return startPageSpeechRecognition()
    }
    return startLocalSpeechRecognition()
  }

  async function startPageSpeechRecognition(): Promise<boolean> {
    voiceError = ''
    voiceTranscript = ''
    speechFinalTranscript = ''
    ignoreNextRecognition = false
    speechRecognitionStopRequested = false
    speechRecognitionFatalError = ''
    speechRecognitionMode = 'page'
    voiceStage = 'recording'
    startSyntheticVoicePulse()
    try {
      await onBrowserSpeechStart?.(navigator.language || 'zh-CN')
      return true
    } catch (error) {
      speechRecognitionMode = null
      cleanupRecordingResources()
      voiceStage = 'idle'
      voiceLevel = 0
      voiceError = chromeSpeechErrorMessage(error instanceof Error ? error.message : String(error))
      return false
    }
  }

  function startLocalSpeechRecognition(): boolean {
    const Recognition = speechRecognitionConstructor()
    if (!Recognition) {
      browserSpeechAvailable = false
      voiceError = 'Browser speech recognition is unavailable.'
      return false
    }

    voiceError = ''
    voiceTranscript = ''
    speechFinalTranscript = ''
    ignoreNextRecognition = false
    speechRecognitionStopRequested = false
    speechRecognitionFatalError = ''
    try {
      const recognition = new Recognition()
      recognition.lang = navigator.language || 'zh-CN'
      recognition.continuous = true
      recognition.interimResults = true
      recognition.onresult = handleSpeechRecognitionResult
      recognition.onerror = (event) => {
        handleSpeechRecognitionError(event)
      }
      recognition.onend = () => {
        void handleSpeechRecognitionEnd(recognition)
      }
      speechRecognition = recognition
      speechRecognitionMode = 'local'
      recognition.start()
      voiceStage = 'recording'
      startSyntheticVoicePulse()
      return true
    } catch (error) {
      speechRecognition = null
      speechRecognitionMode = null
      voiceStage = 'idle'
      voiceError = chromeSpeechErrorMessage(error instanceof Error ? error.message : String(error))
      return false
    }
  }

  function selectVoiceProvider(provider: VoiceProvider) {
    voiceProvider = provider
    voiceProviderSelected = true
    voiceError = ''
  }

  async function startPageAudioRecording(): Promise<boolean> {
    voiceError = ''
    voiceTranscript = ''
    ignoreNextRecording = false
    audioRecordingMode = 'page'
    voiceStage = 'recording'
    startSyntheticVoicePulse()
    try {
      await onBrowserAudioStart?.(preferredRecordingMimeType(voiceCapabilities.transcription))
      return true
    } catch (error) {
      audioRecordingMode = null
      cleanupRecordingResources()
      voiceStage = 'idle'
      voiceLevel = 0
      voiceError = audioCaptureErrorMessage(error instanceof Error ? error.message : String(error))
      return false
    }
  }

  async function startAudioRecording() {
    if (!navigator.mediaDevices?.getUserMedia || typeof MediaRecorder === 'undefined') {
      voiceError = 'Voice input is unavailable in this browser.'
      return
    }
    voiceError = ''
    ignoreNextRecording = false
    audioRecordingMode = 'local'
    try {
      const stream = await navigator.mediaDevices.getUserMedia({
        audio: {
          echoCancellation: true,
          noiseSuppression: true,
          autoGainControl: true
        }
      })
      const mimeType = preferredRecordingMimeType(voiceCapabilities.transcription)
      const recorder = new MediaRecorder(stream, mimeType ? { mimeType } : undefined)
      mediaStream = stream
      mediaRecorder = recorder
      audioChunks = []
      recorder.ondataavailable = (event) => {
        if (event.data.size > 0) {
          audioChunks.push(event.data)
        }
      }
      recorder.onstop = () => {
        void finishRecording(recorder.mimeType || mimeType || 'audio/webm')
      }
      startVoiceLevelMeter(stream)
      recorder.start()
      voiceStage = 'recording'
    } catch (error) {
      cleanupRecordingResources()
      voiceStage = 'idle'
      voiceError = audioCaptureErrorMessage(error instanceof Error ? error.message : String(error))
    }
  }

  function stopRecording() {
    if (speechRecognitionMode === 'page') {
      void finishPageSpeechRecognition()
      return
    }
    if (audioRecordingMode === 'page') {
      void finishPageAudioRecording()
      return
    }
    if (speechRecognition) {
      speechRecognitionStopRequested = true
      voiceStage = 'processing'
      speechRecognition.stop()
      return
    }
    if (mediaRecorder?.state === 'recording') {
      voiceStage = 'processing'
      mediaRecorder.stop()
    }
  }

  async function cancelRecording() {
    ignoreNextRecognition = true
    speechRecognitionStopRequested = false
    speechRecognitionFatalError = ''
    ignoreNextRecording = true
    if (speechRecognitionMode === 'page') {
      await onBrowserSpeechCancel?.().catch(() => undefined)
      speechRecognitionMode = null
    }
    if (audioRecordingMode === 'page') {
      await onBrowserAudioCancel?.().catch(() => undefined)
      audioRecordingMode = null
    }
    if (speechRecognition) {
      speechRecognition.onend = null
      try {
        speechRecognition.abort?.()
      } catch (_error) {
        try {
          speechRecognition.stop()
        } catch (_stopError) {}
      }
      speechRecognition = null
    }
    if (mediaRecorder?.state === 'recording') {
      mediaRecorder.stop()
    }
    cleanupRecordingResources()
    voiceStage = 'idle'
    voiceLevel = 0
  }

  function handleSpeechRecognitionResult(event: BrowserSpeechRecognitionEvent) {
    voiceError = ''
    let interimTranscript = ''
    for (let index = event.resultIndex; index < event.results.length; index += 1) {
      const result = event.results[index]
      const transcript = result[0]?.transcript?.trim() || ''
      if (!transcript) {
        continue
      }
      if (result.isFinal) {
        speechFinalTranscript = `${speechFinalTranscript} ${transcript}`.trim()
      } else {
        interimTranscript = `${interimTranscript} ${transcript}`.trim()
      }
    }
    voiceTranscript = `${speechFinalTranscript} ${interimTranscript}`.trim()
    voiceLevel = Math.min(1, Math.max(0.28, voiceLevel + 0.18))
  }

  function handleSpeechRecognitionError(event: BrowserSpeechRecognitionError) {
    const errorName = event.error || ''
    if (errorName === 'no-speech') {
      return
    }
    if (errorName === 'aborted' && ignoreNextRecognition) {
      return
    }
    speechRecognitionFatalError = errorName || event.message || 'Browser speech recognition failed.'
    voiceError = event.message || speechRecognitionErrorMessage(speechRecognitionFatalError)
  }

  async function handleSpeechRecognitionEnd(recognition: BrowserSpeechRecognition) {
    if (ignoreNextRecognition || speechRecognitionFatalError) {
      await finishSpeechRecognition()
      return
    }
    if (!speechRecognitionStopRequested && voiceStage === 'recording') {
      try {
        recognition.start()
        return
      } catch (error) {
        speechRecognitionFatalError = error instanceof Error ? error.message : String(error)
        voiceError = speechRecognitionErrorMessage(speechRecognitionFatalError)
      }
    }
    await finishSpeechRecognition()
  }

  async function finishPageSpeechRecognition() {
    if (!onBrowserSpeechStop) {
      voiceError = 'Voice mode is not connected.'
      voiceStage = 'idle'
      return
    }
    speechRecognitionStopRequested = true
    voiceStage = 'processing'
    try {
      const transcript = (await onBrowserSpeechStop()).trim()
      speechFinalTranscript = transcript
      voiceTranscript = transcript
      await finishSpeechRecognition()
    } catch (error) {
      speechRecognitionMode = null
      cleanupRecordingResources()
      voiceLevel = 0
      voiceError = error instanceof Error ? error.message : String(error)
      voiceStage = 'idle'
    }
  }

  async function finishSpeechRecognition() {
    const transcript = voiceTranscript.trim() || speechFinalTranscript.trim()
    speechRecognition = null
    speechRecognitionMode = null
    cleanupRecordingResources()
    voiceLevel = 0
    speechRecognitionStopRequested = false
    if (ignoreNextRecognition) {
      ignoreNextRecognition = false
      voiceStage = 'idle'
      return
    }
    if (speechRecognitionFatalError) {
      speechRecognitionFatalError = ''
      voiceStage = 'idle'
      return
    }
    if (!transcript) {
      voiceError = 'No speech was recognized.'
      voiceStage = 'idle'
      return
    }
    if (!onVoiceSend) {
      voiceError = 'Voice mode is not connected.'
      voiceStage = 'idle'
      return
    }
    try {
      voiceStage = 'processing'
      await onVoiceSend({ transcript, ttsEnabled, voiceProvider })
      voiceError = ''
    } catch (error) {
      voiceError = error instanceof Error ? error.message : String(error)
    } finally {
      voiceStage = 'idle'
    }
  }

  async function finishPageAudioRecording() {
    if (!onBrowserAudioStop) {
      voiceError = 'Voice mode is not connected.'
      voiceStage = 'idle'
      return
    }
    voiceStage = 'processing'
    let result: PageAudioResult | null = null
    try {
      result = await onBrowserAudioStop()
    } catch (error) {
      voiceError = audioCaptureErrorMessage(error instanceof Error ? error.message : String(error))
    } finally {
      cleanupRecordingResources()
      voiceLevel = 0
    }
    if (ignoreNextRecording) {
      ignoreNextRecording = false
      voiceStage = 'idle'
      return
    }
    if (!result) {
      voiceStage = 'idle'
      return
    }
    if (!result.audioBase64 || !result.mimeType) {
      voiceError = chrome.i18n.getMessage('noVoiceCaptured')
      voiceStage = 'idle'
      return
    }
    if (!onVoiceSend) {
      voiceError = chrome.i18n.getMessage('voiceNotConnected')
      voiceStage = 'idle'
      return
    }
    try {
      await onVoiceSend({
        voiceProvider: 'anda',
        audioBase64: result.audioBase64,
        fileName: `chrome_voice_${Date.now()}.${audioExtensionForMime(result.mimeType)}`,
        mimeType: result.mimeType,
        size: result.size,
        ttsEnabled
      })
      voiceError = ''
    } catch (error) {
      voiceError = error instanceof Error ? error.message : String(error)
    } finally {
      voiceStage = 'idle'
    }
  }

  async function finishRecording(mimeType: string) {
    const chunks = audioChunks
    cleanupRecordingResources()
    voiceLevel = 0
    if (ignoreNextRecording) {
      ignoreNextRecording = false
      voiceStage = 'idle'
      return
    }
    const blob = new Blob(chunks, { type: mimeType })
    if (!blob.size) {
      voiceError = chrome.i18n.getMessage('noVoiceCaptured')
      voiceStage = 'idle'
      return
    }
    if (!onVoiceSend) {
      voiceError = chrome.i18n.getMessage('voiceNotConnected')
      voiceStage = 'idle'
      return
    }
    try {
      voiceStage = 'processing'
      await onVoiceSend({
        voiceProvider,
        audioBase64: await blobToBase64(blob),
        fileName: `chrome_voice_${Date.now()}.${audioExtensionForMime(mimeType)}`,
        mimeType,
        size: blob.size,
        ttsEnabled
      })
      voiceError = ''
    } catch (error) {
      voiceError = error instanceof Error ? error.message : String(error)
    } finally {
      voiceStage = 'idle'
    }
  }

  function cleanupRecordingResources() {
    if (levelAnimationFrame !== null) {
      cancelAnimationFrame(levelAnimationFrame)
      levelAnimationFrame = null
    }
    void audioContext?.close().catch(() => undefined)
    audioContext = null
    analyserNode = null
    mediaStream?.getTracks().forEach((track) => track.stop())
    mediaStream = null
    mediaRecorder = null
    audioRecordingMode = null
    audioChunks = []
  }

  function startSyntheticVoicePulse() {
    if (levelAnimationFrame !== null) {
      cancelAnimationFrame(levelAnimationFrame)
    }
    const tickLevel = () => {
      voiceLevel = voiceStage === 'recording' ? Math.max(0.12, voiceLevel * 0.86) : 0
      levelAnimationFrame = requestAnimationFrame(tickLevel)
    }
    tickLevel()
  }

  function startVoiceLevelMeter(stream: MediaStream) {
    const context = new AudioContext()
    const source = context.createMediaStreamSource(stream)
    const analyser = context.createAnalyser()
    analyser.fftSize = 256
    source.connect(analyser)
    audioContext = context
    analyserNode = analyser
    const samples = new Uint8Array(analyser.frequencyBinCount)
    const updateLevel = () => {
      analyser.getByteFrequencyData(samples)
      const total = samples.reduce((sum, sample) => sum + sample, 0)
      voiceLevel = Math.min(1, total / samples.length / 120)
      levelAnimationFrame = requestAnimationFrame(updateLevel)
    }
    updateLevel()
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
    class={inputClass('hidden')}
    onchange={handleFileInput}
  />

  <div
    class={cardClass(
      `composer-shell gap-2 rounded-lg border-stone-100 bg-white p-2 shadow-[0_10px_30px_rgba(36,45,39,0.08)] ${
        composerWorking ? 'composer-working' : ''
      }`
    )}
    aria-busy={composerWorking}
  >
    <AttachmentList {attachments} onRemove={removeAttachment} />

    {#if pendingFollowUps.length}
      <div class="pending-follow-ups" aria-live="polite">
        <div class="pending-follow-ups-header">
          <span class="pending-follow-ups-pulse">
            <LoaderCircle class="size-3 animate-spin" />
          </span>
          <span class="min-w-0 flex-1 truncate">
            {chrome.i18n.getMessage('queuedFollowUps')}
          </span>
          <span class="pending-follow-ups-count">{pendingFollowUps.length}</span>
        </div>
        <div class="pending-follow-ups-list">
          {#each pendingFollowUps as message (message.id)}
            <div class="pending-follow-up-item">
              <span class="pending-follow-up-dot"></span>
              <span class="min-w-0 flex-1 truncate">{pendingFollowUpPreview(message)}</span>
            </div>
          {/each}
        </div>
      </div>
    {/if}

    {#if attachmentError}
      <div
        role="alert"
        class={alertClass(
          'mb-2 rounded-md border-amber-200 bg-amber-50 px-2 py-1 text-[11px] text-amber-800'
        )}
      >
        <div class={alertDescriptionClass('text-[11px] text-amber-800')}>
          {attachmentError}
        </div>
      </div>
    {/if}

    <div class="grid gap-2">
      {#if inputMode === 'voice'}
        <VoicePanel
          {voiceStage}
          {sending}
          {canRecordVoice}
          {voiceOrbStyle}
          {voiceStatus}
          {voiceProvider}
          {canUseBrowserSpeech}
          {canUseAndaVoice}
          {voiceTranscript}
          onToggleRecording={toggleRecording}
          onSelectVoiceProvider={selectVoiceProvider}
        />
      {:else}
        <div class="prompt-input-wrap">
          {#if promptCommandPanelOpen}
            <PromptCommandPanel
              title={promptCommandPanelTitle}
              suggestions={promptCommandSuggestions}
              activeIndex={activePromptCommandIndex}
              onApply={applyPromptCommandSuggestion}
            />
          {/if}
          <div
            role="group"
            class={inputGroupClass(
              'h-auto min-h-10 border-0 bg-transparent shadow-none has-[[data-slot=input-group-control]:focus-visible]:ring-0'
            )}
          >
            <textarea
              bind:this={textareaElement}
              data-slot="input-group-control"
              bind:value={text}
              rows={1}
              {placeholder}
              spellcheck="true"
              disabled={disabled || sending}
              aria-haspopup="listbox"
              class={textareaClass(
                'max-h-38 min-h-10 flex-1 resize-none rounded-none border-0 bg-transparent px-2 leading-5 text-stone-950 shadow-none ring-0 placeholder:text-stone-400 focus-visible:ring-0 disabled:opacity-60 aria-invalid:ring-0 dark:bg-transparent'
              )}
              onkeydown={handleKeydown}
              oninput={handleTextareaInput}
              onfocus={handleTextareaFocus}
              onblur={handleTextareaBlur}
              onclick={updateTextareaCaret}
              onkeyup={updateTextareaCaret}
              onselect={updateTextareaCaret}
            ></textarea>
          </div>
        </div>
      {/if}

      {#if inputMode === 'voice' && voiceError}
        <div
          role="alert"
          class={alertClass(
            'rounded-md border-amber-200 bg-amber-50 px-2 py-1 text-[11px] text-amber-800'
          )}
        >
          <div class={alertDescriptionClass('text-[11px] text-amber-800')}>
            {voiceError}
          </div>
        </div>
      {/if}

      <div class="flex items-center justify-between gap-2">
        <div class="flex items-center gap-1">
          <button
            type="button"
            class={buttonClass('ghost', 'icon-sm', 'text-stone-500 hover:text-emerald-700')}
            disabled={disabled || preparingAttachments}
            aria-label={chrome.i18n.getMessage('attachFiles')}
            title={chrome.i18n.getMessage('attachFiles')}
            onclick={openFileDialog}
          >
            {#if preparingAttachments}
              <LoaderCircle class="size-4 animate-spin" />
            {:else}
              <Paperclip class="size-4" />
            {/if}
          </button>

          {#if canUseVoice}
            <button
              type="button"
              class={buttonClass(
                inputMode === 'voice' ? 'secondary' : 'ghost',
                'icon-sm',
                'text-stone-500 hover:text-emerald-700'
              )}
              disabled={disabled || sending}
              aria-label={inputMode === 'voice'
                ? chrome.i18n.getMessage('switchToKeyboard')
                : chrome.i18n.getMessage('switchToVoice')}
              title={inputMode === 'voice'
                ? chrome.i18n.getMessage('keyboardInput')
                : chrome.i18n.getMessage('voiceInput')}
              onclick={toggleInputMode}
            >
              {#if inputMode === 'voice'}
                <Keyboard class="size-4" />
              {:else}
                <Mic class="size-4" />
              {/if}
            </button>
          {/if}
        </div>

        <div class="flex items-center gap-1">
          {#if inputMode === 'voice'}
            <button
              type="button"
              class={buttonClass(
                ttsEnabled ? 'secondary' : 'ghost',
                'icon-sm',
                'text-stone-500 hover:text-emerald-700'
              )}
              disabled={disabled ||
                sending ||
                voiceStage === 'recording' ||
                !selectedVoiceTtsAvailable}
              aria-label={ttsEnabled
                ? chrome.i18n.getMessage('disablePlayback')
                : chrome.i18n.getMessage('enablePlayback')}
              title={selectedVoiceTtsAvailable
                ? `${voiceProviderLabel} ${ttsEnabled ? chrome.i18n.getMessage('playbackOn') : chrome.i18n.getMessage('playbackOff')}`
                : `${voiceProviderLabel} ${chrome.i18n.getMessage('playbackUnavailable')}`}
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
                    <button
                      {...props}
                      type="submit"
                      disabled={!canSend}
                      class={buttonClass(
                        canSend ? 'default' : 'ghost',
                        'icon-sm',
                        `duration-200 ${
                          canSend
                            ? 'bg-primary/80 shadow-sm hover:bg-primary focus-visible:bg-primary'
                            : 'text-stone-300'
                        }`
                      )}
                      aria-label={chrome.i18n.getMessage('send')}
                    >
                      {#if sending}
                        <LoaderCircle class="size-4 animate-spin" />
                      {:else}
                        <SendHorizontal class="size-4" />
                      {/if}
                    </button>
                  {/snippet}
                </Tooltip.Trigger>
                <Tooltip.Portal>
                  <Tooltip.Content side="top" sideOffset={6} class={tooltipContentClass()}>
                    {submitTitle}
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
  </div>
</form>

<style>
  :global(.composer-shell) {
    position: relative;
    isolation: isolate;
    overflow: visible;
    transition:
      border-color 180ms ease-out,
      box-shadow 180ms ease-out;
  }

  :global(.composer-shell)::before,
  :global(.composer-shell)::after {
    position: absolute;
    content: '';
    pointer-events: none;
    opacity: 0;
    transition: opacity 300ms ease-in-out;
    z-index: 0;
  }

  :global(.composer-shell)::before {
    inset: -1px;
    border-radius: 9px;
    background: linear-gradient(90deg, #10b981, #3b82f6, #f59e0b, #10b981);
    background-size: 300% 100%;
    mask:
      linear-gradient(#fff 0 0) content-box,
      linear-gradient(#fff 0 0);
    mask-composite: exclude;
    padding: 1.5px;
  }

  :global(.composer-shell)::after {
    inset: -1px;
    border-radius: 9px;
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

  :global(.composer-shell) > :global(*) {
    position: relative;
    z-index: 1;
  }

  :global(.composer-shell.composer-working) {
    border-color: transparent;
  }

  :global(.composer-shell.composer-working)::before,
  :global(.composer-shell.composer-working)::after {
    opacity: 1;
    animation: composer-border-flow 4s linear infinite;
  }

  .pending-follow-ups {
    display: grid;
    gap: 6px;
    border-radius: 7px;
    border: 1px solid rgba(4, 120, 87, 0.16);
    background: rgba(236, 253, 245, 0.82);
    padding: 7px;
    color: #2f3b35;
    box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.74);
  }

  .pending-follow-ups-header,
  .pending-follow-up-item {
    display: flex;
    min-width: 0;
    align-items: center;
  }

  .pending-follow-ups-header {
    gap: 7px;
    font-size: 11px;
    font-weight: 700;
    line-height: 1.2;
  }

  .pending-follow-ups-pulse {
    display: grid;
    width: 20px;
    height: 20px;
    flex: 0 0 auto;
    place-items: center;
    border-radius: 6px;
    border: 1px solid rgba(4, 120, 87, 0.12);
    background: rgba(255, 255, 255, 0.74);
    color: #047857;
  }

  .pending-follow-ups-count {
    min-width: 18px;
    border-radius: 999px;
    background: rgba(255, 255, 255, 0.76);
    padding: 1px 6px;
    text-align: center;
    font-size: 10px;
    font-weight: 800;
    color: #047857;
  }

  .pending-follow-ups-list {
    display: grid;
    gap: 4px;
  }

  .pending-follow-up-item {
    gap: 7px;
    border-radius: 6px;
    background: rgba(255, 255, 255, 0.58);
    padding: 5px 7px;
    font-size: 12px;
    line-height: 1.25;
    color: #1f2d27;
  }

  .pending-follow-up-dot {
    width: 5px;
    height: 5px;
    flex: 0 0 auto;
    border-radius: 999px;
    background: #10b981;
    box-shadow: 0 0 0 3px rgba(16, 185, 129, 0.12);
  }

  .prompt-input-wrap {
    position: relative;
    min-width: 0;
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
    :global(.composer-shell.composer-working)::before,
    :global(.composer-shell.composer-working)::after {
      animation: none;
    }
  }
</style>
