import { afterEach, expect, it, vi } from 'vitest'
import { parseConfigDraft, renderConfigYaml, removeArrayItem } from './lib/anda/config/schema'
import { loadBrainGraphSettings, saveBrainGraphSettings } from './lib/anda/brain/api'
import { conversationToGroup, type NormalizedMessageCache } from './lib/anda/client/conversations'
import { DaemonConfigApi, loadConfigSettings, saveConfigSettings } from './lib/anda/config/api'
import { renderMarkdown } from './lib/utils/markdown'
import { rememberActiveTab } from './lib/service-worker/browser-tabs'
import { handlePageAudioCapture } from './lib/service-worker/page-voice'
import { createTabLoadWatcher } from './lib/service-worker/browser-navigation'
import { waitForNetworkIdle } from './lib/service-worker/browser-debugger'
import { BookmarkBrowser, emptyBookmarkFolders } from './lib/anda/bookmarks/browser.svelte'
import { BrainGraphData } from './lib/anda/brain/graph.svelte'
import { Channel } from './lib/anda/client/channel.svelte'
import { attachmentDownloadUrl } from './lib/anda/chat/attachment-view'
import { resolveInputTarget } from './lib/service-worker/page-scripts'
import { pageAudioCaptureDispatcher } from './lib/service-worker/page-audio'
import { VoiceRecorder } from './lib/anda/composer/recorder.svelte'

function deferred<T = any>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>((r) => {
    resolve = r
  })
  return { promise, resolve }
}
function event() {
  const listeners = new Set<(...args: any[]) => void>()
  return {
    addListener: (fn: any) => listeners.add(fn),
    removeListener: (fn: any) => listeners.delete(fn),
    emit: (...args: any[]) => {
      for (const fn of listeners) fn(...args)
    }
  }
}
afterEach(() => {
  vi.useRealTimers()
  vi.unstubAllGlobals()
  rememberActiveTab(null)
})

it('keeps the remaining YAML provider free of deleted fields', () => {
  const source =
    'model:\n  providers:\n    - model: first\n      disabled: true\n      api_base: https://first.example\n    - model: second\n'
  const draft = parseConfigDraft(source)!
  removeArrayItem(draft.model as any, 'providers', 0)
  const output = parseConfigDraft(renderConfigYaml(draft, source))!
  expect((output.model as any).providers).toEqual([{ model: 'second' }])
})

it('preserves approval policy when saving daemon configuration', async () => {
  const state: any = {
    baseUrl: 'http://localhost:8042',
    token: 'test',
    approvalMode: 'request_approval'
  }
  vi.stubGlobal('chrome', {
    storage: {
      local: {
        get: async (keys: string[]) =>
          Object.fromEntries(keys.filter((k) => k in state).map((k) => [k, state[k]])),
        set: async (items: any) => Object.assign(state, items)
      }
    }
  })
  await saveConfigSettings(await loadConfigSettings())
  expect(state.approvalMode).toBe('request_approval')
})

it('escapes raw HTML and malformed math', () => {
  const html = renderMarkdown(
    '<style>body { display: none }</style>\n<form action="https://example.test"><input name="token"></form>'
  )
  expect(html).not.toContain('<style>body { display: none }</style>')
  expect(html).not.toContain('<form action=')
  const math = renderMarkdown('$$\\invalidcommand <img src="https://example.test/probe">$$')
  expect(math).not.toContain('<img src="https://example.test/probe">')
})

it('stops recording in its original tab after tab switching', async () => {
  const calls: number[] = []
  const chrome: any = {
    storage: {},
    tabs: { get: async (id: number) => ({ id, active: true, url: 'https://example.test' }) },
    scripting: {
      executeScript: async (args: any) => {
        calls.push(args.target.tabId)
        return [{ result: { available: true, started: true } }]
      }
    }
  }
  rememberActiveTab(1)
  await handlePageAudioCapture(chrome, { action: 'start' })
  rememberActiveTab(2)
  await handlePageAudioCapture(chrome, { action: 'stop' })
  expect(calls).toEqual([1, 1])
})

