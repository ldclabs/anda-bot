<script lang="ts">
	import ChatComposer, {
		type ComposerSubmitPayload,
		type ComposerVoicePayload
	} from '$lib/anda/ChatComposer.svelte'
	import ChatMessageItem from '$lib/anda/ChatMessageItem.svelte'
	import {
		AndaSidePanelClient,
		type ChatMessage,
		type ChromeTabInfo,
		type ConversationGroup,
		type PageAudioResult,
		type SettingsState,
		type VoiceCapabilities
	} from '$lib/anda/client'
	import { Button } from '$lib/components/ui/button/index.js'
	import { Input } from '$lib/components/ui/input/index.js'
	import { Textarea } from '$lib/components/ui/textarea/index.js'
	import {
		Bot,
		CircleAlert,
		ExternalLink,
		History,
		KeyRound,
		LoaderCircle,
		PlugZap,
		Radio,
		Save,
		Settings
	} from '@lucide/svelte'
	import { onMount, tick } from 'svelte'

	let client: AndaSidePanelClient | null = null
	let settings = $state<SettingsState>({ baseUrl: 'http://127.0.0.1:8042', token: '' })
	let draftSettings = $state<SettingsState>({ baseUrl: 'http://127.0.0.1:8042', token: '' })
	let tab = $state<ChromeTabInfo | null>(null)
	let conversationGroups = $state<ConversationGroup[]>([])
	let messages = $state<ChatMessage[]>([])
	let status = $state('starting')
	let sending = $state(false)
	let settingsOpen = $state(false)
	let settingsDirty = $state(false)
	let savingSettings = $state(false)
	let testingConnection = $state(false)
	let loadingPrevious = $state(false)
	let hasPreviousConversations = $state(false)
	let syncing = $state(false)
	let voiceCapabilities = $state<VoiceCapabilities>({
		transcription: [],
		daemonTts: [],
		chromeTts: false
	})
	let shouldStickToBottom = $state(true)
	let historyLoadInFlight = $state(false)
	let messagesElement: HTMLElement | null = null

	const isBusy = $derived(
		sending || syncing || ['sending', 'submitted', 'working', 'connecting'].includes(status)
	)
	const statusIsWarning = $derived(
		[
			'request failed',
			'poll failed',
			'connection failed',
			'extension unavailable',
			'restore failed',
			'history failed',
			'voice failed',
			'failed'
		].includes(status)
	)
	const showHistoryControl = $derived(messages.length > 0 && hasPreviousConversations)
	const visibleConversationGroups = $derived(displayConversationGroups(conversationGroups))

	onMount(() => {
		client = new AndaSidePanelClient((snapshot) => {
			settings = snapshot.settings
			if (!settingsDirty) {
				draftSettings = { ...snapshot.settings }
			}
			tab = snapshot.tab
			conversationGroups = snapshot.conversationGroups
			messages = snapshot.messages
			status = snapshot.status
			sending = snapshot.sending
			loadingPrevious = snapshot.loadingPrevious
			hasPreviousConversations = snapshot.hasPreviousConversations
			syncing = snapshot.syncing
			voiceCapabilities = snapshot.voiceCapabilities
			if (!snapshot.settings.token) {
				settingsOpen = true
			}
		})

		client.init().catch((error) => {
			status = 'extension unavailable'
			conversationGroups = [
				{
					id: 'startup-error',
					status: 'failed',
					messages: [
						{
							id: 'startup-error-message',
							role: 'system',
							text: error instanceof Error ? error.message : String(error),
							timestamp: Date.now()
						}
					]
				}
			]
			messages = conversationGroups.flatMap((group) => group.messages)
			settingsOpen = true
		})

		return () => client?.destroy()
	})

	$effect(() => {
		messages.length
		if (!historyLoadInFlight && shouldStickToBottom) {
			void tick().then(scrollMessagesToBottom)
		}
	})

	function scrollMessagesToBottom() {
		if (messagesElement) {
			messagesElement.scrollTop = messagesElement.scrollHeight
		}
	}

	function handleMessagesScroll() {
		if (!messagesElement) {
			return
		}
		const distanceFromBottom =
			messagesElement.scrollHeight - messagesElement.scrollTop - messagesElement.clientHeight
		shouldStickToBottom = distanceFromBottom < 90
		if (messagesElement.scrollTop < 32 && showHistoryControl && !loadingPrevious) {
			void loadPreviousConversations()
		}
	}

	async function loadPreviousConversations() {
		if (!client || loadingPrevious || historyLoadInFlight || !hasPreviousConversations) {
			return
		}
		const beforeHeight = messagesElement?.scrollHeight || 0
		const beforeTop = messagesElement?.scrollTop || 0
		historyLoadInFlight = true
		try {
			const loaded = await client.loadPreviousConversations()
			await tick()
			if (loaded && messagesElement) {
				messagesElement.scrollTop = messagesElement.scrollHeight - beforeHeight + beforeTop
			}
		} finally {
			historyLoadInFlight = false
		}
	}

	function markSettingsDirty() {
		settingsDirty = true
	}

	async function saveSettings() {
		if (!client || savingSettings) {
			return
		}
		savingSettings = true
		try {
			await client.saveSettings(draftSettings)
			settingsDirty = false
			settingsOpen = false
			draftSettings = { ...settings }
		} finally {
			savingSettings = false
		}
	}

	async function testConnection() {
		if (!client || testingConnection) {
			return
		}
		testingConnection = true
		try {
			await client.testConnection(draftSettings)
			settingsDirty = false
			draftSettings = { ...settings }
		} catch (_error) {
		} finally {
			testingConnection = false
		}
	}

	async function sendPrompt(payload: ComposerSubmitPayload) {
		if (!client || sending) {
			return
		}
		if (!settings.token) {
			settingsOpen = true
		}
		await client.sendPrompt(payload.text, payload.attachments)
		shouldStickToBottom = true
		await tick()
		scrollMessagesToBottom()
	}

	async function sendVoiceTurn(payload: ComposerVoicePayload) {
		if (!client || sending) {
			return
		}
		if (!settings.token) {
			settingsOpen = true
		}
		await client.sendVoiceTurn(payload)
		shouldStickToBottom = true
		await tick()
		scrollMessagesToBottom()
	}

	async function startBrowserSpeechRecognition(language: string) {
		if (!client) {
			throw new Error('Voice mode is not connected.')
		}
		await client.startBrowserSpeechRecognition(language)
	}

	async function stopBrowserSpeechRecognition() {
		if (!client) {
			throw new Error('Voice mode is not connected.')
		}
		return client.stopBrowserSpeechRecognition()
	}

	async function cancelBrowserSpeechRecognition() {
		await client?.cancelBrowserSpeechRecognition()
	}

	async function startBrowserAudioCapture(mimeType?: string) {
		if (!client) {
			throw new Error('Voice mode is not connected.')
		}
		await client.startBrowserAudioCapture(mimeType)
	}

	async function stopBrowserAudioCapture(): Promise<PageAudioResult> {
		if (!client) {
			throw new Error('Voice mode is not connected.')
		}
		return client.stopBrowserAudioCapture()
	}

	async function cancelBrowserAudioCapture() {
		await client?.cancelBrowserAudioCapture()
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
				const detailText = detailOnlyText(message)
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
				thinkingText: detailRun.map(detailOnlyText).filter(Boolean).join('\n\n---\n\n'),
				timestamp: last.timestamp || first.timestamp,
				conversationId: first.conversationId,
				attachments: attachments.length ? attachments : undefined
			})
			detailRun = []
		}

		for (const message of sourceMessages) {
			if (detailOnlyText(message)) {
				detailRun = [...detailRun, message]
				continue
			}
			flushDetails()
			compacted.push(message)
		}
		flushDetails()
		return compacted
	}

	function displayConversationGroups(sourceGroups: ConversationGroup[]): ConversationGroup[] {
		return sourceGroups
			.map((group) => ({ ...group, messages: displayMessages(group.messages) }))
			.filter((group) => group.messages.length)
	}

	function detailRunId(message: ChatMessage): string {
		return `${message.id}-detail-run`
	}

	function detailOnlyText(message: ChatMessage): string {
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

	function groupLabel(group: ConversationGroup): string {
		const time = group.createdAt || group.updatedAt || group.messages[0]?.timestamp
		if (!time) {
			return group.current ? 'Current session' : 'Conversation'
		}
		const date = new Date(time)
		if (Number.isNaN(date.getTime())) {
			return group.current ? 'Current session' : 'Conversation'
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
			<p class="truncate text-xs font-bold text-stone-800">{tab?.title || 'No active tab'}</p>
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
		</div>

		<Button
			variant="ghost"
			size="icon"
			aria-label="Settings"
			title="Settings"
			onclick={() => (settingsOpen = !settingsOpen)}
		>
			<Settings class="size-4" />
		</Button>
	</header>

	{#if settingsOpen}
		<section
			class="grid gap-3 border-b border-stone-200 bg-[#fbfcfa] px-3 py-3"
			aria-label="Settings"
		>
			<label class="grid gap-1.5 text-[11px] font-bold text-stone-500" for="base-url">
				<span class="flex items-center gap-1.5"><ExternalLink class="size-3" />Gateway URL</span>
				<Input
					id="base-url"
					type="url"
					spellcheck={false}
					bind:value={draftSettings.baseUrl}
					oninput={markSettingsDirty}
				/>
			</label>

			<label class="grid gap-1.5 text-[11px] font-bold text-stone-500" for="token">
				<span class="flex items-center gap-1.5"><KeyRound class="size-3" />Bearer token</span>
				<Textarea
					id="token"
					rows={4}
					spellcheck={false}
					bind:value={draftSettings.token}
					oninput={markSettingsDirty}
				/>
			</label>

			<div class="flex gap-2">
				<Button size="sm" disabled={savingSettings} onclick={saveSettings}>
					{#if savingSettings}
						<LoaderCircle class="size-3.5 animate-spin" />
					{:else}
						<Save class="size-3.5" />
					{/if}
					Save
				</Button>
				<Button variant="outline" size="sm" disabled={testingConnection} onclick={testConnection}>
					{#if testingConnection}
						<LoaderCircle class="size-3.5 animate-spin" />
					{:else}
						<PlugZap class="size-3.5" />
					{/if}
					Test
				</Button>
			</div>
		</section>
	{/if}

	<main
		bind:this={messagesElement}
		class="scrollbar-slim flex min-h-0 w-full flex-1 flex-col gap-3 overflow-y-auto px-3 py-4"
		onscroll={handleMessagesScroll}
	>
		{#if messages.length === 0}
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
				<div class="text-xs font-semibold text-stone-700">{syncing ? 'Syncing' : 'Ready'}</div>
				<div class="max-w-full truncate text-[11px]">{tab?.url || tab?.title || status}</div>
			</div>
		{:else}
			{#if showHistoryControl}
				<div class="flex justify-center">
					<Button
						variant="outline"
						size="xs"
						class="bg-white/80 text-stone-600 shadow-sm"
						disabled={loadingPrevious || historyLoadInFlight}
						onclick={loadPreviousConversations}
					>
						{#if loadingPrevious || historyLoadInFlight}
							<LoaderCircle class="size-3 animate-spin" />
						{:else}
							<History class="size-3" />
						{/if}
						Load history
					</Button>
				</div>
			{/if}

			{#each visibleConversationGroups as group (group.id)}
				<section class="grid w-full gap-2">
					{#if visibleConversationGroups.length > 1}
						<div
							class="flex items-center justify-center gap-2 py-1 text-[10px] font-semibold text-stone-400"
						>
							<span class="h-px flex-1 bg-stone-200"></span>
							<span class="max-w-[70%] truncate">{groupLabel(group)}</span>
							{#if group.status}
								<span class="rounded-full bg-stone-100 px-1.5 py-0.5 text-stone-500">
									{group.status}
								</span>
							{/if}
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

	<footer class="border-t border-stone-200 bg-[#f6f8f5]/90 p-2.5 backdrop-blur">
		<ChatComposer
			placeholder={settings.token ? 'Message Anda' : 'Paste token in Settings'}
			disabled={sending}
			{sending}
			working={isBusy}
			voiceAvailable={voiceCapabilities.transcription.length > 0}
			{voiceCapabilities}
			onSend={sendPrompt}
			onVoiceSend={sendVoiceTurn}
			onBrowserSpeechStart={startBrowserSpeechRecognition}
			onBrowserSpeechStop={stopBrowserSpeechRecognition}
			onBrowserSpeechCancel={cancelBrowserSpeechRecognition}
			onBrowserAudioStart={startBrowserAudioCapture}
			onBrowserAudioStop={stopBrowserAudioCapture}
			onBrowserAudioCancel={cancelBrowserAudioCapture}
		/>
	</footer>
</div>
