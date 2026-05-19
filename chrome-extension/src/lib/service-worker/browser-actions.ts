import type {
	BrowserActionArgs,
	BrowserActionResult,
	BrowserCommand,
	ChromeApi,
	ChromeTabInfo
} from './types'

type BrowserActionDependencies = {
	chromeApi: ChromeApi
}

const debuggerActionLocks = new Map<number, Promise<void>>()

export async function executeBrowserAction(
	command: BrowserCommand,
	deps: BrowserActionDependencies
): Promise<BrowserActionResult> {
	const { chromeApi } = deps
	const args = command.args || {}

	if (args.action === 'list_tabs') {
		return listTabs(chromeApi, args)
	}

	if (args.action === 'get_current_tab') {
		return { tab: tabSummary(await activeTab(chromeApi)) }
	}

	if (args.action === 'open_tab') {
		const tab = await chromeApi.tabs.create({
			url: normalizeOptionalText(args.url),
			active: args.active ?? true,
			windowId: positiveInteger(args.window_id) || undefined
		})
		return { opened: true, tab: tabSummary(tab) }
	}

	if (args.action === 'switch_tab') {
		const tabId = requirePositiveInteger(args.tab_id, 'switch_tab requires tab_id')
		const tab = await activateTab(chromeApi, tabId)
		return { switched: true, tab: tabSummary(tab) }
	}

	if (args.action === 'close_tab') {
		const tabId = requirePositiveInteger(args.tab_id, 'close_tab requires tab_id')
		await chromeApi.tabs.remove(tabId)
		return { closed: true, tab_id: tabId }
	}

	if (args.action === 'launch_browser') {
		return { launched: false, connected: true, reason: 'browser is already running' }
	}

	if (args.action === 'navigate') {
		if (!args.url) {
			throw new Error('navigate requires url')
		}
		const tab = await tabForAction(chromeApi, args)
		const active = args.active ?? true
		if (!tab?.id) {
			const created = await chromeApi.tabs.create({
				url: args.url,
				active,
				windowId: positiveInteger(args.window_id) || undefined
			})
			return { navigated: true, url: args.url, tab: tabSummary(created) }
		}
		const updated = await chromeApi.tabs.update(tab.id, { url: args.url, active })
		if (active && updated) {
			await focusWindow(chromeApi, updated.windowId).catch(() => undefined)
		}
		return { navigated: true, url: args.url, tab: tabSummary(updated) }
	}

	if (args.action === 'reload') {
		const tab = await tabForPageAction(chromeApi, args)
		const tabId = tab?.id
		if (!tabId) {
			throw new Error('no target tab')
		}
		await chromeApi.tabs.reload(tabId, { bypassCache: args.bypass_cache ?? false })
		return { reloaded: true, bypass_cache: args.bypass_cache ?? false, tab: tabSummary(tab) }
	}

	const tab = await tabForPageAction(chromeApi, args)
	const tabId = tab?.id
	if (!tabId) {
		throw new Error('no target tab')
	}

	if (args.action === 'screenshot') {
		const activeTab = await activateTab(chromeApi, tabId).catch(() => tab)
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

	if (args.action === 'execute_javascript' && scriptExecutionMode(args) === 'debugger') {
		return executeJavaScriptWithDebugger(chromeApi, tabId, args)
	}

	const [execution] = await chromeApi.scripting.executeScript<
		BrowserActionResult,
		BrowserActionArgs
	>({
		target: scriptTarget(tabId, args),
		world: args.action === 'execute_javascript' ? scriptExecutionWorld(args) : 'ISOLATED',
		func: pageActionDispatcher,
		args: [args]
	})
	if (args.action === 'execute_javascript' && execution?.result === undefined) {
		throw new Error('execute_javascript did not return a script result')
	}
	return execution ? execution.result : null
}

async function listTabs(
	chromeApi: ChromeApi,
	args: BrowserActionArgs
): Promise<BrowserActionResult> {
	const windowId = positiveInteger(args.window_id)
	const queryInfo = windowId ? { windowId } : {}
	const tabs = await chromeApi.tabs.query(queryInfo)
	return {
		tabs: tabs.map(tabSummary),
		active_tab_id: tabs.find((tab) => tab.active)?.id || null
	}
}

async function tabForAction(
	chromeApi: ChromeApi,
	args: BrowserActionArgs
): Promise<ChromeTabInfo | null> {
	const tabId = positiveInteger(args.tab_id)
	if (tabId) {
		return chromeApi.tabs.get(tabId)
	}
	return activeTab(chromeApi)
}

async function tabForPageAction(
	chromeApi: ChromeApi,
	args: BrowserActionArgs
): Promise<ChromeTabInfo | null> {
	const tabId = positiveInteger(args.tab_id)
	if (tabId) {
		return activateTab(chromeApi, tabId)
	}
	return activeTab(chromeApi)
}

function scriptTarget(
	tabId: number,
	args: BrowserActionArgs
): { tabId: number; frameIds?: number[] } {
	const frameId = positiveInteger(args.frame_id)
	return frameId ? { tabId, frameIds: [frameId] } : { tabId }
}

type ScriptExecutionMode = 'debugger' | 'scripting'
type ScriptExecutionWorld = 'ISOLATED' | 'MAIN'
type RequestedScriptWorld = ScriptExecutionWorld | 'DEBUGGER'

function scriptExecutionMode(args: BrowserActionArgs): ScriptExecutionMode {
	const world = requestedScriptWorld(args.world)
	if (world === 'DEBUGGER') {
		return 'debugger'
	}
	return args.use_bridge === false ? 'scripting' : 'debugger'
}

function scriptExecutionWorld(args: BrowserActionArgs): ScriptExecutionWorld {
	return requestedScriptWorld(args.world) === 'MAIN' ? 'MAIN' : 'ISOLATED'
}

function requestedScriptWorld(value: unknown): RequestedScriptWorld {
	const normalized = typeof value === 'string' ? value.trim().toLowerCase() : ''
	if (normalized === 'main') {
		return 'MAIN'
	}
	if (normalized === 'debugger' || normalized === 'bridge') {
		return 'DEBUGGER'
	}
	return 'ISOLATED'
}

type DebuggerTarget = { tabId: number }
type RuntimeRemoteObject = {
	type?: string
	subtype?: string
	value?: unknown
	unserializableValue?: string
	description?: string
}
type RuntimeExceptionDetails = {
	text?: string
	exception?: RuntimeRemoteObject
	lineNumber?: number
	columnNumber?: number
}
type RuntimeEvaluateResult = {
	result?: RuntimeRemoteObject
	exceptionDetails?: RuntimeExceptionDetails
}

async function executeJavaScriptWithDebugger(
	chromeApi: ChromeApi,
	tabId: number,
	args: BrowserActionArgs
): Promise<Record<string, unknown>> {
	return runExclusiveDebuggerAction(tabId, () =>
		executeJavaScriptWithAttachedDebugger(chromeApi, tabId, args)
	)
}

function runExclusiveDebuggerAction<T>(tabId: number, task: () => Promise<T>): Promise<T> {
	const previous = debuggerActionLocks.get(tabId) || Promise.resolve()
	const run = previous.catch(() => undefined).then(task)
	const release = run.then(
		() => undefined,
		() => undefined
	)
	debuggerActionLocks.set(tabId, release)
	return run.finally(() => {
		if (debuggerActionLocks.get(tabId) === release) {
			debuggerActionLocks.delete(tabId)
		}
	})
}

async function executeJavaScriptWithAttachedDebugger(
	chromeApi: ChromeApi,
	tabId: number,
	args: BrowserActionArgs
): Promise<Record<string, unknown>> {
	const code = String(args.code || '')
	if (!code.trim()) {
		throw new Error('execute_javascript requires code')
	}
	if (args.frame_id !== undefined && args.frame_id !== null) {
		throw new Error(
			'execute_javascript frame_id is only supported when use_bridge is false and world is isolated or main'
		)
	}
	if (!chromeApi.debugger) {
		throw new Error('Chrome debugger API is unavailable; enable the debugger permission')
	}

	const target = { tabId }
	let attached = false
	try {
		await chromeApi.debugger.detach(target).catch(() => undefined)
		await chromeApi.debugger.attach(target, '1.3')
		attached = true
		await chromeApi.debugger.sendCommand(target, 'Runtime.enable').catch(() => undefined)
		const result = await evaluateDebuggerJavaScript(chromeApi, target, code)
		return { executed: true, world: 'debugger', result }
	} finally {
		if (attached) {
			await chromeApi.debugger.detach(target).catch(() => undefined)
		}
	}
}

async function evaluateDebuggerJavaScript(
	chromeApi: ChromeApi,
	target: DebuggerTarget,
	code: string
): Promise<unknown> {
	const expression = code.trim().replace(/;+$/, '')
	const expressionResult = await sendDebuggerRuntimeEvaluate(chromeApi, target, `(${expression})`)
	if (!isSyntaxException(expressionResult.exceptionDetails)) {
		return debuggerEvaluationValue(expressionResult)
	}

	const bodyResult = await sendDebuggerRuntimeEvaluate(
		chromeApi,
		target,
		`(function () {\n${code}\n})()`
	)
	return debuggerEvaluationValue(bodyResult)
}

function sendDebuggerRuntimeEvaluate(
	chromeApi: ChromeApi,
	target: DebuggerTarget,
	expression: string
): Promise<RuntimeEvaluateResult> {
	return chromeApi.debugger!.sendCommand<RuntimeEvaluateResult>(target, 'Runtime.evaluate', {
		expression,
		awaitPromise: true,
		returnByValue: true,
		userGesture: true,
		replMode: true
	})
}

function isSyntaxException(details?: RuntimeExceptionDetails): boolean {
	return debuggerExceptionText(details).includes('SyntaxError')
}

function debuggerEvaluationValue(evaluation: RuntimeEvaluateResult): unknown {
	if (evaluation.exceptionDetails) {
		throw new Error(
			`execute_javascript failed: ${debuggerExceptionText(evaluation.exceptionDetails)}`
		)
	}
	const result = evaluation.result
	if (!result || result.type === 'undefined') {
		return null
	}
	if ('value' in result) {
		return result.value
	}
	return result.unserializableValue || result.description || null
}

function debuggerExceptionText(details?: RuntimeExceptionDetails): string {
	return (
		details?.exception?.description ||
		details?.exception?.value ||
		details?.text ||
		'Unknown JavaScript exception'
	).toString()
}

export async function activeTab(chromeApi: ChromeApi): Promise<ChromeTabInfo | null> {
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

async function activateTab(chromeApi: ChromeApi, tabId: number): Promise<ChromeTabInfo> {
	const tab = (await chromeApi.tabs.update(tabId, { active: true })) as ChromeTabInfo
	await focusWindow(chromeApi, tab.windowId).catch(() => undefined)
	return tab
}

async function focusWindow(chromeApi: ChromeApi, windowId?: number): Promise<void> {
	if (typeof windowId === 'number' && chromeApi.windows?.update) {
		await chromeApi.windows.update(windowId, { focused: true })
	}
}

export function tabSummary(tab: ChromeTabInfo | null | undefined): Record<string, unknown> | null {
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

export function pageActionDispatcher(
	args: BrowserActionArgs
): BrowserActionResult | Promise<BrowserActionResult> {
	const maxText = 12000
	const defaultHtmlLimit = 200000

	function truncate(value: unknown, limit = maxChars(maxText)): string {
		const text = String(value || '')
		return text.length > limit
			? `${text.slice(0, limit)}\n[truncated ${text.length - limit} chars]`
			: text
	}

	function maxChars(defaultLimit: number): number {
		const requested =
			typeof args.max_chars === 'number' && Number.isFinite(args.max_chars)
				? args.max_chars
				: defaultLimit
		return Math.max(1000, Math.min(500000, Math.floor(requested)))
	}

	function timeoutMs(defaultTimeout: number): number {
		const requested =
			typeof args.timeout_ms === 'number' && Number.isFinite(args.timeout_ms)
				? args.timeout_ms
				: defaultTimeout
		return Math.max(100, Math.min(120000, Math.floor(requested)))
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

	function deepQuerySelector(
		root: Document | ShadowRoot | Element,
		selector: string
	): Element | null {
		const direct = root.querySelector(selector)
		if (direct) {
			return direct
		}

		for (const element of Array.from(root.querySelectorAll('*'))) {
			const shadowRoot = element instanceof HTMLElement ? element.shadowRoot : null
			if (!shadowRoot) {
				continue
			}
			const found = deepQuerySelector(shadowRoot, selector)
			if (found) {
				return found
			}
		}
		return null
	}

	function queryRequired(selector?: string): Element {
		if (!selector) {
			throw new Error('selector is required')
		}
		const element = deepQuerySelector(document, selector)
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

	function elementAttributes(element: Element): Record<string, string> {
		return Object.fromEntries(Array.from(element.attributes).map((attr) => [attr.name, attr.value]))
	}

	function elementBox(element: Element): Record<string, number> {
		const rect = element.getBoundingClientRect()
		return {
			x: rect.x,
			y: rect.y,
			width: rect.width,
			height: rect.height,
			top: rect.top,
			right: rect.right,
			bottom: rect.bottom,
			left: rect.left
		}
	}

	function elementInfo(element: Element): Record<string, unknown> {
		return {
			selector: cssPath(element),
			tag: element.tagName.toLowerCase(),
			id: element.id || null,
			classes: Array.from(element.classList),
			role: element.getAttribute('role'),
			name: element.getAttribute('name'),
			type: element.getAttribute('type'),
			label: elementLabel(element),
			text: truncate(
				element instanceof HTMLElement ? element.innerText : element.textContent,
				2000
			),
			value: 'value' in element ? String(element.value) : null,
			attributes: elementAttributes(element),
			bounding_box: elementBox(element),
			visible: visible(element),
			disabled: 'disabled' in element ? Boolean(element.disabled) : false
		}
	}

	function pointFromArgs(prefix = ''): { x: number; y: number } | null {
		const x = prefix === 'to_' ? args.to_x : args.x
		const y = prefix === 'to_' ? args.to_y : args.y
		return typeof x === 'number' &&
			typeof y === 'number' &&
			Number.isFinite(x) &&
			Number.isFinite(y)
			? { x, y }
			: null
	}

	function elementFromSelectorOrPoint(selector = args.selector): Element {
		if (selector) {
			return queryRequired(selector)
		}
		const point = pointFromArgs()
		if (!point) {
			throw new Error('selector or x/y coordinates are required')
		}
		const element = document.elementFromPoint(point.x, point.y)
		if (!element) {
			throw new Error(`no element at coordinates: ${point.x},${point.y}`)
		}
		return element
	}

	function centerOf(element: Element): { x: number; y: number } {
		const rect = element.getBoundingClientRect()
		return { x: rect.left + rect.width / 2, y: rect.top + rect.height / 2 }
	}

	function pointerPoint(element: Element): { x: number; y: number } {
		return pointFromArgs() || centerOf(element)
	}

	function dispatchMouse(element: Element, type: string, point: { x: number; y: number }): void {
		element.dispatchEvent(
			new MouseEvent(type, {
				bubbles: true,
				cancelable: true,
				clientX: point.x,
				clientY: point.y,
				view: window
			})
		)
	}

	function scrollBehavior(): ScrollBehavior {
		return args.behavior === 'auto' || args.behavior === 'smooth' || args.behavior === 'instant'
			? args.behavior
			: 'smooth'
	}

	function structuredData(): Record<string, unknown> {
		const jsonLd = Array.from(document.querySelectorAll('script[type="application/ld+json"]'))
			.slice(0, 20)
			.map((element) => {
				const text = element.textContent || ''
				try {
					return JSON.parse(text)
				} catch (_error) {
					return { parse_error: true, text: truncate(text, 2000) }
				}
			})
		const meta = Array.from(document.querySelectorAll('meta[name], meta[property]'))
			.slice(0, 80)
			.map((element) => ({
				name: element.getAttribute('name') || element.getAttribute('property'),
				content: truncate(element.getAttribute('content'), 1000)
			}))
		const tables = Array.from(document.querySelectorAll('table'))
			.filter(visible)
			.slice(0, 10)
			.map((table) => {
				const rows = Array.from(table.querySelectorAll('tr')).slice(0, 50)
				return {
					selector: cssPath(table),
					caption: truncate(table.querySelector('caption')?.textContent, 500),
					rows: rows.map((row) =>
						Array.from(row.querySelectorAll('th, td'))
							.slice(0, 20)
							.map((cell) => truncate(cell.textContent, 500))
					)
				}
			})
		const lists = Array.from(document.querySelectorAll('ul, ol'))
			.filter(visible)
			.slice(0, 20)
			.map((list) => ({
				selector: cssPath(list),
				ordered: list.tagName.toLowerCase() === 'ol',
				items: Array.from(list.children)
					.slice(0, 40)
					.map((item) => truncate(item.textContent, 500))
			}))

		return { url: location.href, title: document.title, json_ld: jsonLd, meta, tables, lists }
	}

	function findMatches(): Record<string, unknown> {
		const query = String(args.query || '')
			.trim()
			.toLowerCase()
		if (!query) {
			throw new Error('find_in_page requires query')
		}
		const matches = Array.from(document.body?.querySelectorAll('*') || [])
			.filter(
				(element) => visible(element) && (element.textContent || '').toLowerCase().includes(query)
			)
			.slice(0, 80)
			.map((element) => {
				if (args.highlight && element instanceof HTMLElement) {
					element.dataset.andaFindHighlight = 'true'
					element.style.outline = '2px solid #f59e0b'
					element.style.outlineOffset = '2px'
				}
				return {
					selector: cssPath(element),
					label: elementLabel(element),
					text: truncate(element.textContent, 800),
					bounding_box: elementBox(element)
				}
			})
		return {
			query: args.query,
			count: matches.length,
			highlighted: Boolean(args.highlight),
			matches
		}
	}

	function serializeForResult(value: unknown, depth = 0): unknown {
		if (
			value === null ||
			value === undefined ||
			typeof value === 'string' ||
			typeof value === 'number' ||
			typeof value === 'boolean'
		) {
			return value
		}
		if (value instanceof Element) {
			return elementInfo(value)
		}
		if (value instanceof Error) {
			return { name: value.name, message: value.message, stack: value.stack }
		}
		if (depth > 4) {
			return String(value)
		}
		if (Array.isArray(value)) {
			return value.slice(0, 200).map((item) => serializeForResult(item, depth + 1))
		}
		if (typeof value === 'object') {
			return Object.fromEntries(
				Object.entries(value as Record<string, unknown>)
					.slice(0, 200)
					.map(([key, entry]) => [key, serializeForResult(entry, depth + 1)])
			)
		}
		return String(value)
	}

	function serializeScriptResult(value: unknown): unknown {
		const serialized = serializeForResult(value)
		return serialized === undefined ? null : serialized
	}

	function compileScriptExpression(code: string): ((args: BrowserActionArgs) => unknown) | null {
		const expression = code.trim().replace(/;+$/, '')
		if (!expression) {
			return null
		}

		try {
			return new Function('args', `"use strict"; return (${expression})`) as (
				args: BrowserActionArgs
			) => unknown
		} catch (error) {
			if (error instanceof SyntaxError) {
				return null
			}
			throw error
		}
	}

	function compileScriptBody(code: string): (args: BrowserActionArgs) => unknown {
		return new Function('args', `"use strict";\n${code}`) as (args: BrowserActionArgs) => unknown
	}

	function isPromiseLike(value: unknown): value is PromiseLike<unknown> {
		return (
			value !== null &&
			(typeof value === 'object' || typeof value === 'function') &&
			typeof (value as { then?: unknown }).then === 'function'
		)
	}

	function scriptResult(value: unknown): Record<string, unknown> {
		return { executed: true, result: serializeScriptResult(value) }
	}

	function executeJavaScript(
		code: string
	): Record<string, unknown> | Promise<Record<string, unknown>> {
		const execute = compileScriptExpression(code) || compileScriptBody(code)
		const result = execute(args)
		return isPromiseLike(result) ? Promise.resolve(result).then(scriptResult) : scriptResult(result)
	}

	async function copyText(text: string): Promise<Record<string, unknown>> {
		try {
			await navigator.clipboard.writeText(text)
		} catch (_error) {
			const textarea = document.createElement('textarea')
			textarea.value = text
			textarea.style.position = 'fixed'
			textarea.style.opacity = '0'
			document.body.appendChild(textarea)
			textarea.focus()
			textarea.select()
			const copied = document.execCommand('copy')
			textarea.remove()
			if (!copied) {
				throw new Error('copy command failed')
			}
		}
		return { copied: true, length: text.length }
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
				active_element: document.activeElement ? elementInfo(document.activeElement) : null,
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
		case 'get_full_page_html': {
			return {
				url: location.href,
				title: document.title,
				html: truncate(document.documentElement?.outerHTML || '', maxChars(defaultHtmlLimit))
			}
		}
		case 'get_structured_data': {
			return structuredData()
		}
		case 'get_element_info': {
			const element = queryRequired(args.selector)
			return elementInfo(element)
		}
		case 'get_viewport_size': {
			return {
				viewport: { width: window.innerWidth, height: window.innerHeight },
				screen: { width: window.screen.width, height: window.screen.height },
				device_pixel_ratio: window.devicePixelRatio,
				scroll: {
					x: window.scrollX,
					y: window.scrollY,
					max_x: document.documentElement.scrollWidth,
					max_y: document.documentElement.scrollHeight
				}
			}
		}
		case 'find_in_page': {
			return findMatches()
		}
		case 'wait_for_element': {
			const timeout = timeoutMs(10000)
			const selector = args.selector
			if (!selector) {
				throw new Error('wait_for_element requires selector')
			}
			const existing = deepQuerySelector(document, selector)
			if (existing && visible(existing)) {
				return { found: true, selector, element: elementInfo(existing) }
			}
			return new Promise((resolve, reject) => {
				const observer = new MutationObserver(() => {
					const element = deepQuerySelector(document, selector)
					if (element && visible(element)) {
						clearTimeout(timer)
						observer.disconnect()
						resolve({ found: true, selector, element: elementInfo(element) })
					}
				})
				const timer = setTimeout(() => {
					observer.disconnect()
					reject(new Error(`selector not found before timeout: ${selector}`))
				}, timeout)
				observer.observe(document.documentElement, {
					childList: true,
					subtree: true,
					attributes: true
				})
			})
		}
		case 'read_selection': {
			return { selection: String(window.getSelection ? window.getSelection() : '') }
		}
		case 'click': {
			const element = elementFromSelectorOrPoint()
			element.scrollIntoView({ block: 'center', inline: 'center' })
			const point = pointerPoint(element)
			dispatchMouse(element, 'mouseover', point)
			dispatchMouse(element, 'mousemove', point)
			dispatchMouse(element, 'mousedown', point)
			dispatchMouse(element, 'mouseup', point)
			dispatchMouse(element, 'click', point)
			return { clicked: true, selector: args.selector, label: elementLabel(element) }
		}
		case 'hover': {
			const element = elementFromSelectorOrPoint()
			element.scrollIntoView({ block: 'center', inline: 'center' })
			const point = pointerPoint(element)
			dispatchMouse(element, 'mouseover', point)
			dispatchMouse(element, 'mouseenter', point)
			dispatchMouse(element, 'mousemove', point)
			return { hovered: true, selector: args.selector, label: elementLabel(element) }
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
				new InputEvent('input', {
					bubbles: true,
					inputType: 'insertText',
					data: args.text || ''
				})
			)
			element.dispatchEvent(new Event('change', { bubbles: true }))
			return { typed: true, selector: args.selector, length: String(args.text || '').length }
		}
		case 'select_dropdown': {
			const element = queryRequired(args.selector)
			element.scrollIntoView({ block: 'center', inline: 'center' })
			if (!(element instanceof HTMLSelectElement)) {
				throw new Error(`selector is not a select element: ${args.selector}`)
			}
			const value = String(args.value || '')
			const option = Array.from(element.options).find(
				(option) => option.value === value || option.label === value || option.text === value
			)
			if (!option) {
				throw new Error(`select option not found: ${value}`)
			}
			element.value = option.value
			element.dispatchEvent(new Event('input', { bubbles: true }))
			element.dispatchEvent(new Event('change', { bubbles: true }))
			return { selected: true, selector: args.selector, value: option.value, label: option.label }
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
		case 'scroll_to': {
			const element = queryRequired(args.selector)
			element.scrollIntoView({ block: 'center', inline: 'center', behavior: scrollBehavior() })
			return { scrolled_to: true, selector: args.selector, label: elementLabel(element) }
		}
		case 'drag_and_drop': {
			const source = queryRequired(args.from_selector)
			const target = args.to_selector
				? queryRequired(args.to_selector)
				: (() => {
						const point = pointFromArgs('to_')
						if (!point) {
							throw new Error('drag_and_drop requires to_selector or to_x/to_y')
						}
						const element = document.elementFromPoint(point.x, point.y)
						if (!element) {
							throw new Error(`no drop target at coordinates: ${point.x},${point.y}`)
						}
						return element
					})()
			source.scrollIntoView({ block: 'center', inline: 'center' })
			const sourcePoint = centerOf(source)
			const targetPoint = pointFromArgs('to_') || centerOf(target)
			const dataTransfer = new DataTransfer()
			source.dispatchEvent(
				new DragEvent('dragstart', { bubbles: true, cancelable: true, dataTransfer })
			)
			dispatchMouse(source, 'mousedown', sourcePoint)
			dispatchMouse(target, 'mousemove', targetPoint)
			target.dispatchEvent(
				new DragEvent('dragenter', { bubbles: true, cancelable: true, dataTransfer })
			)
			target.dispatchEvent(
				new DragEvent('dragover', { bubbles: true, cancelable: true, dataTransfer })
			)
			target.dispatchEvent(new DragEvent('drop', { bubbles: true, cancelable: true, dataTransfer }))
			dispatchMouse(target, 'mouseup', targetPoint)
			source.dispatchEvent(
				new DragEvent('dragend', { bubbles: true, cancelable: true, dataTransfer })
			)
			return {
				dragged: true,
				from_selector: args.from_selector,
				to_selector: args.to_selector || cssPath(target),
				from: elementLabel(source),
				to: elementLabel(target)
			}
		}
		case 'copy_to_clipboard': {
			return copyText(String(args.text || ''))
		}
		case 'go_back': {
			history.back()
			return { went_back: true, url: location.href }
		}
		case 'go_forward': {
			history.forward()
			return { went_forward: true, url: location.href }
		}
		case 'execute_javascript': {
			const code = String(args.code || '')
			if (!code.trim()) {
				throw new Error('execute_javascript requires code')
			}
			return executeJavaScript(code)
		}
		default:
			throw new Error(`unsupported browser action: ${args.action}`)
	}
}
