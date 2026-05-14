type SettingsState = {
	baseUrl: string
	token: string
}

type StorageState = Partial<SettingsState> & {
	browserSessionId?: string
}

type ChromeTabInfo = {
	id?: number
	windowId?: number
	index?: number
	active?: boolean
	highlighted?: boolean
	pinned?: boolean
	status?: string
	title?: string
	url?: string
}

type BrowserActionArgs = {
	action?: string
	url?: string
	selector?: string
	text?: string
	key?: string
	amount?: number
	tab_id?: number
	window_id?: number
	active?: boolean
	include_links?: boolean
	include_forms?: boolean
	include_data_url?: boolean
}

type BrowserCommand = {
	session: string
	request_id: number
	args?: BrowserActionArgs
}

type BrowserActionResult = unknown

type ExtensionMessage = {
	type?: string
	settings?: SettingsState
	method?: string
	params?: unknown[]
}

type ExtensionResponse =
	| { ok: true; result?: unknown; status?: string }
	| { ok: false; error: string; status?: string }

type RpcResponseMessage = {
	id?: number
	method?: string
	params?: unknown
	result?: unknown
	error?: string
}

type PendingRpc = {
	resolve: (value: unknown) => void
	reject: (error: Error) => void
	timeout: ReturnType<typeof setTimeout>
}

interface ChromeEvent<Listener extends (...args: never[]) => void> {
	addListener(listener: Listener): void
	removeListener(listener: Listener): void
}

interface ChromeApi {
	runtime: {
		onInstalled: ChromeEvent<() => void>
		onStartup: ChromeEvent<() => void>
		onMessage: {
			addListener(
				listener: (
					message: ExtensionMessage,
					sender: unknown,
					sendResponse: (response: ExtensionResponse) => void
				) => boolean | void
			): void
		}
	}
	extension?: {
		inIncognitoContext?: boolean
	}
	action: {
		onClicked: ChromeEvent<(tab: ChromeTabInfo) => void>
	}
	sidePanel?: {
		setPanelBehavior?(options: { openPanelOnActionClick: boolean }): Promise<void>
		open?(options: { tabId?: number; windowId?: number }): Promise<void>
	}
	storage: {
		local: {
			get(keys: string[]): Promise<StorageState>
			set(items: StorageState): Promise<void>
		}
	}
	tabs: {
		query(queryInfo: {
			active?: boolean
			lastFocusedWindow?: boolean
			currentWindow?: boolean
			windowId?: number
		}): Promise<ChromeTabInfo[]>
		get(tabId: number): Promise<ChromeTabInfo>
		create(createProperties: {
			url?: string
			active?: boolean
			windowId?: number
			index?: number
		}): Promise<ChromeTabInfo>
		remove(tabIds: number | number[]): Promise<void>
		update(
			tabId: number,
			updateProperties: { url?: string; active?: boolean }
		): Promise<ChromeTabInfo>
		captureVisibleTab(windowId: number | undefined, options: { format: 'png' }): Promise<string>
		onActivated: ChromeEvent<(activeInfo: { tabId: number; windowId: number }) => void>
		onUpdated: ChromeEvent<
			(tabId: number, changeInfo: { title?: string; url?: string }, tab: ChromeTabInfo) => void
		>
	}
	windows?: {
		update(windowId: number, updateInfo: { focused?: boolean }): Promise<unknown>
	}
	scripting: {
		executeScript<Result>(details: {
			target: { tabId: number }
			func: (args: BrowserActionArgs) => Result
			args: [BrowserActionArgs]
		}): Promise<Array<{ result: Result }>>
	}
}

const defaultSettings: SettingsState = {
	baseUrl: 'http://127.0.0.1:8042',
	token: ''
}

const keepAliveIntervalMs = 20_000
const reconnectDelayMs = 3_000
const rpcTimeoutMs = 30 * 60 * 1000
const browserSessionStorageKey = 'browserSessionId'

const chromeApi = getChromeApi()
let currentSettings: SettingsState = { ...defaultSettings }
let socket: WebSocket | null = null
let socketKey = ''
let opening: Promise<void> | null = null
let openingReject: ((error: Error) => void) | null = null
let reconnectTimer: ReturnType<typeof setTimeout> | null = null
let keepAliveTimer: ReturnType<typeof setInterval> | null = null
let nextMessageId = 1
let status = 'starting'
const pending = new Map<number, PendingRpc>()

