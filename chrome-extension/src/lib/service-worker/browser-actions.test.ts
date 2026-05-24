import { describe, expect, it, vi } from 'vitest'

import { executeBrowserAction } from './browser-actions'
import type { ChromeApi, ChromeTabInfo } from './types'

type DebuggerTarget = { tabId: number }

type RuntimeEvaluateParams = {
  expression?: string
}

type DebuggerSendCommand = NonNullable<ChromeApi['debugger']>['sendCommand']
type TabUpdatedListener = Parameters<ChromeApi['tabs']['onUpdated']['addListener']>[0]

function createChromeEvent<Listener extends (...args: any[]) => void>() {
  const listeners = new Set<Listener>()
  return {
    event: {
      addListener: vi.fn((listener: Listener) => {
        listeners.add(listener)
      }),
      removeListener: vi.fn((listener: Listener) => {
        listeners.delete(listener)
      })
    },
    emit: (...args: Parameters<Listener>) => {
      for (const listener of Array.from(listeners)) {
        listener(...args)
      }
    }
  }
}

function createChromeApi(
  debuggerApi: ChromeApi['debugger'],
  tab: ChromeTabInfo & { id: number; windowId: number; active: boolean } = {
    id: 123,
    windowId: 1,
    active: true
  }
): ChromeApi {
  const onActivated = createChromeEvent<(activeInfo: { tabId: number; windowId: number }) => void>()
  const onUpdated = createChromeEvent<TabUpdatedListener>()
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
      get: vi.fn().mockResolvedValue(tab),
      create: vi.fn(),
      remove: vi.fn(),
      update: vi.fn().mockResolvedValue(tab),
      reload: vi.fn(),
      captureVisibleTab: vi.fn(),
      onActivated: onActivated.event,
      onUpdated: onUpdated.event
    },
    debugger: debuggerApi,
    scripting: { executeScript: vi.fn() }
  } as unknown as ChromeApi
}

