export type SubmitKeyMode = 'enter' | 'modifier-enter'

export interface SettingsState {
  baseUrl: string
  token: string
  submitKeyMode: SubmitKeyMode
}

export type StorageState = Partial<SettingsState> & {
  browserSessionId?: string
}

export type ChromeTabInfo = {
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

export type BrowserActionArgs = {
  action?: string
  url?: string
  selector?: string
  text?: string
  value?: string
  code?: string
  world?: string
  use_bridge?: boolean
  query?: string
  key?: string
  amount?: number
  x?: number
  y?: number
  to_x?: number
  to_y?: number
  from_selector?: string
  to_selector?: string
  tab_id?: number
  window_id?: number
  frame_id?: number
  active?: boolean
  include_links?: boolean
  include_forms?: boolean
  include_data_url?: boolean
  highlight?: boolean
  bypass_cache?: boolean
  behavior?: ScrollBehavior
  max_chars?: number
  timeout_ms?: number
}

export type BrowserCommand = {
  session: string
  request_id: number
  args?: BrowserActionArgs
}

export type BrowserActionResult = unknown

export type ExtensionMessage = {
  type?: string
  settings?: SettingsState
  method?: string
  params?: unknown[]
  text?: string
  language?: string
  mimeType?: string
}

export type ExtensionResponse<Result = unknown> =
  | { ok: true; result?: Result; status?: string }
  | { ok: false; error: string; status?: string }

export type RpcResponseMessage = {
  id?: number
  method?: string
  params?: unknown
  result?: unknown
  error?: string
}

export type PendingRpc = {
  resolve: (value: unknown) => void
  reject: (error: Error) => void
  timeout: ReturnType<typeof setTimeout>
}

export interface ChromeEvent<Listener extends (...args: never[]) => void> {
  addListener(listener: Listener): void
  removeListener(listener: Listener): void
}

export interface ChromeApi {
  runtime: {
    lastError?: { message?: string }
    onInstalled: ChromeEvent<(raeson: string) => void>
    onStartup: ChromeEvent<() => void>
    sendMessage<Result>(message: ExtensionMessage): Promise<ExtensionResponse<Result>>
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
  tts?: {
    speak(
      utterance: string,
      options?: {
        enqueue?: boolean
        rate?: number
        pitch?: number
        volume?: number
        requiredEventTypes?: string[]
        desiredEventTypes?: string[]
        onEvent?: (event: { type?: string; errorMessage?: string }) => void
      },
      callback?: () => void
    ): void
    stop?(): void
    getVoices?(callback: (voices: unknown[]) => void): void
  }
  extension?: {
    inIncognitoContext?: boolean
  }
  action: {
    onClicked: ChromeEvent<(tab: ChromeTabInfo) => void>
  }
  i18n: typeof chrome.i18n
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
    reload(tabId?: number, reloadProperties?: { bypassCache?: boolean }): Promise<void>
    captureVisibleTab(windowId: number | undefined, options: { format: 'png' }): Promise<string>
    onActivated: ChromeEvent<(activeInfo: { tabId: number; windowId: number }) => void>
    onUpdated: ChromeEvent<
      (tabId: number, changeInfo: { title?: string; url?: string }, tab: ChromeTabInfo) => void
    >
  }
  windows?: {
    update(windowId: number, updateInfo: { focused?: boolean }): Promise<unknown>
  }
  debugger?: {
    attach(target: { tabId: number }, requiredVersion: string): Promise<void>
    detach(target: { tabId: number }): Promise<void>
    sendCommand<Result = unknown>(
      target: { tabId: number },
      method: string,
      commandParams?: Record<string, unknown>
    ): Promise<Result>
  }
  scripting: {
    executeScript<Result, Args>(details: {
      target: { tabId: number; frameIds?: number[] }
      world?: 'ISOLATED' | 'MAIN'
      func: (args: Args) => Result | Promise<Result>
      args: [Args]
    }): Promise<Array<{ result: Awaited<Result> }>>
  }
}

export type PageSpeechAction = 'available' | 'start' | 'stop' | 'cancel'

export type PageSpeechArgs = {
  action: PageSpeechAction
  language?: string
}

export type PageSpeechResult = {
  available?: boolean
  started?: boolean
  transcript?: string
  canceled?: boolean
  error?: string
}

export type PageAudioAction = 'available' | 'start' | 'stop' | 'cancel'

export type PageAudioArgs = {
  action: PageAudioAction
  mimeType?: string
}

export type PageAudioResult = {
  available?: boolean
  started?: boolean
  audioBase64?: string
  mimeType?: string
  size?: number
  canceled?: boolean
  error?: string
}