chromeApi.runtime.onInstalled.addListener(() => {
	if (chromeApi.sidePanel?.setPanelBehavior) {
		chromeApi.sidePanel.setPanelBehavior({ openPanelOnActionClick: true }).catch(() => {})
	}
	void loadSettingsAndConnect()
})

chromeApi.runtime.onStartup.addListener(() => {
	void loadSettingsAndConnect()
})

chromeApi.action.onClicked.addListener((tab) => {
	void openSidePanel(tab)
})

chromeApi.tabs.onActivated.addListener(() => {
	void registerBrowserSession().catch(() => undefined)
})

chromeApi.tabs.onUpdated.addListener((_tabId, changeInfo) => {
	if (changeInfo.title || changeInfo.url) {
		void registerBrowserSession().catch(() => undefined)
	}
})

chromeApi.runtime.onMessage.addListener((message, _sender, sendResponse) => {
	void handleExtensionMessage(message)
		.then(sendResponse)
		.catch((error) => {
			sendResponse({ ok: false, error: errorToMessage(error), status })
		})
	return true
})

void loadSettingsAndConnect()

async function handleExtensionMessage(message: ExtensionMessage): Promise<ExtensionResponse> {
	if (message.settings) {
		currentSettings = normalizeSettings(message.settings)
	}

	switch (message.type) {
		case 'anda_rpc': {
			if (!message.method) {
				throw new Error('missing RPC method')
			}
			const result = await sendRpc(message.method, message.params || [], currentSettings)
			return { ok: true, result, status }
		}
		case 'anda_settings_changed': {
			await chromeApi.storage.local.set(currentSettings)
			if (currentSettings.token) {
				await ensureSocket(currentSettings)
				await registerBrowserSession(currentSettings)
			} else {
				closeSocket('missing bearer token')
				status = 'ready'
			}
			return { ok: true, status }
		}
		case 'anda_register': {
			const session = await registerBrowserSession(currentSettings)
			return { ok: true, result: { session }, status }
		}
		case 'anda_status': {
			return { ok: true, result: { status }, status }
		}
		default:
			throw new Error(`unsupported extension message: ${message.type || 'unknown'}`)
	}
}

async function loadSettingsAndConnect(): Promise<void> {
	currentSettings = await loadSettings()
	if (!currentSettings.token) {
		status = 'ready'
		return
	}

	try {
		await ensureSocket(currentSettings)
		await registerBrowserSession(currentSettings)
	} catch (_error) {
		scheduleReconnect(currentSettings)
	}
}

async function loadSettings(): Promise<SettingsState> {
	const saved = await chromeApi.storage.local.get(['baseUrl', 'token'])
	return normalizeSettings({
		baseUrl: saved.baseUrl || defaultSettings.baseUrl,
		token: saved.token || ''
	})
}

async function openSidePanel(tab: ChromeTabInfo): Promise<void> {
	if (!chromeApi.sidePanel?.open) {
		return
	}

	try {
		if (tab.id) {
			await chromeApi.sidePanel.open({ tabId: tab.id })
		} else if (tab.windowId) {
			await chromeApi.sidePanel.open({ windowId: tab.windowId })
		}
	} catch (_error) {}
}

async function sendRpc(
	method: string,
	params: unknown[],
	settings: SettingsState
): Promise<unknown> {
	await ensureSocket(settings)
	const activeSocket = socket
	if (!activeSocket || activeSocket.readyState !== WebSocket.OPEN) {
		throw new Error('WebSocket is not connected')
	}

	const id = nextMessageId++
	const payload = JSON.stringify({ id, method, params })

	return new Promise((resolve, reject) => {
		const timeout = setTimeout(() => {
			pending.delete(id)
			reject(new Error(`RPC ${method} timed out`))
		}, rpcTimeoutMs)

		pending.set(id, { resolve, reject, timeout })
		try {
			activeSocket.send(payload)
		} catch (error) {
			pending.delete(id)
			clearTimeout(timeout)
			reject(error instanceof Error ? error : new Error(String(error)))
		}
	})
}