describe('executeBrowserAction waited browser actions', () => {
  it('waits for open_tab to reach a loaded page before returning', async () => {
    const onUpdated = createChromeEvent<TabUpdatedListener>()
    const loadingTab = {
      id: 234,
      windowId: 1,
      active: true,
      status: 'loading',
      url: 'https://example.com'
    }
    const completeTab = { ...loadingTab, status: 'complete', title: 'Example' }
    let currentTab = loadingTab
    const chromeApi = createChromeApi(undefined, loadingTab)
    chromeApi.tabs.onUpdated = onUpdated.event
    chromeApi.tabs.create = vi.fn(async () => loadingTab)
    chromeApi.tabs.get = vi.fn(async () => {
      if (currentTab.status === 'loading') {
        queueMicrotask(() => {
          currentTab = completeTab
          onUpdated.emit(234, { status: 'complete' }, completeTab)
        })
      }
      return currentTab
    })

    const result = (await executeBrowserAction(
      {
        session: 'test',
        request_id: 1,
        args: { action: 'open_tab', url: 'https://example.com' }
      },
      { chromeApi }
    )) as Record<string, unknown>

    expect(result.opened).toBe(true)
    expect(result.tab).toMatchObject({ id: 234, status: 'complete', title: 'Example' })
    expect(result.page_ready).toMatchObject({ loaded: true })
    expect(onUpdated.event.addListener).toHaveBeenCalled()
    expect(onUpdated.event.removeListener).toHaveBeenCalled()
  })

  it('pre-arms navigation waiting before navigate updates the tab', async () => {
    const onUpdated = createChromeEvent<TabUpdatedListener>()
    const startTab = {
      id: 123,
      windowId: 1,
      active: true,
      status: 'complete',
      url: 'https://old.example'
    }
    const completeTab = { ...startTab, url: 'https://new.example', status: 'complete' }
    let currentTab = startTab
    const chromeApi = createChromeApi(undefined, startTab)
    chromeApi.tabs.onUpdated = onUpdated.event
    chromeApi.tabs.get = vi.fn(async () => currentTab)
    chromeApi.tabs.update = vi.fn(async () => {
      currentTab = completeTab
      onUpdated.emit(123, { url: completeTab.url, status: 'complete' }, completeTab)
      return completeTab
    })

    const result = (await executeBrowserAction(
      {
        session: 'test',
        request_id: 1,
        args: { action: 'navigate', url: 'https://new.example' }
      },
      { chromeApi }
    )) as Record<string, unknown>

    expect(result.navigated).toBe(true)
    expect(result.tab).toMatchObject({ url: 'https://new.example', status: 'complete' })
    expect(onUpdated.event.addListener.mock.invocationCallOrder[0]).toBeLessThan(
      (chromeApi.tabs.update as ReturnType<typeof vi.fn>).mock.invocationCallOrder[0]
    )
  })

  it('falls back to page history when native goBack throws a localized error', async () => {
    const onUpdated = createChromeEvent<TabUpdatedListener>()
    const startTab = {
      id: 123,
      windowId: 1,
      active: true,
      status: 'complete',
      url: 'https://news.ycombinator.com'
    }
    const loadingTab = { ...startTab, status: 'loading', url: 'https://example.com' }
    const completeTab = {
      ...startTab,
      status: 'complete',
      title: 'Example',
      url: 'https://example.com'
    }
    let currentTab = startTab
    const chromeApi = createChromeApi(undefined, startTab)
    chromeApi.tabs.onUpdated = onUpdated.event
    chromeApi.tabs.get = vi.fn(async () => currentTab)
    chromeApi.tabs.goBack = vi.fn(async () => {
      throw new Error('无法在历史记录中找到下一页。')
    })
    chromeApi.scripting.executeScript = vi.fn(async () => {
      currentTab = loadingTab
      queueMicrotask(() => {
        currentTab = completeTab
        onUpdated.emit(123, { status: 'complete', url: completeTab.url }, completeTab)
      })
      return [{ result: { went_back: true, url: completeTab.url } }]
    }) as ChromeApi['scripting']['executeScript']

    const result = (await executeBrowserAction(
      {
        session: 'test',
        request_id: 1,
        args: { action: 'go_back' }
      },
      { chromeApi }
    )) as Record<string, unknown>

    expect(chromeApi.tabs.goBack).toHaveBeenCalledWith(123)
    expect(chromeApi.scripting.executeScript).toHaveBeenCalledOnce()
    expect(result.went_back).toBe(true)
    expect(result.tab).toMatchObject({ url: 'https://example.com', status: 'complete' })
    expect(result.page_ready).toMatchObject({ loaded: true })
  })

  it('treats data URL navigation as ready once the tab reaches the URL', async () => {
    const startTab = {
      id: 123,
      windowId: 1,
      active: true,
      status: 'complete',
      url: 'https://old.example'
    }
    const dataTab = {
      ...startTab,
      status: 'loading',
      url: 'data:text/html,<select><option>One</option></select>'
    }
    let currentTab = startTab
    const chromeApi = createChromeApi(undefined, startTab)
    chromeApi.tabs.get = vi.fn(async () => currentTab)
    chromeApi.tabs.update = vi.fn(async () => {
      currentTab = dataTab
      return dataTab
    })

    const result = (await executeBrowserAction(
      {
        session: 'test',
        request_id: 1,
        args: { action: 'navigate', url: dataTab.url }
      },
      { chromeApi }
    )) as Record<string, unknown>

    expect(result.navigated).toBe(true)
    expect(result.tab).toMatchObject({ url: dataTab.url })
    expect(result.page_ready).toMatchObject({ loaded: true })
  })

  it('does not mask page action errors with pre-action loading timeouts', async () => {
    vi.useFakeTimers()
    const loadingTab = {
      id: 123,
      windowId: 1,
      active: true,
      status: 'loading',
      url: 'https://example.com/loading'
    }
    const chromeApi = createChromeApi(undefined, loadingTab)
    chromeApi.tabs.get = vi.fn(async () => loadingTab)
    chromeApi.scripting.executeScript = vi.fn(async () => {
      throw new Error('selector is not a select element: #iframeResult')
    }) as ChromeApi['scripting']['executeScript']

    const pending = executeBrowserAction(
      {
        session: 'test',
        request_id: 1,
        args: {
          action: 'select_dropdown',
          selector: '#iframeResult',
          value: 'saab',
          timeout_ms: 100
        }
      },
      { chromeApi }
    )
    const rejection = expect(pending).rejects.toThrow(
      'selector is not a select element: #iframeResult'
    )
    try {
      await vi.advanceTimersByTimeAsync(100)
      await rejection
    } finally {
      vi.useRealTimers()
    }
  })

  it('waits for downloads to complete before returning', async () => {
    vi.useFakeTimers()
    const chromeApi = createChromeApi(undefined)
    let state = 'in_progress'
    chromeApi.downloads = {
      download: vi.fn(async () => 42),
      search: vi.fn(async () => [
        {
          id: 42,
          url: 'https://example.com/report.csv',
          filename: 'report.csv',
          state
        }
      ]),
      cancel: vi.fn(),
      open: vi.fn()
    }

    const pending = executeBrowserAction(
      {
        session: 'test',
        request_id: 1,
        args: { action: 'download', url: 'https://example.com/report.csv' }
      },
      { chromeApi }
    )
    await Promise.resolve()
    state = 'complete'
    await vi.advanceTimersByTimeAsync(250)
    const result = (await pending) as Record<string, unknown>
    vi.useRealTimers()

    expect(result.downloaded).toBe(true)
    expect(result.download).toMatchObject({ id: 42, state: 'complete', filename: 'report.csv' })
  })

  it('attaches page readiness to input actions that may trigger page changes', async () => {
    const completeTab = { id: 123, windowId: 1, active: true, status: 'complete' }
    const chromeApi = createChromeApi(undefined, completeTab)
    chromeApi.tabs.get = vi.fn(async () => completeTab)
    chromeApi.scripting.executeScript = vi.fn(async () => [
      { result: { clicked: true } }
    ]) as ChromeApi['scripting']['executeScript']

    const result = (await executeBrowserAction(
      {
        session: 'test',
        request_id: 1,
        args: { action: 'click', selector: 'button' }
      },
      { chromeApi }
    )) as Record<string, unknown>

    expect(result.clicked).toBe(true)
    expect(result.page_ready).toMatchObject({ loaded: true })
  })
})

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

  it('returns the final expression from multi-statement debugger scripts', async () => {
    const expressions: string[] = []
    const sendCommand = vi.fn(
      async (_target: DebuggerTarget, method: string, params?: RuntimeEvaluateParams) => {
        if (method !== 'Runtime.evaluate') {
          return {}
        }
        expressions.push(params?.expression || '')
        if (expressions.length === 1) {
          return { exceptionDetails: { text: 'SyntaxError: Unexpected token const' } }
        }
        return { result: { type: 'object', value: { exists: true, value: 'opt2' } } }
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
        args: {
          action: 'execute_javascript',
          code: "const sel = document.querySelector('#testselect');\nsel ? { exists: true, value: sel.value } : { exists: false }"
        }
      },
      { chromeApi }
    )) as Record<string, unknown>

    expect(result.result).toEqual({ exists: true, value: 'opt2' })
    expect(expressions[1]).toContain('return (sel ?')
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
    expect(detach).toHaveBeenCalledTimes(2)
  })

  it('reattaches and retries debugger commands after transient detachment', async () => {
    let evaluateAttempts = 0
    const attach = vi.fn(async () => undefined)
    const detach = vi.fn(async () => undefined)
    const sendCommand = vi.fn(
      async (_target: DebuggerTarget, method: string, params?: RuntimeEvaluateParams) => {
        if (method === 'Runtime.evaluate') {
          evaluateAttempts += 1
          if (evaluateAttempts === 1) {
            throw new Error('Debugger is not attached to the tab with id: 123')
          }
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

    const result = (await executeBrowserAction(
      {
        session: 'test',
        request_id: 1,
        args: { action: 'execute_javascript', code: 'document.title' }
      },
      { chromeApi }
    )) as Record<string, unknown>

    expect(result.result).toBe('(document.title)')
    expect(attach).toHaveBeenCalledTimes(2)
    expect(detach).toHaveBeenCalledTimes(1)
    expect(evaluateAttempts).toBe(2)
  })
})

describe('executeBrowserAction screenshot debugger capture', () => {
  it('uses CDP viewport emulation for fixed-size screenshots', async () => {
    const sendCommand = vi.fn(
      async (_target: DebuggerTarget, method: string, _params?: Record<string, unknown>) => {
        if (method === 'Page.captureScreenshot') {
          return { data: 'cG5nLWJ5dGVz' }
        }
        return {}
      }
    )
    const chromeApi = createChromeApi({
      attach: vi.fn(async () => undefined),
      detach: vi.fn(async () => undefined),
      sendCommand: sendCommand as DebuggerSendCommand
    })
    chromeApi.tabs.captureVisibleTab = vi.fn()

    const result = (await executeBrowserAction(
      {
        session: 'test',
        request_id: 1,
        args: {
          action: 'screenshot',
          viewport_width: 800,
          viewport_height: 600,
          device_scale_factor: 2,
          include_data_url: true
        }
      },
      { chromeApi }
    )) as Record<string, unknown>

    expect(chromeApi.tabs.captureVisibleTab).not.toHaveBeenCalled()
    expect(sendCommand).toHaveBeenCalledWith({ tabId: 123 }, 'Emulation.setDeviceMetricsOverride', {
      width: 800,
      height: 600,
      deviceScaleFactor: 2,
      mobile: false
    })
    expect(sendCommand).toHaveBeenCalledWith(
      { tabId: 123 },
      'Emulation.clearDeviceMetricsOverride',
      undefined
    )
    expect(result).toMatchObject({
      captured: true,
      data_url: 'data:image/png;base64,cG5nLWJ5dGVz',
      viewport: { width: 800, height: 600, deviceScaleFactor: 2, mobile: false }
    })
  })
})

describe('executeBrowserAction native input', () => {
  it('dispatches touch events for clicks on mobile-like pages', async () => {
    const tab = { id: 123, windowId: 1, active: true, status: 'complete' }
    const sendCommand = vi.fn(
      async (_target: DebuggerTarget, method: string, _params?: Record<string, unknown>) => {
        if (method === 'Runtime.evaluate') {
          return { result: { type: 'boolean', value: true } }
        }
        return {}
      }
    )
    const chromeApi = createChromeApi(
      {
        attach: vi.fn(async () => undefined),
        detach: vi.fn(async () => undefined),
        sendCommand: sendCommand as DebuggerSendCommand
      },
      tab
    )
    chromeApi.tabs.get = vi.fn(async () => tab)
    chromeApi.scripting.executeScript = vi.fn(async () => [
      {
        result: {
          x: 12,
          y: 34,
          label: 'Tap me',
          bounding_box: { x: 0, y: 0, width: 24, height: 68 }
        }
      }
    ]) as ChromeApi['scripting']['executeScript']

    const result = (await executeBrowserAction(
      {
        session: 'test',
        request_id: 1,
        args: { action: 'click', selector: 'button' }
      },
      { chromeApi }
    )) as Record<string, unknown>

    const inputEvents = sendCommand.mock.calls
      .filter((call) => String(call[1]).startsWith('Input.'))
      .map((call) => [call[1], (call[2] as Record<string, unknown> | undefined)?.type])

    expect(result).toMatchObject({ clicked: true, native: true, x: 12, y: 34 })
    expect(inputEvents).toEqual([
      ['Input.dispatchMouseEvent', 'mouseMoved'],
      ['Input.dispatchTouchEvent', 'touchStart'],
      ['Input.dispatchTouchEvent', 'touchEnd']
    ])
  })

  it('uses native CDP text insertion for editable text targets', async () => {
    const tab = { id: 123, windowId: 1, active: true, status: 'complete' }
    const sendCommand = vi.fn(
      async (_target: DebuggerTarget, method: string, _params?: Record<string, unknown>) => {
        if (method === 'Runtime.evaluate') {
          return { result: { type: 'boolean', value: false } }
        }
        return {}
      }
    )
    const chromeApi = createChromeApi(
      {
        attach: vi.fn(async () => undefined),
        detach: vi.fn(async () => undefined),
        sendCommand: sendCommand as DebuggerSendCommand
      },
      tab
    )
    chromeApi.tabs.get = vi.fn(async () => tab)
    chromeApi.scripting.executeScript = vi.fn(async () => [
      {
        result: {
          native_text_input: true,
          x: 20,
          y: 30,
          selector: '#name',
          label: 'Name',
          bounding_box: { x: 10, y: 10, width: 100, height: 40 }
        }
      }
    ]) as ChromeApi['scripting']['executeScript']

    const result = (await executeBrowserAction(
      {
        session: 'test',
        request_id: 1,
        args: { action: 'type_text', selector: '#name', text: 'Ada' }
      },
      { chromeApi }
    )) as Record<string, unknown>

    const insertTextCall = sendCommand.mock.calls.find((call) => call[1] === 'Input.insertText')
    const selectAllCalls = sendCommand.mock.calls.filter(
      (call) =>
        call[1] === 'Input.dispatchKeyEvent' &&
        Array.isArray((call[2] as Record<string, unknown> | undefined)?.commands)
    )

    expect(result).toMatchObject({ typed: true, native: true, selector: '#name', length: 3 })
    expect(selectAllCalls).toHaveLength(2)
    expect(insertTextCall?.[2]).toEqual({ text: 'Ada' })
  })
})
