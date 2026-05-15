<script lang="ts" module>
	import type {
		ChatAttachment,
		PageAudioResult,
		PromptSkill,
		ResourceInput,
		VoiceCapabilities,
		VoiceProvider,
		VoiceRecordingInput
	} from '$lib/anda/client'

	export interface ComposerSubmitPayload {
		text: string
		attachments: ChatAttachment[]
	}

	export type ComposerVoicePayload = VoiceRecordingInput
</script>

<script lang="ts">
	import { Button } from '$lib/components/ui/button/index.js'
	import {
		FileText,
		Keyboard,
		LoaderCircle,
		Mic,
		Paperclip,
		SendHorizontal,
		Square,
		Volume2,
		VolumeX,
		X
	} from '@lucide/svelte'
	import { onDestroy, onMount, tick } from 'svelte'

	type BrowserSpeechRecognitionEvent = {
		resultIndex: number
		results: ArrayLike<{
			isFinal: boolean
			[index: number]: { transcript: string }
		}>
	}

	type BrowserSpeechRecognitionError = {
		error?: string
		message?: string
	}

	type BrowserSpeechRecognition = {
		lang: string
		continuous: boolean
		interimResults: boolean
		onresult: ((event: BrowserSpeechRecognitionEvent) => void) | null
		onerror: ((event: BrowserSpeechRecognitionError) => void) | null
		onend: (() => void) | null
		start(): void
		stop(): void
		abort?: () => void
	}

	type BrowserSpeechRecognitionConstructor = new () => BrowserSpeechRecognition

	type PromptCommandContext = {
		open: boolean
		mode: 'command' | 'skill'
		query: string
		replaceStart: number
		replaceEnd: number
		key: string
	}

	type PromptCommandSuggestion = {
		id: string
		label: string
		insertText: string
		description: string
		detail?: string
		disabled?: boolean
		kind: 'command' | 'skill' | 'status'
	}

	const emptyPromptCommandContext: PromptCommandContext = {
		open: false,
		mode: 'command',
		query: '',
		replaceStart: 0,
		replaceEnd: 0,
		key: 'closed'
	}

	const promptCommandItems: PromptCommandSuggestion[] = [
		{
			id: 'command:goal',
			label: '/goal',
			insertText: '/goal ',
			description: 'Start a supervised long-running task.',
			detail: 'alias: /loop',
			kind: 'command'
		},
		{
			id: 'command:side',
			label: '/side',
			insertText: '/side ',
			description: 'Run a temporary side request in a subagent.',
			detail: 'alias: /btw',
			kind: 'command'
		},
		{
			id: 'command:steer',
			label: '/steer',
			insertText: '/steer ',
			description: 'Redirect the next model step with a new instruction.',
			kind: 'command'
		},
		{
			id: 'command:skill',
			label: '/skill',
			insertText: '/skill ',
			description: 'Route the prompt to a named skill subagent.',
			kind: 'command'
		},
		{
			id: 'command:stop',
			label: '/stop',
			insertText: '/stop ',
			description: 'Cancel the current task with an optional reason.',
			detail: 'alias: /cancel',
			kind: 'command'
		},
		{
			id: 'command:cancel',
			label: '/cancel',
			insertText: '/cancel ',
			description: 'Cancel the current task with an optional reason.',
			detail: 'alias: /stop',
			kind: 'command'
		},
		{
			id: 'command:ping',
			label: '/ping',
			insertText: '/ping ',
			description: 'Send a lightweight ping.',
			kind: 'command'
		}
	]
	const promptSkillsCacheMs = 60_000

	let {
		disabled = false,
		sending = false,
		placeholder = chrome.i18n.getMessage('placeholderMessage'),
		working = false,
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
		onLoadSkills
	}: {
		disabled?: boolean
		sending?: boolean
		placeholder?: string
		working?: boolean
		voiceAvailable?: boolean
		voiceCapabilities?: VoiceCapabilities
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
	let promptCommandListElement: HTMLDivElement | null = $state(null)
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
		isMacPlatform()
			? chrome.i18n.getMessage('sendWithCmdEnter')
			: chrome.i18n.getMessage('sendWithCtrlEnter')
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
		if (!promptCommandPanelOpen) {
			return
		}
		activePromptCommandIndex
		promptCommandSelectionKey
		void tick().then(scrollActivePromptCommandIntoView)
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
		if (event.shiftKey || disabled || sending || preparingAttachments || event.isComposing) {
			return false
		}
		if (event.keyCode === 229) {
			return false
		}
		const isEnter = event.key === 'Enter' || event.code === 'Enter' || event.keyCode === 13
		if (!isEnter) {
			return false
		}
		return isMacPlatform() ? event.metaKey : event.ctrlKey
	}

	function readPromptCommandContext(value: string, caret: number): PromptCommandContext {
		const safeCaret = Math.max(0, Math.min(caret, value.length))
		const firstLineBreak = value.indexOf('\n')
		const commandLineEnd = firstLineBreak === -1 ? value.length : firstLineBreak
		if (safeCaret > commandLineEnd) {
			return emptyPromptCommandContext
		}

		const commandLine = value.slice(0, commandLineEnd)
		const leadingWhitespace = commandLine.match(/^\s*/)?.[0] || ''
		const slashIndex = leadingWhitespace.length
		if (commandLine[slashIndex] !== '/' || safeCaret < slashIndex + 1) {
			return emptyPromptCommandContext
		}

		const commandBody = commandLine.slice(slashIndex + 1)
		const commandToken = commandBody.match(/^\S*/)?.[0] || ''
		const commandTokenEnd = slashIndex + 1 + commandToken.length
		const commandName = commandToken.toLowerCase()

		if (commandName === 'skill' && safeCaret >= commandTokenEnd) {
			const afterCommand = commandLine.slice(commandTokenEnd)
			const spacesAfterCommand = afterCommand.match(/^\s+/)?.[0] || ''
			if (spacesAfterCommand.length > 0) {
				const skillStart = commandTokenEnd + spacesAfterCommand.length
				const skillToken = commandLine.slice(skillStart).match(/^\S*/)?.[0] || ''
				const skillEnd = skillStart + skillToken.length
				if (safeCaret >= skillStart && safeCaret <= skillEnd) {
					const query = commandLine.slice(skillStart, safeCaret)
					return {
						open: true,
						mode: 'skill',
						query,
						replaceStart: skillStart,
						replaceEnd: skillEnd,
						key: `skill:${query}:${skillStart}:${skillEnd}`
					}
				}
				return emptyPromptCommandContext
			}
		}

		if (safeCaret > commandTokenEnd) {
			return emptyPromptCommandContext
		}

		let replaceEnd = commandTokenEnd
		while (replaceEnd < commandLineEnd && /[ \t]/.test(value[replaceEnd])) {
			replaceEnd += 1
		}
		const query = commandLine.slice(slashIndex + 1, Math.min(safeCaret, commandTokenEnd))
		return {
			open: true,
			mode: 'command',
			query,
			replaceStart: slashIndex,
			replaceEnd,
			key: `command:${query}:${slashIndex}:${replaceEnd}`
		}
	}

	function buildPromptCommandSuggestions(
		context: PromptCommandContext,
		skills: PromptSkill[],
		skillsLoading: boolean,
		skillsError: string
	): PromptCommandSuggestion[] {
		if (!context.open) {
			return []
		}

		const query = context.query.trim().toLowerCase()
		if (context.mode === 'command') {
			const matches = promptCommandItems.filter((item) => {
				const label = item.label.slice(1).toLowerCase()
				const detail = item.detail?.toLowerCase() || ''
				return !query || label.startsWith(query) || detail.includes(`/${query}`)
			})
			return matches.length
				? matches
				: [promptCommandStatus('commands-empty', chrome.i18n.getMessage('promptCommandsEmpty'))]
		}

		if (skillsLoading && skills.length === 0) {
			return [promptCommandStatus('skills-loading', chrome.i18n.getMessage('promptSkillsLoading'))]
		}
		if (skillsError) {
			return [promptCommandStatus('skills-error', skillsError)]
		}

		const matches = skills
			.filter((skill) => {
				const name = skill.name.toLowerCase()
				const description = skill.description?.toLowerCase() || ''
				return !query || name.includes(query) || description.includes(query)
			})
			.slice(0, 20)
		return matches.length
			? matches.map((skill) => ({
					id: `skill:${skill.name}`,
					label: skill.name,
					insertText: `${skill.name} `,
					description: skill.description || chrome.i18n.getMessage('promptSkillDescription'),
					detail: '/skill',
					kind: 'skill'
				}))
			: [promptCommandStatus('skills-empty', chrome.i18n.getMessage('promptSkillsEmpty'))]
	}

	function promptCommandStatus(id: string, description: string): PromptCommandSuggestion {
		return {
			id,
			label: '',
			insertText: '',
			description,
			kind: 'status',
			disabled: true
		}
	}

	function firstEnabledPromptCommandIndex(suggestions: PromptCommandSuggestion[]): number {
		const index = suggestions.findIndex((suggestion) => !suggestion.disabled)
		return index === -1 ? 0 : index
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

	function scrollActivePromptCommandIntoView() {
		const activeOption = promptCommandListElement?.querySelector<HTMLElement>(
			`[data-prompt-command-index="${activePromptCommandIndex}"]`
		)
		activeOption?.scrollIntoView({ block: 'nearest' })
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
		if (isSubmitEvent(event)) {
			event.preventDefault()
			void submitMessage()
			return
		}
		if (handlePromptCommandKeydown(event)) {
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

	function chromeSpeechErrorMessage(error: string): string {
		const normalized = error.toLowerCase()
		if (normalized.includes('permission dismissed')) {
			return 'Chrome speech permission was dismissed.'
		}
		if (normalized.includes('permission was not accepted')) {
			return 'Chrome speech permission was not accepted.'
		}
		if (
			normalized.includes('microphone access was blocked') ||
			normalized.includes('not-allowed')
		) {
			return 'Chrome speech microphone access was blocked.'
		}
		return error || 'Chrome speech recognition did not start.'
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

	function audioCaptureErrorMessage(error: string): string {
		const normalized = error.toLowerCase()
		if (normalized.includes('permission dismissed')) {
			return 'Microphone permission was dismissed for the current page.'
		}
		if (
			normalized.includes('microphone access was blocked') ||
			normalized.includes('notallowed') ||
			normalized.includes('not-allowed')
		) {
			return 'Microphone access was blocked for the current page.'
		}
		return error || 'Anda voice recording did not start.'
	}

	function isPermissionError(error: string): boolean {
		const normalized = error.toLowerCase()
		return (
			normalized.includes('permission') ||
			normalized.includes('microphone access was blocked') ||
			normalized.includes('notallowed') ||
			normalized.includes('not-allowed')
		)
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

	function speechRecognitionErrorMessage(error: string): string {
		switch (error) {
			case 'not-allowed':
			case 'service-not-allowed':
				return 'Microphone access was blocked.'
			case 'audio-capture':
				return 'No microphone was found.'
			case 'network':
				return 'Browser speech recognition is offline.'
			default:
				return error || 'Browser speech recognition failed.'
		}
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

	function preferredRecordingMimeType(acceptedFormats: string[] = []): string {
		if (typeof MediaRecorder === 'undefined') {
			return ''
		}
		const accepted = new Set(acceptedFormats.map((format) => format.toLowerCase()))
		const directTypes = [
			{ format: 'webm', mimeType: 'audio/webm;codecs=opus' },
			{ format: 'webm', mimeType: 'audio/webm' },
			{ format: 'ogg', mimeType: 'audio/ogg;codecs=opus' },
			{ format: 'mp4', mimeType: 'audio/mp4' },
			{ format: 'm4a', mimeType: 'audio/mp4' }
		]
		const direct = directTypes.find(
			({ format, mimeType }) => accepted.has(format) && MediaRecorder.isTypeSupported(mimeType)
		)
		if (direct) {
			return direct.mimeType
		}
		const fallbackTypes = [
			'audio/webm;codecs=opus',
			'audio/webm',
			'audio/ogg;codecs=opus',
			'audio/mp4'
		]
		return fallbackTypes.find((type) => MediaRecorder.isTypeSupported(type)) || ''
	}

	function audioExtensionForMime(mimeType: string): string {
		const normalized = mimeType.toLowerCase()
		if (normalized.includes('ogg')) {
			return 'ogg'
		}
		if (normalized.includes('mp4')) {
			return 'm4a'
		}
		if (normalized.includes('wav')) {
			return 'wav'
		}
		return 'webm'
	}

	async function blobToBase64(blob: Blob): Promise<string> {
		const dataUrl = await new Promise<string>((resolve, reject) => {
			const reader = new FileReader()
			reader.onload = () => resolve(String(reader.result || ''))
			reader.onerror = () => reject(reader.error || new Error('Failed to read voice audio.'))
			reader.readAsDataURL(blob)
		})
		return dataUrl.split(',', 2)[1] || ''
	}

	function isMacPlatform(): boolean {
		if (typeof navigator === 'undefined') {
			return false
		}
		return /mac|iphone|ipad|ipod/i.test(navigator.platform || navigator.userAgent)
	}

	function fileSizeLabel(size: number): string {
		if (size < 1024) {
			return `${size} B`
		}
		if (size < 1024 * 1024) {
			return `${(size / 1024).toFixed(1)} KB`
		}
		return `${(size / 1024 / 1024).toFixed(1)} MB`
	}

	async function fileToAttachment(file: File): Promise<ChatAttachment> {
		const blob = arrayBufferToBase64(await file.arrayBuffer())
		const extension = file.name.includes('.') ? file.name.split('.').pop()?.toLowerCase() : ''
		const primaryType = file.type.includes('/') ? file.type.split('/')[0] : ''
		const tags = Array.from(
			new Set(
				[primaryType, extension, isTextLike(file.type, extension) ? 'text' : 'file'].filter(
					Boolean
				) as string[]
			)
		)
		const resource: ResourceInput = {
			_id: 0,
			tags,
			name: file.name,
			mime_type: file.type || undefined,
			blob,
			size: file.size,
			metadata: {
				source: file.webkitRelativePath || 'chrome_extension',
				last_modified: file.lastModified
			}
		}
		return {
			id: `${file.name}-${file.size}-${file.lastModified}`,
			name: file.name,
			type: file.type || extension || undefined,
			size: file.size,
			resource
		}
	}

	function isTextLike(mimeType: string, extension: string | undefined): boolean {
		return (
			mimeType.startsWith('text/') ||
			['md', 'markdown', 'txt', 'json', 'csv', 'ts', 'js', 'rs', 'py', 'html', 'css'].includes(
				extension || ''
			)
		)
	}

	function arrayBufferToBase64(buffer: ArrayBuffer): string {
		const bytes = new Uint8Array(buffer)
		const chunkSize = 0x8000
		let binary = ''
		for (let index = 0; index < bytes.length; index += chunkSize) {
			binary += String.fromCharCode(...bytes.subarray(index, index + chunkSize))
		}
		return btoa(binary)
	}

	function speechRecognitionSupported(): boolean {
		return Boolean(speechRecognitionConstructor())
	}

	function speechRecognitionConstructor(): BrowserSpeechRecognitionConstructor | null {
		const scope = globalThis as typeof globalThis & {
			SpeechRecognition?: BrowserSpeechRecognitionConstructor
			webkitSpeechRecognition?: BrowserSpeechRecognitionConstructor
		}
		return scope.SpeechRecognition || scope.webkitSpeechRecognition || null
	}
</script>

<form
	class="composer-shell rounded-lg border border-stone-100 bg-white p-2 shadow-[0_10px_30px_rgba(36,45,39,0.08)]"
	class:composer-working={composerWorking}
	aria-busy={composerWorking}
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

	{#if attachments.length}
		<div class="mb-2 flex flex-wrap gap-1.5 px-1">
			{#each attachments as attachment (attachment.id)}
				{#if attachment.type?.startsWith('image/')}
					<div
						class="group relative size-8 shrink-0 overflow-hidden rounded-md border border-stone-200 bg-stone-50 shadow-sm transition-all hover:border-emerald-500/50"
					>
						<img
							src={`data:${attachment.type};base64,${attachment.resource.blob}`}
							alt={attachment.name}
							class="size-full object-cover"
						/>
						<button
							type="button"
							class="absolute top-0 right-0 grid size-3.5 place-items-center rounded-bl-md bg-black/50 text-white opacity-0 transition-opacity group-hover:opacity-100 hover:bg-red-500"
							aria-label={chrome.i18n.getMessage('removeAttachment')}
							onclick={() => removeAttachment(attachment.id)}
						>
							<X class="size-2" />
						</button>
					</div>
				{:else}
					<span
						class="inline-flex max-w-full items-center gap-1.5 rounded-md border border-stone-200 bg-stone-50 px-2 py-1 text-[11px] text-stone-600"
						title={attachment.name}
					>
						<FileText class="size-3 shrink-0 text-emerald-700" />
						<span class="max-w-30 truncate">{attachment.name}</span>
						<span class="shrink-0 text-stone-400">{fileSizeLabel(attachment.size || 0)}</span>
						<button
							type="button"
							class="grid size-4 shrink-0 place-items-center rounded-sm text-stone-400 hover:bg-stone-200 hover:text-stone-700"
							aria-label={chrome.i18n.getMessage('removeAttachment')}
							title={chrome.i18n.getMessage('removeAttachment')}
							onclick={() => removeAttachment(attachment.id)}
						>
							<X class="size-3" />
						</button>
					</span>
				{/if}
			{/each}
		</div>
	{/if}

	{#if attachmentError}
		<div
			class="mb-2 rounded-md border border-amber-200 bg-amber-50 px-2 py-1 text-[11px] text-amber-800"
		>
			{attachmentError}
		</div>
	{/if}

	<div class="grid gap-2">
		{#if inputMode === 'voice'}
			<div
				class="voice-panel relative grid min-h-32 place-items-center overflow-hidden rounded-md border border-emerald-900/10 bg-[#06120f] px-3 py-4 text-white"
				class:active={voiceStage === 'recording' || voiceStage === 'processing' || sending}
			>
				<div class="voice-field"></div>
				<button
					type="button"
					class="voice-orb relative grid place-items-center"
					class:recording={voiceStage === 'recording'}
					class:processing={voiceStage === 'processing' || sending}
					style={voiceOrbStyle}
					disabled={!canRecordVoice}
					aria-label={voiceStage === 'recording'
						? chrome.i18n.getMessage('stopRecording')
						: chrome.i18n.getMessage('startRecording')}
					title={voiceStage === 'recording'
						? chrome.i18n.getMessage('stopRecording')
						: chrome.i18n.getMessage('startRecording')}
					onclick={toggleRecording}
				>
					<span class="voice-orb-core"></span>
					<span class="voice-orb-icon">
						{#if voiceStage === 'processing' || sending}
							<LoaderCircle class="size-5 animate-spin" />
						{:else if voiceStage === 'recording'}
							<Square class="size-4 fill-current" />
						{:else}
							<Mic class="size-5" />
						{/if}
					</span>
				</button>

				<div class="relative z-10 mt-3 flex items-center gap-2 text-[11px] font-semibold">
					<span class="voice-status-dot" class:recording={voiceStage === 'recording'}></span>
					<span>{voiceStatus}</span>
				</div>
				<div class="voice-service relative z-10 mt-2 flex items-center gap-1 text-[11px]">
					<div class="voice-service-switch" aria-label="Voice service">
						<button
							type="button"
							class:active={voiceProvider === 'chrome'}
							disabled={!canUseBrowserSpeech || voiceStage !== 'idle'}
							title={chrome.i18n.getMessage('useChromeVoice')}
							onclick={() => selectVoiceProvider('chrome')}
						>
							Chrome
						</button>
						<button
							type="button"
							class:active={voiceProvider === 'anda'}
							disabled={!canUseAndaVoice || voiceStage !== 'idle'}
							title={chrome.i18n.getMessage('useAndaVoice')}
							onclick={() => selectVoiceProvider('anda')}
						>
							Anda
						</button>
					</div>
					<span class="voice-service-label"
						>{voiceProvider === 'chrome'
							? chrome.i18n.getMessage('chromeVoiceService')
							: chrome.i18n.getMessage('andaVoiceService')}</span
					>
				</div>
				{#if voiceTranscript}
					<div
						class="voice-transcript relative z-10 mt-2 max-w-full truncate px-3 text-center text-[11px] text-emerald-50/90"
					>
						{voiceTranscript}
					</div>
				{/if}
			</div>
		{:else}
			<div class="prompt-input-wrap">
				{#if promptCommandPanelOpen}
					<div class="prompt-command-panel" role="listbox" aria-label={promptCommandPanelTitle}>
						<div class="prompt-command-title">{promptCommandPanelTitle}</div>
						<div class="prompt-command-list" bind:this={promptCommandListElement}>
							{#each promptCommandSuggestions as suggestion, index (suggestion.id)}
								{#if suggestion.disabled}
									<div class="prompt-command-status">{suggestion.description}</div>
								{:else}
									<button
										type="button"
										class="prompt-command-option"
										class:active={index === activePromptCommandIndex}
										data-prompt-command-index={index}
										role="option"
										aria-selected={index === activePromptCommandIndex}
										onmousedown={(event) => event.preventDefault()}
										onclick={() => void applyPromptCommandSuggestion(suggestion)}
									>
										<span class="prompt-command-main">
											<span class="prompt-command-label">{suggestion.label}</span>
											{#if suggestion.detail}
												<span class="prompt-command-detail">{suggestion.detail}</span>
											{/if}
										</span>
										<span class="prompt-command-description">{suggestion.description}</span>
									</button>
								{/if}
							{/each}
						</div>
					</div>
				{/if}
				<textarea
					bind:this={textareaElement}
					bind:value={text}
					rows="1"
					{placeholder}
					spellcheck="true"
					disabled={disabled || sending}
					aria-haspopup="listbox"
					class="max-h-38 min-h-10 w-full resize-none border-0 bg-transparent px-2 py-2 leading-5 text-stone-950 outline-none placeholder:text-stone-400 disabled:cursor-not-allowed disabled:opacity-60"
					onkeydown={handleKeydown}
					oninput={handleTextareaInput}
					onfocus={handleTextareaFocus}
					onblur={handleTextareaBlur}
					onclick={updateTextareaCaret}
					onkeyup={updateTextareaCaret}
					onselect={updateTextareaCaret}
				></textarea>
			</div>
		{/if}

		{#if inputMode === 'voice' && voiceError}
			<div
				class="rounded-md border border-amber-200 bg-amber-50 px-2 py-1 text-[11px] text-amber-800"
			>
				{voiceError}
			</div>
		{/if}

		<div class="flex items-center justify-between gap-2">
			<div class="flex items-center gap-1">
				<Button
					type="button"
					variant="ghost"
					size="icon-sm"
					class="text-stone-500 hover:text-emerald-700"
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
				</Button>

				{#if canUseVoice}
					<Button
						type="button"
						variant={inputMode === 'voice' ? 'secondary' : 'ghost'}
						size="icon-sm"
						class="text-stone-500 hover:text-emerald-700"
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
					</Button>
				{/if}
			</div>

			<div class="flex items-center gap-1">
				{#if inputMode === 'voice'}
					<Button
						type="button"
						variant={ttsEnabled ? 'secondary' : 'ghost'}
						size="icon-sm"
						class="text-stone- stone-500 hover:text-emerald-700"
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
					</Button>
				{:else}
					<div class="group relative flex items-center">
						<span
							class="pointer-events-none absolute right-full mr-2 hidden rounded bg-stone-800 px-2 py-1 text-[10px] font-medium whitespace-nowrap text-white opacity-0 transition-opacity duration-200 group-hover:block group-hover:opacity-100"
						>
							{submitTitle}
						</span>
						<Button
							type="submit"
							size="icon-sm"
							variant={canSend ? 'default' : 'ghost'}
							disabled={!canSend}
							class="transition-all duration-200 {canSend
								? 'bg-primary/80 shadow-sm hover:bg-primary focus-visible:bg-primary'
								: 'text-stone-300'}"
							aria-label={chrome.i18n.getMessage('send')}
						>
							{#if sending}
								<LoaderCircle class="size-4 animate-spin" />
							{:else}
								<SendHorizontal class="size-4" />
							{/if}
						</Button>
					</div>
				{/if}
			</div>
		</div>
	</div>
</form>

<style>
	.composer-shell {
		position: relative;
		isolation: isolate;
		overflow: visible;
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
		border-radius: 9px;
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

	.prompt-input-wrap {
		position: relative;
		min-width: 0;
	}

	.prompt-command-panel {
		position: absolute;
		left: 0;
		right: 0;
		bottom: calc(100% + 8px);
		z-index: 30;
		max-height: min(260px, 45vh);
		overflow: hidden;
		border: 1px solid rgba(120, 113, 108, 0.18);
		border-radius: 8px;
		background: rgba(255, 255, 255, 0.98);
		box-shadow:
			0 18px 48px rgba(36, 45, 39, 0.16),
			0 0 0 1px rgba(255, 255, 255, 0.7) inset;
		backdrop-filter: blur(14px);
	}

	.prompt-command-title {
		padding: 7px 9px 5px;
		border-bottom: 1px solid rgba(231, 229, 228, 0.9);
		font-size: 10px;
		font-weight: 700;
		letter-spacing: 0;
		text-transform: uppercase;
		color: #78716c;
	}

	.prompt-command-list {
		max-height: 218px;
		overflow-y: auto;
		padding: 4px;
	}

	.prompt-command-option {
		width: 100%;
		min-width: 0;
		border: 0;
		border-radius: 6px;
		background: transparent;
		padding: 7px 8px;
		text-align: left;
		transition:
			background 140ms ease-out,
			box-shadow 140ms ease-out;
	}

	.prompt-command-option:hover,
	.prompt-command-option.active {
		background: #ecfdf5;
		box-shadow: inset 0 0 0 1px rgba(16, 185, 129, 0.16);
	}

	.prompt-command-main {
		display: flex;
		min-width: 0;
		align-items: center;
		gap: 6px;
	}

	.prompt-command-label {
		flex: 0 0 auto;
		font-family:
			ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, 'Liberation Mono', monospace;
		font-size: 12px;
		font-weight: 750;
		color: #065f46;
	}

	.prompt-command-detail,
	.prompt-command-description {
		min-width: 0;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}

	.prompt-command-detail {
		font-size: 10px;
		font-weight: 600;
		color: #a16207;
	}

	.prompt-command-description {
		display: block;
		margin-top: 2px;
		font-size: 11px;
		color: #57534e;
	}

	.prompt-command-status {
		padding: 9px 8px;
		font-size: 11px;
		color: #78716c;
	}

	.voice-panel {
		isolation: isolate;
	}

	.voice-panel::before {
		position: absolute;
		inset: -50% -50%;
		content: '';
		background: conic-gradient(
			from 0deg,
			transparent,
			rgba(16, 185, 129, 0.3),
			rgba(59, 130, 246, 0.3),
			rgba(245, 158, 11, 0.3),
			transparent
		);
		filter: blur(40px);
		opacity: 0;
		transition: opacity 500ms ease-in-out;
		z-index: -2;
	}

	.voice-panel.active::before {
		opacity: 1;
		animation: voice-panel-rotate 10s linear infinite;
	}

	.voice-panel::after {
		position: absolute;
		inset: 0;
		content: '';
		background-image: radial-gradient(
			circle at 2px 2px,
			rgba(255, 255, 255, 0.05) 1px,
			transparent 0
		);
		background-size: 24px 24px;
		mask-image: radial-gradient(circle, black 30%, transparent 80%);
		opacity: 0.4;
		z-index: -1;
	}

	.voice-field {
		position: absolute;
		inset: 0;
		border-radius: inherit;
		background: radial-gradient(circle at center, rgba(16, 185, 129, 0.1) 0%, transparent 70%);
		transform: scale(calc(0.5 + var(--voice-level, 0) * 1.2));
		opacity: calc(0.1 + var(--voice-level, 0) * 0.5);
		transition: transform 150ms cubic-bezier(0.2, 0, 0.3, 1);
	}

	.voice-orb {
		width: 100px;
		height: 100px;
		border: 0;
		border-radius: 999px;
		color: white;
		background: #06120f;
		position: relative;
		display: grid;
		place-items: center;
		transition: transform 0.3s cubic-bezier(0.34, 1.56, 0.64, 1);
	}

	.voice-orb:hover:not(:disabled) {
		transform: scale(1.05);
	}

	.voice-orb::before {
		content: '';
		position: absolute;
		inset: -2px;
		border-radius: inherit;
		background: linear-gradient(135deg, #10b981, #3b82f6, #f59e0b);
		padding: 2px;
		mask:
			linear-gradient(#fff 0 0) content-box,
			linear-gradient(#fff 0 0);
		mask-composite: exclude;
		animation: voice-orb-border-rotate 4s linear infinite;
	}

	.voice-orb.recording::after {
		content: '';
		position: absolute;
		inset: -8px;
		border-radius: inherit;
		border: 2px solid rgba(16, 185, 129, 0.4);
		animation: voice-orb-pulse 2s cubic-bezier(0, 0, 0.2, 1) infinite;
	}

	.voice-orb.recording {
		transform: scale(calc(1 + var(--voice-level, 0) * 0.2));
	}

	.voice-orb.processing {
		animation: voice-orb-breathing 2s ease-in-out infinite;
	}

	.voice-orb-core {
		position: absolute;
		inset: 6px;
		border-radius: inherit;
		background: radial-gradient(circle at 30% 30%, rgba(255, 255, 255, 0.1), transparent);
		box-shadow:
			inset 0 0 20px rgba(16, 185, 129, 0.2),
			0 0 30px rgba(16, 185, 129, 0.1);
	}

	.voice-orb-icon {
		position: relative;
		z-index: 2;
		display: grid;
		place-items: center;
		width: 44px;
		height: 44px;
		border-radius: 999px;
		background: rgba(255, 255, 255, 0.05);
		backdrop-filter: blur(4px);
		box-shadow: 0 4px 12px rgba(0, 0, 0, 0.2);
		transition: all 0.3s ease;
	}

	.voice-orb.recording .voice-orb-icon {
		background: rgba(16, 185, 129, 0.2);
		box-shadow: 0 0 15px rgba(16, 185, 129, 0.4);
	}

	.voice-status-dot {
		width: 8px;
		height: 8px;
		border-radius: 999px;
		background: #10b981;
		box-shadow: 0 0 10px rgba(16, 185, 129, 0.8);
		transition: all 0.3s ease;
	}

	.voice-status-dot.recording {
		background: #f59e0b;
		box-shadow: 0 0 15px rgba(245, 158, 11, 0.9);
		animation: status-dot-blink 1s ease-in-out infinite;
	}

	@keyframes status-dot-blink {
		50% {
			opacity: 0.5;
			transform: scale(0.8);
		}
	}

	@keyframes voice-orb-pulse {
		0% {
			transform: scale(1);
			opacity: 0.8;
		}
		100% {
			transform: scale(1.5);
			opacity: 0;
		}
	}

	@keyframes voice-orb-breathing {
		0%,
		100% {
			transform: scale(1);
			opacity: 0.9;
		}
		50% {
			transform: scale(1.05);
			opacity: 1;
		}
	}

	@keyframes voice-orb-border-rotate {
		from {
			rotate: 0deg;
		}
		to {
			rotate: 360deg;
		}
	}

	@keyframes voice-panel-rotate {
		from {
			transform: rotate(0deg);
		}
		to {
			transform: rotate(360deg);
		}
	}

	.voice-service {
		max-width: 100%;
	}

	.voice-service-label {
		max-width: 136px;
		overflow: hidden;
		color: rgba(236, 253, 245, 0.78);
		font-weight: 650;
		text-overflow: ellipsis;
		white-space: nowrap;
	}

	.voice-service-switch {
		display: inline-flex;
		padding: 2px;
		border: 1px solid rgba(255, 255, 255, 0.14);
		border-radius: 8px;
		background: rgba(6, 18, 15, 0.42);
		box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.05);
	}

	.voice-service-switch button {
		min-width: 48px;
		border: 0;
		border-radius: 6px;
		padding: 3px 8px;
		color: rgba(236, 253, 245, 0.68);
		font-weight: 700;
		line-height: 1.2;
		transition:
			background 140ms ease-out,
			color 140ms ease-out,
			opacity 140ms ease-out;
	}

	.voice-service-switch button.active {
		background: rgba(236, 253, 245, 0.92);
		color: #064e3b;
	}

	.voice-service-switch button:disabled {
		cursor: not-allowed;
	}

	.voice-service-switch button:disabled:not(.active) {
		opacity: 0.42;
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