async function ensureSocket(settings: SettingsState): Promise<void> {
	const normalized = normalizeSettings(settings)
	if (!normalized.token) {
		throw new Error('missing bearer token')
	}

	const key = connectionKey(normalized)
	if (socket && socket.readyState === WebSocket.OPEN && socketKey === key) {
		status = 'connected'
		return
	}
	if (opening && socketKey === key) {
		return opening
	}

	closeSocket('reconnecting')
	socketKey = key
	currentSettings = normalized
	status = 'connecting'

	opening = new Promise((resolve, reject) => {
		let settled = false
		const ws = new WebSocket(websocketUrl(normalized))
		socket = ws

		const fail = (error: Error) => {
			openingReject = null
			if (!settled) {
				settled = true
				reject(error)
			}
		}
		openingReject = fail

		const openTimeout = setTimeout(() => {
			fail(new Error('WebSocket connection timed out'))
			ws.close()
		}, 15_000)

		ws.onopen = () => {
			clearTimeout(openTimeout)
			settled = true
			openingReject = null
			opening = null
			status = 'connected'
			startKeepAlive()
			resolve()
			console.info('WebSocket connected')
		}

		ws.onmessage = (event) => {
			void handleSocketMessage(event.data).catch((error) => {
				console.warn('Anda WebSocket message failed', error)
			})
		}

		ws.onerror = () => {
			status = 'connection failed'
		}

		ws.onclose = () => {
			clearTimeout(openTimeout)
			if (socket === ws) {
				socket = null
				opening = null
				stopKeepAlive()
				rejectPending('WebSocket connection closed')
				status = 'disconnected'
				scheduleReconnect(normalized)
			}
			fail(new Error('WebSocket connection closed'))
		}
	})

	return opening
}

function closeSocket(reason: string): void {
	if (reconnectTimer) {
		clearTimeout(reconnectTimer)
		reconnectTimer = null
	}
	stopKeepAlive()
	if (openingReject) {
		openingReject(new Error(reason))
		openingReject = null
	}
	if (socket) {
		const oldSocket = socket
		socket = null
		oldSocket.onclose = null
		oldSocket.close()
	}
	opening = null
	rejectPending(reason)
}

function scheduleReconnect(settings: SettingsState): void {
	if (!settings.token || reconnectTimer) {
		return
	}
	reconnectTimer = setTimeout(() => {
		reconnectTimer = null
		void ensureSocket(settings)
			.then(() => registerBrowserSession(settings))
			.catch(() => scheduleReconnect(settings))
	}, reconnectDelayMs)
}

function startKeepAlive(): void {
	stopKeepAlive()
	keepAliveTimer = setInterval(() => {
		if (socket?.readyState === WebSocket.OPEN) {
			socket.send(JSON.stringify({ method: 'ping' }))
		}
	}, keepAliveIntervalMs)
}

function stopKeepAlive(): void {
	if (keepAliveTimer) {
		clearInterval(keepAliveTimer)
		keepAliveTimer = null
	}
}

function rejectPending(reason: string): void {
	for (const [id, entry] of pending) {
		clearTimeout(entry.timeout)
		entry.reject(new Error(reason))
		pending.delete(id)
	}
}

async function handleSocketMessage(data: unknown): Promise<void> {
	if (typeof data !== 'string') {
		return
	}

	const message = JSON.parse(data) as RpcResponseMessage
	if (message.method === 'browser_action') {
		await handleBrowserActionRequest(message)
		return
	}

	if (typeof message.id !== 'number') {
		return
	}

	const entry = pending.get(message.id)
	if (!entry) {
		return
	}
	pending.delete(message.id)
	clearTimeout(entry.timeout)

	if (message.error) {
		entry.reject(new Error(message.error))
	} else {
		entry.resolve(message.result)
	}
}

async function handleBrowserActionRequest(message: RpcResponseMessage): Promise<void> {
	const command = message.params as BrowserCommand
	const id = typeof message.id === 'number' ? message.id : command.request_id
	let result: Record<string, unknown>

	try {
		const value = await executeBrowserAction(command)
		result = { ok: true, value }
	} catch (error) {
		result = { ok: false, value: null, error: errorToMessage(error) }
	}

	if (socket?.readyState === WebSocket.OPEN) {
		socket.send(
			JSON.stringify({
				id,
				session: command.session,
				result
			})
		)
	}
}