it('accepts a navigation completion after a redirect', async () => {
  vi.useFakeTimers()
  const completed = event()
  const chrome: any = {
    tabs: {
      get: async () => ({ id: 1, status: 'complete', url: 'https://www.example.test/home' })
    },
    webNavigation: { onCommitted: event(), onCompleted: completed, onErrorOccurred: event() }
  }
  const watcher = createTabLoadWatcher(chrome, 1, 1000, {
    action: 'navigate',
    url: 'http://example.test'
  })
  const result = watcher.wait().then(
    () => 'resolved',
    (e: Error) => e.message
  )
  completed.emit({ tabId: 1, frameId: 0, url: 'https://www.example.test/home' })
  await vi.advanceTimersByTimeAsync(1000)
  expect(await result).toBe('resolved')
})

it('counts a redirected network request only once', async () => {
  vi.useFakeTimers()
  const onEvent = event()
  const chrome: any = {
    debugger: {
      onEvent,
      attach: async () => {},
      detach: async () => {},
      sendCommand: async () => ({})
    }
  }
  const result = waitForNetworkIdle(chrome, 1, {}, 2000).then(
    () => 'resolved',
    (e: Error) => e.message
  )
  await vi.advanceTimersByTimeAsync(0)
  onEvent.emit({ tabId: 1 }, 'Network.requestWillBeSent', { requestId: 'r' })
  onEvent.emit({ tabId: 1 }, 'Network.requestWillBeSent', { requestId: 'r', redirectResponse: {} })
  onEvent.emit({ tabId: 1 }, 'Network.loadingFinished', { requestId: 'r' })
  await vi.advanceTimersByTimeAsync(2000)
  expect(await result).toBe('resolved')
})

it('ignores an old folder response after selecting another folder', async () => {
  const a = deferred(),
    b = deferred()
  const browser = new BookmarkBrowser({
    listFolders: async () => emptyBookmarkFolders(),
    listInFolder: (id: number) => (id === 1 ? a.promise : b.promise)
  } as any)
  const first = browser.selectFolder(1)
  await Promise.resolve()
  const second = browser.selectFolder(2)
  await Promise.resolve()
  const item = (id: number) => ({
    _id: id,
    conversation: id,
    folder_ids: [id],
    messages: [{ index: 0, role: 'assistant', text: String(id) }]
  })
  b.resolve({ items: [item(2)], nextCursor: 'cursor-b' })
  await second
  a.resolve({ items: [item(1)], nextCursor: 'cursor-a' })
  await first
  expect(browser.activeFolder).toBe(2)
  expect(browser.items[0].conversation).toBe(2)
})

it('restores the cursor after repeated cached graph expansion', async () => {
  const graph = new BrainGraphData({
    executeKipReadonly: async () => ({
      kip: '2.0',
      status: 'succeeded',
      results: [0, 1].map(() => ({ status: 'succeeded', result: [] }))
    })
  } as any)
  graph.addConcept({ id: 'C-1', name: 'test', type: 'Event', attributes: {} })
  await graph.expandConcept('C-1')
  expect(graph.nodes.get('C-1')!._isExpanding).toBe(false)
  await graph.expandConcept('C-1')
  expect(graph.nodes.get('C-1')!._isExpanding).toBe(false)
})

const conversation = (id: number, ancestors: number[] = []) => ({
  _id: id,
  user: 'test',
  status: 'completed',
  ancestors,
  messages: [{ role: 'assistant', content: 'test' }],
  created_at: 1,
  updated_at: 1,
  usage: {}
})
it('discards previous history returned after clearing the conversation', async () => {
  const history = deferred()
  const channel = new Channel('browser:test', {
    activeChannel: () => 'browser:test',
    requestExtra: async () => ({}),
    updateStatus: () => {},
    rpc: async (_method: string, [input]: any[]) => {
      if (input.args.type === 'GetSourceState') return { output: { result: { c: 2 } } }
      if (input.args.type === 'GetConversation') return { output: { result: conversation(2, [1]) } }
      return history.promise
    }
  } as any)
  await channel.init()
  const loading = channel.loadPreviousConversations()
  await Promise.resolve()
  channel.clearConversation()
  history.resolve({ output: { result: [conversation(1)] } })
  await loading
  expect(channel.conversationId).toBe(0)
  expect(channel.messageGroups).toEqual([])
  channel.destroy()
})

