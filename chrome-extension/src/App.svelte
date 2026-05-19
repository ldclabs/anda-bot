<script lang="ts">
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
	import { Button } from '$lib/components/ui/button/index.js'
	import { scrollIntoView } from '$lib/utils/document'
	import {
		Bot,
		ChevronDown,
		ChevronUp,
		CircleAlert,
		History,
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

	const status = $derived(andaClient.status)
	const syncing = $derived(andaClient.activeChannel?.syncing || false)
	const sending = $derived(andaClient.activeChannel?.sending || false)
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

	$effect(() => {
		const lastGroup = visibleMessageGroups[visibleMessageGroups.length - 1]
		const lastMessage = lastGroup?.messages[lastGroup.messages.length - 1]
		if (lastMessage) {
			scrollIntoView(lastMessage.id, 'smooth', 'start')
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

	async function sendPrompt(payload: ComposerSubmitPayload) {
		if (sending) {
			return
		}
		if (!andaClient.settings.token) {
			settingsOpen = true
		}
		await andaClient.sendPrompt(payload.text, payload.attachments)
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

<div class="flex h-screen min-w-80 flex-col overflow-hidden bg-[#f6f8f5] text-stone-950">
	<header
		class="grid grid-cols-[auto_1fr_auto] items-center gap-3 border-b border-emerald-900/10 bg-emerald-50/75 px-3 py-2"
	>
		<div
			class="grid size-8 place-items-center rounded-md border border-emerald-900/10 bg-white/80 shadow-sm"
		>
			<Bot class="size-4 text-emerald-800" />
		</div>

		<div class="min-w-0 text-center">
			<div
				class="mt-0.5 flex min-w-0 items-center justify-center gap-1.5 text-[11px] text-stone-500"
			>
				{#if isBusy}
					<LoaderCircle class="size-3 shrink-0 animate-spin text-emerald-700" />
				{:else if statusIsWarning}
					<CircleAlert class={`size-3 shrink-0 ${statusIconClass()}`} />
				{:else}
					<Radio class={`size-3 shrink-0 ${statusIconClass()}`} />
				{/if}
				<span class="truncate">{status}</span>
			</div>
			{#if andaClient.systemMessage}
				<p class="truncate text-xs font-bold text-stone-800">
					{andaClient.systemMessage.text}
				</p>
			{/if}
		</div>

		<Button
			variant="ghost"
			size="icon"
			aria-label={chrome.i18n.getMessage('settings')}
			title={chrome.i18n.getMessage('settings')}
			onclick={toggleSettingsPanel}
		>
			<Settings class="size-4" />
		</Button>
	</header>

	{#if settingsOpen}
		<ChatSettings {setupGuideOpen} />
	{/if}

	<main
		bind:this={messagesElement}
		class="scrollbar-slim flex min-h-0 w-full flex-1 flex-col gap-3 overflow-y-auto px-3 py-4"
	>
		{#if !andaClient.activeChannel || andaClient.activeChannel.messageGroups.length === 0}
			<div class="m-auto grid max-w-64 place-items-center gap-2 text-center text-stone-500">
				<div
					class="grid size-11 place-items-center rounded-md border border-stone-200 bg-white shadow-sm"
				>
					{#if syncing}
						<LoaderCircle class="size-5 animate-spin text-emerald-800" />
					{:else}
						<Bot class="size-5 text-emerald-800" />
					{/if}
				</div>
				<div class="text-xs font-semibold text-stone-700">
					{syncing ? chrome.i18n.getMessage('syncing') : chrome.i18n.getMessage('ready')}
				</div>
			</div>
		{:else}
			{#if hasPreviousConversations}
				<div class="flex justify-center">
					<Button
						variant="outline"
						size="xs"
						class="bg-white/80 text-stone-600 shadow-sm"
						disabled={loadingPrevious}
						onclick={loadPreviousConversations}
					>
						{#if loadingPrevious}
							<LoaderCircle class="size-3 animate-spin" />
						{:else}
							<History class="size-3" />
						{/if}
						{chrome.i18n.getMessage('loadHistory')}
					</Button>
				</div>
			{/if}

			{#each visibleMessageGroups as group (group._id)}
				<section class="grid w-full gap-2">
					{#if visibleMessageGroups.length > 1}
						<div
							class="flex items-center justify-center gap-2 py-1 text-[10px] font-semibold text-stone-400"
						>
							<span class="h-px flex-1 bg-stone-200"></span>
							<span class="max-w-[70%] truncate">{groupLabel(group)}</span>
							<span class="rounded-full bg-stone-100 px-1.5 py-0.5 text-stone-500">
								{group.status}
							</span>
							<span class="h-px flex-1 bg-stone-200"></span>
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
		<section class="border-t border-emerald-900/10 bg-emerald-50/80 backdrop-blur">
			<button
				type="button"
				class="flex h-10 w-full items-center gap-2 px-3 text-left transition hover:bg-white/55"
				aria-expanded={sideMessagesOpen}
				aria-label={chrome.i18n.getMessage(
					sideMessagesOpen ? 'collapseSideTasks' : 'expandSideTasks'
				)}
				title={chrome.i18n.getMessage(sideMessagesOpen ? 'collapseSideTasks' : 'expandSideTasks')}
				onclick={toggleSideMessagesPanel}
			>
				<span
					class="grid size-6 shrink-0 place-items-center rounded-md border border-emerald-900/10 bg-white/85 text-emerald-800 shadow-sm"
				>
					<Bot class="size-3.5" />
				</span>
				<span class="min-w-0 flex-1 truncate text-xs font-bold text-stone-700">
					{chrome.i18n.getMessage('sideTasksLabel')}
				</span>
				<span
					class="shrink-0 rounded-full border border-emerald-900/10 bg-white/80 px-1.5 py-0.5 text-[10px] leading-none font-bold text-emerald-800"
				>
					{sideMessageCount}
				</span>
				{#if sideMessagesOpen}
					<ChevronDown class="size-4 shrink-0 text-stone-500" />
				{:else}
					<ChevronUp class="size-4 shrink-0 text-stone-500" />
				{/if}
			</button>

			{#if sideMessagesOpen}
				<div
					bind:this={sideMessagesElement}
					class="scrollbar-slim max-h-3/4 overflow-y-auto border-t border-emerald-900/10 px-3 py-3"
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

	<footer class="border-t border-stone-200 bg-[#f6f8f5]/90 p-2.5 backdrop-blur">
		<ChatComposer
			placeholder={andaClient.settings.token
				? chrome.i18n.getMessage('placeholderMessage')
				: chrome.i18n.getMessage('placeholderSettings')}
			disabled={sending}
			{sending}
			working={isBusy}
			voiceAvailable={andaClient.voiceCapabilities.transcription.length > 0}
			voiceCapabilities={andaClient.voiceCapabilities}
			submitKeyMode={andaClient.settings.submitKeyMode}
			onSend={sendPrompt}
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
