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

export async function executeBrowserAction(
	command: BrowserCommand,
	deps: BrowserActionDependencies
): Promise<BrowserActionResult> {
	const { chromeApi } = deps
	const args = command.args || {}

	if (args.action === 'list_tabs') {
		return listTabs(chromeApi, args)
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

	const tab = await tabForAction(chromeApi, args)
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

	const [execution] = await chromeApi.scripting.executeScript<
		BrowserActionResult,
		BrowserActionArgs
	>({
		target: { tabId },
		func: pageActionDispatcher,
		args: [args]
	})
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

export function pageActionDispatcher(args: BrowserActionArgs): BrowserActionResult {
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
				new InputEvent('input', {
					bubbles: true,
					inputType: 'insertText',
					data: args.text || ''
				})
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