async function registerBrowserSession(settings: SettingsState = currentSettings): Promise<string> {
	const session = await browserSession()
	if (!settings.token) {
		return session
	}
	const tab = await activeTab()

	await sendRpc(
		'browser_register',
		[
			{
				session,
				tab_id: tab?.id,
				url: tab?.url || '',
				title: tab?.title || ''
			}
		],
		settings
	)

	return session
}

async function executeBrowserAction(command: BrowserCommand): Promise<BrowserActionResult> {
	const args = command.args || {}

	if (args.action === 'list_tabs') {
		return listTabs(args)
	}

	if (args.action === 'open_tab') {
		const tab = await chromeApi.tabs.create({
			url: normalizeOptionalText(args.url),
			active: args.active ?? true,
			windowId: positiveInteger(args.window_id) || undefined
		})
		void registerBrowserSession().catch(() => undefined)
		return { opened: true, tab: tabSummary(tab) }
	}

	if (args.action === 'switch_tab') {
		const tabId = requirePositiveInteger(args.tab_id, 'switch_tab requires tab_id')
		const tab = await activateTab(tabId)
		void registerBrowserSession().catch(() => undefined)
		return { switched: true, tab: tabSummary(tab) }
	}

	if (args.action === 'close_tab') {
		const tabId = requirePositiveInteger(args.tab_id, 'close_tab requires tab_id')
		await chromeApi.tabs.remove(tabId)
		void registerBrowserSession().catch(() => undefined)
		return { closed: true, tab_id: tabId }
	}

	if (args.action === 'launch_browser') {
		return { launched: false, connected: true, reason: 'browser is already running' }
	}

	if (args.action === 'navigate') {
		if (!args.url) {
			throw new Error('navigate requires url')
		}
		const tab = await tabForAction(args)
		const active = args.active ?? true
		if (!tab?.id) {
			const created = await chromeApi.tabs.create({
				url: args.url,
				active,
				windowId: positiveInteger(args.window_id) || undefined
			})
			void registerBrowserSession().catch(() => undefined)
			return { navigated: true, url: args.url, tab: tabSummary(created) }
		}
		const updated = await chromeApi.tabs.update(tab.id, { url: args.url, active })
		if (active) {
			await focusWindow(updated.windowId).catch(() => undefined)
		}
		void registerBrowserSession().catch(() => undefined)
		return { navigated: true, url: args.url, tab: tabSummary(updated) }
	}

	const tab = await tabForAction(args)
	const tabId = tab?.id
	if (!tabId) {
		throw new Error('no target tab')
	}

	if (args.action === 'screenshot') {
		const activeTab = await activateTab(tabId).catch(() => tab)
		const dataUrl = await chromeApi.tabs.captureVisibleTab(activeTab?.windowId || tab?.windowId, {
			format: 'png'
		})
		return {
			captured: true,
			tab: tabSummary(activeTab || tab),
			mime_type: 'image/png',
			size: dataUrl.length,
			data_url: args.include_data_url ? dataUrl : undefined
		}
	}

	const [execution] = await chromeApi.scripting.executeScript({
		target: { tabId },
		func: pageActionDispatcher,
		args: [args]
	})
	return execution ? execution.result : null
}

async function listTabs(args: BrowserActionArgs): Promise<BrowserActionResult> {
	const windowId = positiveInteger(args.window_id)
	const queryInfo = windowId ? { windowId } : {}
	const tabs = await chromeApi.tabs.query(queryInfo)
	return {
		tabs: tabs.map(tabSummary),
		active_tab_id: tabs.find((tab) => tab.active)?.id || null
	}
}

async function tabForAction(args: BrowserActionArgs): Promise<ChromeTabInfo | null> {
	const tabId = positiveInteger(args.tab_id)
	if (tabId) {
		return chromeApi.tabs.get(tabId)
	}
	return activeTab()
}

async function activeTab(): Promise<ChromeTabInfo | null> {
	const [tab] = await chromeApi.tabs.query({ active: true, lastFocusedWindow: true })
	if (tab) {
		return tab
	}
	const [currentWindowTab] = await chromeApi.tabs.query({ active: true, currentWindow: true })
	if (currentWindowTab) {
		return currentWindowTab
	}
	const [fallbackTab] = await chromeApi.tabs.query({})
	return fallbackTab || null
}

async function activateTab(tabId: number): Promise<ChromeTabInfo> {
	const tab = await chromeApi.tabs.update(tabId, { active: true })
	await focusWindow(tab.windowId).catch(() => undefined)
	return tab
}

