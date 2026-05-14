<script lang="ts">
	import {
		AndaSidePanelClient,
		type ChatMessage,
		type ChromeTabInfo,
		type SettingsState
	} from '$lib/anda/client'
	import { Button } from '$lib/components/ui/button/index.js'
	import { Input } from '$lib/components/ui/input/index.js'
	import { Textarea } from '$lib/components/ui/textarea/index.js'
	import {
		Bot,
		CircleAlert,
		CircleCheck,
		ExternalLink,
		KeyRound,
		LoaderCircle,
		PanelRightOpen,
		PlugZap,
		Radio,
		Save,
		SendHorizontal,
		Settings
	} from '@lucide/svelte'
	import { onMount, tick } from 'svelte'

	let client: AndaSidePanelClient | null = null
	let settings = $state<SettingsState>({ baseUrl: 'http://127.0.0.1:8042', token: '' })
	let draftSettings = $state<SettingsState>({ baseUrl: 'http://127.0.0.1:8042', token: '' })
	let tab = $state<ChromeTabInfo | null>(null)
	let messages = $state<ChatMessage[]>([])
	let status = $state('starting')
	let sending = $state(false)
	let settingsOpen = $state(false)
	let settingsDirty = $state(false)
	let savingSettings = $state(false)
	let testingConnection = $state(false)
	let prompt = $state('')
	let messagesElement: HTMLElement | null = null
	let promptElement: HTMLTextAreaElement | null = null

	let isBusy = $derived(['sending', 'submitted', 'working'].includes(status))
	let canSend = $derived(Boolean(prompt.trim()) && !sending)

	onMount(() => {
		client = new AndaSidePanelClient((snapshot) => {
			console.info('State snapshot:', snapshot)
			settings = snapshot.settings
			if (!settingsDirty) {
				draftSettings = { ...snapshot.settings }
			}
			tab = snapshot.tab
			messages = snapshot.messages
			status = snapshot.status
			sending = snapshot.sending
			if (!snapshot.settings.token) {
				settingsOpen = true
			}
		})

		client.init().catch((error) => {
			status = 'extension unavailable'
			messages = [
				{
					id: 'startup-error',
					role: 'system',
					text: error instanceof Error ? error.message : String(error)
				}
			]
			settingsOpen = true
		})

		return () => client?.destroy()
	})

	$effect(() => {
		messages.length
		void tick().then(scrollMessagesToBottom)
	})

	function scrollMessagesToBottom() {
		if (messagesElement) {
			messagesElement.scrollTop = messagesElement.scrollHeight
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

	async function sendPrompt() {
		if (!client || !prompt.trim() || sending) {
			return
		}
		if (!settings.token) {
			settingsOpen = true
			await client.sendPrompt(prompt)
			return
		}
		const text = prompt
		prompt = ''
		resizePrompt()
		await client.sendPrompt(text)
	}

	function handlePromptKeydown(event: KeyboardEvent) {
		if (event.key === 'Enter' && !event.shiftKey) {
			event.preventDefault()
			void sendPrompt()
		}
	}

	function resizePrompt() {
		void tick().then(() => {
			if (!promptElement) {
				return
			}
			promptElement.style.height = 'auto'
			promptElement.style.height = `${Math.min(promptElement.scrollHeight, 140)}px`
		})
	}

	function statusIconClass() {
		if (['connected', 'ready', 'idle'].includes(status)) {
			return 'text-emerald-700'
		}
		if (
			['request failed', 'poll failed', 'connection failed', 'extension unavailable'].includes(
				status
			)
		) {
			return 'text-amber-700'
		}
		return 'text-stone-500'
	}

	function messageBubbleClass(role: ChatMessage['role']) {
		if (role === 'user') {
			return 'ml-auto border-sky-200 bg-sky-50 text-slate-950'
		}
		if (role === 'system') {
			return 'mr-auto border-amber-200 bg-amber-50 text-amber-950'
		}
		return 'mr-auto border-stone-200 bg-white text-stone-950'
	}
</script>

<svelte:head>
	<title>Anda Bot</title>
</svelte:head>

<div class="flex h-screen min-w-80 flex-col overflow-hidden bg-[#f6f8f5] text-stone-950">
	<header
		class="flex items-center justify-between gap-3 border-b border-stone-200 bg-white px-3 py-3"
	>
		<div class="flex min-w-0 items-center gap-3">
			<div class="min-w-0">
				<div class="mt-1 flex min-w-0 items-center gap-1.5 text-[11px] text-stone-500">
					{#if isBusy}
						<LoaderCircle class="size-3 shrink-0 animate-spin text-emerald-700" />
					{:else if ['request failed', 'poll failed', 'connection failed', 'extension unavailable'].includes(status)}
						<CircleAlert class={`size-3 shrink-0 ${statusIconClass()}`} />
					{:else}
						<Radio class={`size-3 shrink-0 ${statusIconClass()}`} />
					{/if}
					<span class="truncate">{status}</span>
				</div>
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

	<section class="border-b border-emerald-900/10 bg-emerald-50/70 px-3 py-2" aria-live="polite">
		<div class="flex min-w-0 items-center gap-2 text-xs font-bold text-stone-800">
			<PanelRightOpen class="size-3.5 shrink-0 text-emerald-800" />
			<span class="truncate">{tab?.title || 'No active tab'}</span>
		</div>
		<div class="mt-1 truncate pl-5 text-[11px] text-stone-500">{tab?.url || ''}</div>
	</section>

	<main
		bind:this={messagesElement}
		class="scrollbar-slim flex min-h-0 flex-1 flex-col gap-2 overflow-y-auto px-3 py-4"
	>
		{#if messages.length === 0}
			<div class="m-auto grid max-w-56 place-items-center gap-2 text-center text-stone-500">
				<div
					class="grid size-10 place-items-center rounded-md border border-stone-200 bg-white shadow-sm"
				>
					<Bot class="size-5 text-emerald-800" />
				</div>
				<div class="text-xs font-semibold text-stone-700">Ready</div>
				<div class="max-w-full truncate text-[11px]">{tab?.title || status}</div>
			</div>
		{:else}
			{#each messages as message (message.id)}
				<article
					class="grid max-w-[92%] gap-1 {message.role === 'user' ? 'self-end' : 'self-start'}"
				>
					<div
						class={`rounded-lg border px-3 py-2 text-[13px] leading-relaxed wrap-break-word whitespace-pre-wrap shadow-sm ${messageBubbleClass(message.role)}`}
					>
						{message.text}
					</div>
				</article>
			{/each}
		{/if}
	</main>

	<form
		class="grid grid-cols-[1fr_auto] gap-2 border-t border-stone-200 bg-white p-2.5"
		onsubmit={(event) => {
			event.preventDefault()
			void sendPrompt()
		}}
	>
		<textarea
			bind:this={promptElement}
			bind:value={prompt}
			rows="1"
			placeholder={settings.token ? 'Message Anda' : 'Paste token in Settings'}
			spellcheck="true"
			class="max-h-35 min-h-10 resize-none rounded-md border border-stone-300 bg-white px-3 py-2 text-[13px] leading-5 text-stone-950 shadow-sm transition-colors outline-none placeholder:text-stone-400 focus:border-emerald-700 focus:ring-2 focus:ring-emerald-700/15"
			disabled={sending}
			onkeydown={handlePromptKeydown}
			oninput={resizePrompt}
		></textarea>
		<Button type="submit" size="icon" class="mt-auto size-10" disabled={!canSend} aria-label="Send">
			{#if sending}
				<LoaderCircle class="size-4 animate-spin" />
			{:else if status === 'connected'}
				<CircleCheck class="size-4" />
			{:else}
				<SendHorizontal class="size-4" />
			{/if}
		</Button>
	</form>
</div>
