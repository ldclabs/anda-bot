import { describe, expect, it, vi } from 'vitest'

import { executeBrowserAction } from './browser-actions'
import type { ChromeApi } from './types'

type DebuggerTarget = { tabId: number }

type RuntimeEvaluateParams = {
	expression?: string
}

type DebuggerSendCommand = NonNullable<ChromeApi['debugger']>['sendCommand']

function createChromeApi(
	debuggerApi: ChromeApi['debugger'],
	tab: { id: number; windowId: number; active: boolean } = { id: 123, windowId: 1, active: true }
): ChromeApi {
	return {
		runtime: {
			onInstalled: { addListener: vi.fn(), removeListener: vi.fn() },
			onStartup: { addListener: vi.fn(), removeListener: vi.fn() },
			sendMessage: vi.fn(),
			onMessage: { addListener: vi.fn() }
		},
		action: { onClicked: { addListener: vi.fn(), removeListener: vi.fn() } },
		i18n: {} as typeof chrome.i18n,
		storage: { local: { get: vi.fn(), set: vi.fn() } },
		tabs: {
			query: vi.fn().mockResolvedValue([tab]),
			get: vi.fn(),
			create: vi.fn(),
			remove: vi.fn(),
			update: vi.fn().mockResolvedValue(tab),
			reload: vi.fn(),
			captureVisibleTab: vi.fn(),
			onActivated: { addListener: vi.fn(), removeListener: vi.fn() },
			onUpdated: { addListener: vi.fn(), removeListener: vi.fn() }
		},
		debugger: debuggerApi,
		scripting: { executeScript: vi.fn() }
	} as unknown as ChromeApi
}

describe('executeBrowserAction execute_javascript debugger bridge', () => {
	it('returns primitive values from direct Runtime.evaluate expressions', async () => {
		const sendCommand = vi.fn(
			async (_target: DebuggerTarget, method: string, params?: RuntimeEvaluateParams) => {
				if (method === 'Runtime.evaluate') {
					expect(params?.expression).toBe('(document.title)')
					return { result: { type: 'string', value: 'MDN Web Docs' } }
				}
				return {}
			}
		)
		const chromeApi = createChromeApi({
			attach: vi.fn(async () => undefined),
			detach: vi.fn(async () => undefined),
			sendCommand: sendCommand as DebuggerSendCommand
		})

		const result = (await executeBrowserAction(
			{
				session: 'test',
				request_id: 1,
				args: { action: 'execute_javascript', code: 'document.title' }
			},
			{ chromeApi }
		)) as Record<string, unknown>

		expect(result).toEqual({ executed: true, world: 'debugger', result: 'MDN Web Docs' })
	})

	it('falls back to function-body evaluation for return statements', async () => {
		const sendCommand = vi.fn(
			async (_target: DebuggerTarget, method: string, params?: RuntimeEvaluateParams) => {
				if (method !== 'Runtime.evaluate') {
					return {}
				}
				if (params?.expression === '(return document.title)') {
					return { exceptionDetails: { text: 'SyntaxError: Illegal return statement' } }
				}
				expect(params?.expression).toBe('(function () {\nreturn document.title\n})()')
				return { result: { type: 'string', value: 'MDN Web Docs' } }
			}
		)
		const chromeApi = createChromeApi({
			attach: vi.fn(async () => undefined),
			detach: vi.fn(async () => undefined),
			sendCommand: sendCommand as DebuggerSendCommand
		})

		const result = (await executeBrowserAction(
			{
				session: 'test',
				request_id: 1,
				args: { action: 'execute_javascript', code: 'return document.title' }
			},
			{ chromeApi }
		)) as Record<string, unknown>

		expect(result.result).toBe('MDN Web Docs')
	})

	it('serializes debugger sessions for concurrent calls on the same tab', async () => {
		let attached = false
		let maxConcurrentAttached = 0
		let currentAttached = 0
		const attach = vi.fn(async () => {
			if (attached) {
				throw new Error('Another debugger is already attached to the tab with id: 123')
			}
			attached = true
			currentAttached += 1
			maxConcurrentAttached = Math.max(maxConcurrentAttached, currentAttached)
		})
		const detach = vi.fn(async () => {
			if (attached) {
				attached = false
				currentAttached -= 1
			}
		})
		const sendCommand = vi.fn(
			async (_target: DebuggerTarget, method: string, params?: RuntimeEvaluateParams) => {
				if (method === 'Runtime.evaluate') {
					await Promise.resolve()
					return { result: { type: 'string', value: params?.expression } }
				}
				return {}
			}
		)
		const chromeApi = createChromeApi({
			attach,
			detach,
			sendCommand: sendCommand as DebuggerSendCommand
		})

		const [first, second] = await Promise.all([
			executeBrowserAction(
				{
					session: 'test',
					request_id: 1,
					args: { action: 'execute_javascript', code: 'document.title' }
				},
				{ chromeApi }
			),
			executeBrowserAction(
				{
					session: 'test',
					request_id: 2,
					args: { action: 'execute_javascript', code: 'location.href' }
				},
				{ chromeApi }
			)
		])

		expect((first as Record<string, unknown>).result).toBe('(document.title)')
		expect((second as Record<string, unknown>).result).toBe('(location.href)')
		expect(maxConcurrentAttached).toBe(1)
		expect(attach).toHaveBeenCalledTimes(2)
		expect(detach).toHaveBeenCalledTimes(4)
	})
})