async function focusWindow(windowId?: number): Promise<void> {
	if (typeof windowId === 'number' && chromeApi.windows?.update) {
		await chromeApi.windows.update(windowId, { focused: true })
	}
}

function tabSummary(tab: ChromeTabInfo | null | undefined): Record<string, unknown> | null {
	if (!tab) {
		return null
	}
	return {
		id: tab.id,
		window_id: tab.windowId,
		index: tab.index,
		active: tab.active,
		highlighted: tab.highlighted,
		pinned: tab.pinned,
		status: tab.status,
		title: tab.title || '',
		url: tab.url || ''
	}
}

function positiveInteger(value: unknown): number | null {
	return typeof value === 'number' && Number.isInteger(value) && value > 0 ? value : null
}

function requirePositiveInteger(value: unknown, message: string): number {
	const integer = positiveInteger(value)
	if (!integer) {
		throw new Error(message)
	}
	return integer
}

function normalizeOptionalText(value: unknown): string | undefined {
	return typeof value === 'string' && value.trim() ? value.trim() : undefined
}

function pageActionDispatcher(args: BrowserActionArgs): BrowserActionResult {
	const maxText = 12000

	function truncate(value: unknown, limit = maxText): string {
		const text = String(value || '')
		return text.length > limit
			? `${text.slice(0, limit)}\n[truncated ${text.length - limit} chars]`
			: text
	}

	function cssEscape(value: string): string {
		if (window.CSS && CSS.escape) {
			return CSS.escape(value)
		}
		return String(value).replace(/[^a-zA-Z0-9_-]/g, '\\$&')
	}

	function visible(element: Element): boolean {
		const style = window.getComputedStyle(element)
		const rect = element.getBoundingClientRect()
		return (
			style.visibility !== 'hidden' && style.display !== 'none' && rect.width > 0 && rect.height > 0
		)
	}

	function cssPath(element: Element | null): string {
		if (!element) {
			return ''
		}
		if (element.id) {
			return `#${cssEscape(element.id)}`
		}

		const parts: string[] = []
		let current: Element | null = element
		while (current && current.nodeType === Node.ELEMENT_NODE && parts.length < 5) {
			let part = current.nodeName.toLowerCase()
			if (current.classList.length) {
				part += `.${Array.from(current.classList).slice(0, 2).map(cssEscape).join('.')}`
			}
			const parent: Element | null = current.parentElement
			if (parent) {
				const currentNodeName = current.nodeName
				const siblings = Array.from(parent.children).filter(
					(sibling: Element) => sibling.nodeName === currentNodeName
				)
				if (siblings.length > 1) {
					part += `:nth-of-type(${siblings.indexOf(current) + 1})`
				}
			}
			parts.unshift(part)
			current = parent
		}
		return parts.join(' > ')
	}

	function queryRequired(selector?: string): Element {
		if (!selector) {
			throw new Error('selector is required')
		}
		const element = document.querySelector(selector)
		if (!element) {
			throw new Error(`selector not found: ${selector}`)
		}
		return element
	}

	function elementLabel(element: Element): string {
		return truncate(
			element.getAttribute('aria-label') ||
				element.getAttribute('title') ||
				element.getAttribute('placeholder') ||
				(element instanceof HTMLElement ? element.innerText : '') ||
				('value' in element ? String(element.value) : '') ||
				element.textContent ||
				element.tagName,
			240
		)
	}

	switch (args.action) {
		case 'snapshot': {
			const links = args.include_links
				? Array.from(document.querySelectorAll('a[href]'))
						.filter(visible)
						.slice(0, 80)
						.map((element) => ({
							text: elementLabel(element),
							href: element instanceof HTMLAnchorElement ? element.href : '',
							selector: cssPath(element)
						}))
				: []
			const forms = args.include_forms
				? Array.from(document.querySelectorAll("input, textarea, select, button, [role='button']"))
						.filter(visible)
						.slice(0, 120)
						.map((element) => ({
							tag: element.tagName.toLowerCase(),
							type: element.getAttribute('type') || element.getAttribute('role') || null,
							name: element.getAttribute('name') || null,
							label: elementLabel(element),
							selector: cssPath(element)
						}))
				: []
			return {
				url: location.href,
				title: document.title,
				selection: String(window.getSelection ? window.getSelection() : ''),
				viewport: { width: window.innerWidth, height: window.innerHeight },
				scroll: {
					x: window.scrollX,
					y: window.scrollY,
					max_y: document.documentElement.scrollHeight
				},
				text: truncate(document.body ? document.body.innerText : ''),
				links,
				forms
			}
		}
		case 'extract_text': {
			const element = args.selector ? queryRequired(args.selector) : document.body
			return {
				selector: args.selector || 'body',
				text: truncate(
					element instanceof HTMLElement
						? element.innerText || element.textContent
						: element?.textContent
				)
			}
		}
		case 'read_selection': {
			return { selection: String(window.getSelection ? window.getSelection() : '') }
		}
		case 'click': {
			const element = queryRequired(args.selector)
			element.scrollIntoView({ block: 'center', inline: 'center' })
			if (element instanceof HTMLElement) {
				element.click()
			} else {
				throw new Error(`selector is not clickable: ${args.selector}`)
			}
			return { clicked: true, selector: args.selector, label: elementLabel(element) }
		}
		case 'type_text': {
			const element = queryRequired(args.selector)
			element.scrollIntoView({ block: 'center', inline: 'center' })
			if (element instanceof HTMLElement) {
				element.focus()
			}
			if (element instanceof HTMLElement && element.isContentEditable) {
				element.textContent = args.text || ''
			} else if (
				element instanceof HTMLInputElement ||
				element instanceof HTMLTextAreaElement ||
				element instanceof HTMLSelectElement
			) {
				element.value = args.text || ''
			} else {
				throw new Error(`selector is not editable: ${args.selector}`)
			}
			element.dispatchEvent(
				new InputEvent('input', { bubbles: true, inputType: 'insertText', data: args.text || '' })
			)
			element.dispatchEvent(new Event('change', { bubbles: true }))
			return { typed: true, selector: args.selector, length: String(args.text || '').length }
		}
		case 'press_key': {
			const target = document.activeElement || document.body
			const key = args.key || 'Enter'
			target.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true }))
			target.dispatchEvent(new KeyboardEvent('keyup', { key, bubbles: true }))
			return { pressed: true, key }
		}
		case 'scroll': {
			const amount =
				typeof args.amount === 'number' && Number.isFinite(args.amount) ? args.amount : 700
			window.scrollBy({ top: amount, behavior: 'smooth' })
			return { scrolled: true, amount, scroll_y: window.scrollY }
		}
		default:
			throw new Error(`unsupported browser action: ${args.action}`)
	}
}