it('recovers an accepted prompt without running it again', async () => {
  vi.useFakeTimers()
  let failed = false
  const rpc = vi.fn(async (method: string, [input]: any[]) => {
    if (method === 'agent_run') return { conversation: 1, session: 's' }
    if (!failed) {
      failed = true
      throw new Error('WebSocket connection closed')
    }
    return {
      output: {
        result:
          input.args.type === 'GetConversationDelta'
            ? { ...conversation(1), messages: [], artifacts: [] }
            : conversation(1)
      }
    }
  })
  const channel = new Channel('browser:test', {
    activeChannel: () => 'browser:test',
    requestExtra: async () => ({}),
    updateStatus: () => {},
    rpc
  } as any)
  const poller = await channel.sendPrompt('hello', [])
  expect(poller).not.toBeNull()
  poller?.close()
  await vi.advanceTimersByTimeAsync(60000)
  expect(rpc.mock.calls.filter(([method]) => method === 'agent_run')).toHaveLength(1)
  expect(channel.conversationId).toBe(1)
  channel.destroy()
})

it('requires a blob URL when attachment bytes and a provenance URI coexist', () => {
  const attachment: any = {
    id: 'page-element',
    name: 'page-content.json',
    type: 'application/json',
    resource: {
      _id: 0,
      uri: 'https://example.test/original-page',
      blob: btoa('{"text":"selected"}')
    }
  }
  expect(
    attachmentDownloadUrl(attachment, { resourceBlobs: new Map(), objectUrls: new Map() })
  ).toBe('')
})

it('disposes a microphone returned after startup was canceled', async () => {
  const pending = deferred()
  const recording = {
    dispose: vi.fn(),
    stop: vi.fn(),
    blob: Promise.resolve(new Blob()),
    mimeType: 'audio/webm'
  }
  const recorder = new VoiceRecorder({
    capabilities: () => ({ transcription: ['webm'], daemonTts: [], chromeTts: false }),
    ttsEnabled: () => false,
    send: async () => {},
    platform: {
      speechRecognitionSupported: () => false,
      startRecording: () => pending.promise,
      meter: () => () => {},
      now: () => 0
    } as any
  })
  const start = recorder.start()
  await recorder.cancel()
  pending.resolve(recording)
  await start
  expect(recorder.stage).toBe('idle')
  expect(recording.dispose).toHaveBeenCalledOnce()
  await recorder.cancel()
})

function mockClientChrome() {
  const sendMessage = vi.fn(async (message: any) => {
    const input = message.params?.[0]
    if (input?.args?.type === 'GetResource')
      return {
        ok: true,
        result: {
          output: {
            result: {
              _id: 1,
              name: message.settings.baseUrl,
              tags: [],
              blob: btoa(message.settings.baseUrl)
            }
          }
        }
      }
    if (input?.args?.type === 'ListSourceState')
      return { ok: true, result: { output: { result: {} } } }
    if (input?.args?.type === 'ReloadSkills')
      return { ok: true, result: { output: { result: [] } } }
    return { ok: true, result: {} }
  })
  const changed = event()
  const stored: any = { baseUrl: 'http://daemon-a', token: 'test' }
  vi.stubGlobal('chrome', {
    runtime: { sendMessage },
    storage: {
      local: {
        get: async (keys: string[]) =>
          Object.fromEntries(keys.filter((key) => key in stored).map((key) => [key, stored[key]])),
        set: async (items: any) => {
          Object.assign(stored, items)
          changed.emit(
            Object.fromEntries(Object.entries(items).map(([key, newValue]) => [key, { newValue }])),
            'local'
          )
        }
      },
      onChanged: changed
    },
    tabs: { onActivated: event(), onUpdated: event() },
    scripting: {},
    i18n: { getMessage: (key: string) => key }
  })
  return sendMessage
}