function getChromeApi(): ChromeApi {
	const chromeApi = (globalThis as typeof globalThis & { chrome?: ChromeApi }).chrome
	if (!chromeApi?.runtime || !chromeApi.storage?.local || !chromeApi.tabs || !chromeApi.scripting) {
		throw new Error('Chrome extension APIs are unavailable.')
	}
	return chromeApi
}

function websocketUrl(settings: SettingsState): string {
	const base = trimTrailingSlash(settings.baseUrl)
	const wsBase = base.replace(/^http:/i, 'ws:').replace(/^https:/i, 'wss:')
	return `${wsBase}/ws/engine/default?token=${encodeURIComponent(settings.token)}`
}

function connectionKey(settings: SettingsState): string {
	return `${trimTrailingSlash(settings.baseUrl)}\n${settings.token}`
}

async function browserSession(): Promise<string> {
	const saved = await chromeApi.storage.local.get([browserSessionStorageKey])
	let id = saved.browserSessionId || '0'
	if (parseInt(id, 10) < 1000) {
		id = Date.now().toString()
		await chromeApi.storage.local.set({ browserSessionId: id })
	}
	return `browser:${browserSessionScope()}:${id}`
}

function browserSessionScope(): string {
	return chromeApi.extension?.inIncognitoContext ? 'incognito' : 'chrome'
}

function normalizeSettings(settings: SettingsState): SettingsState {
	return {
		baseUrl: trimTrailingSlash(settings.baseUrl.trim() || defaultSettings.baseUrl),
		token: settings.token.trim()
	}
}

function trimTrailingSlash(value: string): string {
	return String(value || '').replace(/\/+$/, '')
}

function errorToMessage(error: unknown): string {
	return error instanceof Error ? error.message : String(error)
}

export {}