it('invalidates resource data when the daemon connection changes', async () => {
  const sendMessage = mockClientChrome()
  const { AndaSidePanelClient } = await import('./lib/anda/client/side-panel.svelte')
  const client = new AndaSidePanelClient()
  client.settings = {
    baseUrl: 'http://daemon-a',
    token: 'test',
    submitKeyMode: 'enter',
    appearanceTheme: 'system'
  }
  const summary = { _id: 1, name: 'resource', tags: [] }
  await client.loadResource(summary)
  await client.saveSettings({ ...client.settings, baseUrl: 'http://daemon-b' })
  expect((await client.loadResource(summary))?.name).toBe('http://daemon-b')
  expect(
    sendMessage.mock.calls.filter(([message]) => message.params?.[0]?.args?.type === 'GetResource')
  ).toHaveLength(2)
})

it('announces skills changes to the subscribed feature and persists a cross-page revision', async () => {
  mockClientChrome()
  const { AndaSidePanelClient } = await import('./lib/anda/client/side-panel.svelte')
  const client = new AndaSidePanelClient()
  client.settings.token = 'test'
  const appListener = vi.fn(),
    skillsListener = vi.fn()
  client.skills.addEventListener('skills-changed', appListener)
  client.skills.addEventListener('skills-changed', skillsListener)
  await client.skills.reload()
  expect(skillsListener).toHaveBeenCalledOnce()
  expect(appListener).toHaveBeenCalledOnce()
})

it('delivers fast assistant replies from the initial snapshot to TTS', async () => {
  const conv = conversation(1)
  const channel = new Channel('browser:test', {
    activeChannel: () => 'browser:test',
    requestExtra: async () => ({}),
    updateStatus: () => {},
    rpc: async (method: string, [input]: any[]) => {
      if (method === 'agent_run') return { conversation: 1, session: 's' }
      if (input.args.type === 'GetConversation') return { output: { result: conv } }
      return { output: { result: { ...conv, messages: [], artifacts: [] } } }
    }
  } as any)
  const poller = await channel.sendPrompt('hello', [])
  expect(await poller![Symbol.asyncIterator]().next()).toMatchObject({
    value: { text: 'test', role: 'assistant' },
    done: false
  })
  expect(channel.messageGroups[0].messages.some((m) => m.role === 'assistant')).toBe(true)
  channel.destroy()
})

it('translates an iframe input into top-level coordinates for native typing', () => {
  const top: any = { defaultView: { frameElement: null } }
  const frame: any = {
    tagName: 'IFRAME',
    ownerDocument: top,
    clientLeft: 2,
    clientTop: 2,
    offsetWidth: 400,
    offsetHeight: 200,
    getBoundingClientRect: () => ({ left: 100, top: 200, width: 400, height: 200 })
  }
  const child: any = { defaultView: { frameElement: frame } }
  const input: any = {
    tagName: 'INPUT',
    ownerDocument: child,
    readOnly: false,
    disabled: false,
    getAttribute: (name: string) => (name === 'type' ? 'text' : 'Email'),
    scrollIntoView: vi.fn(),
    getBoundingClientRect: () => ({ x: 10, y: 20, left: 10, top: 20, width: 60, height: 20 })
  }
  frame.contentDocument = child
  top.querySelectorAll = (selector: string) => (selector === '*' ? [frame] : [])
  child.querySelectorAll = () => [input]
  vi.stubGlobal('document', top)
  vi.stubGlobal('window', { getComputedStyle: () => ({ visibility: 'visible', display: 'block' }) })
  expect(resolveInputTarget({ action: 'type_text', selector: 'input' })).toMatchObject({
    native_text_input: true,
    x: 142,
    y: 232
  })
})

it('stops page microphone tracks if permission arrives after cancel', async () => {
  const stream = deferred()
  const stop = vi.fn()
  vi.stubGlobal('navigator', { mediaDevices: { getUserMedia: () => stream.promise } })
  vi.stubGlobal(
    'MediaRecorder',
    class {
      static isTypeSupported() {
        return false
      }
    }
  )
  const start = pageAudioCaptureDispatcher({ action: 'start' })
  await pageAudioCaptureDispatcher({ action: 'cancel' })
  stream.resolve({ getTracks: () => [{ stop }] })
  expect(await start).toMatchObject({ canceled: true, started: false })
  expect(stop).toHaveBeenCalledOnce()
})

it('initializes a dashboard without starting conversation reads or voice discovery', async () => {
  vi.useFakeTimers()
  const sendMessage = mockClientChrome()
  const { AndaSidePanelClient } = await import('./lib/anda/client/side-panel.svelte')
  const client = new AndaSidePanelClient()
  await client.init({ conversations: false })
  expect(client.activeChannel).toBeNull()
  expect(client.channels.size).toBe(0)
  expect(sendMessage.mock.calls.map(([message]) => message.method)).not.toContain('tool_call')
  expect(sendMessage.mock.calls.map(([message]) => message.method)).not.toContain('capabilities')
  const changed = vi.fn()
  client.skills.addEventListener('skills-changed', changed)
  await chrome.storage.local.set({ skillsRevision: 'other-page-revision', appearanceTheme: 'dark' })
  expect(changed).toHaveBeenCalledOnce()
  expect(client.settings.appearanceTheme).toBe('dark')
  client.destroy()
})

it.each(['request_approval', 'on_risk', 'full_access', 'custom'])(
  'preserves %s when Brain saves its own settings',
  async (approvalMode) => {
    const state: any = { baseUrl: 'http://localhost:8042', token: 'test', approvalMode }
    vi.stubGlobal('chrome', {
      storage: {
        local: {
          get: async (keys: string[]) =>
            Object.fromEntries(keys.filter((key) => key in state).map((key) => [key, state[key]])),
          set: async (items: any) => Object.assign(state, items)
        }
      }
    })
    const settings = await loadBrainGraphSettings()
    await saveBrainGraphSettings({ ...settings, appearanceTheme: 'dark' })
    expect(state.approvalMode).toBe(approvalMode)
    expect(state.appearanceTheme).toBe('dark')
  }
)

it('retains existing message objects when appending to a long conversation', () => {
  const cache: NormalizedMessageCache = new WeakMap()
  const messages = Array.from({ length: 1000 }, (_, index) => ({
    role: 'assistant' as const,
    content: [{ type: 'Text' as const, text: `message ${index}` }]
  }))
  const first = conversationToGroup({ ...conversation(1), messages } as any, cache)
  const next = conversationToGroup(
    {
      ...conversation(1),
      messages: [...messages, { role: 'assistant', content: [{ type: 'Text', text: 'new' }] }]
    } as any,
    cache
  )
  expect(next.messages).toHaveLength(1001)
  expect(
    next.messages.slice(0, 1000).every((message, index) => message === first.messages[index])
  ).toBe(true)
})

it('settles an in-flight page stop even when capture is canceled before its stop event', async () => {
  const stopTrack = vi.fn()
  vi.stubGlobal('navigator', {
    mediaDevices: { getUserMedia: async () => ({ getTracks: () => [{ stop: stopTrack }] }) }
  })
  class Recorder extends EventTarget {
    static isTypeSupported() {
      return false
    }
    state = 'inactive'
    mimeType = 'audio/webm'
    start() {
      this.state = 'recording'
    }
    stop() {
      this.state = 'inactive'
      queueMicrotask(() => this.dispatchEvent(new Event('stop')))
    }
  }
  vi.stubGlobal('MediaRecorder', Recorder)
  await pageAudioCaptureDispatcher({ action: 'start' })
  const stopped = pageAudioCaptureDispatcher({ action: 'stop' })
  await pageAudioCaptureDispatcher({ action: 'cancel' })
  await expect(stopped).resolves.toMatchObject({ canceled: true })
  expect(stopTrack).toHaveBeenCalled()
})

it('refuses to save a configuration draft to an obsolete daemon connection', async () => {
  mockClientChrome()
  const oldSettings = await loadConfigSettings()
  await chrome.storage.local.set({ baseUrl: 'http://daemon-b', token: 'new-token' })
  const fetch = vi.fn()
  vi.stubGlobal('fetch', fetch)
  await expect(new DaemonConfigApi(oldSettings).save('log_level: info')).rejects.toThrow(
    'configConnectionChanged'
  )
  expect(fetch).not.toHaveBeenCalled()
})
