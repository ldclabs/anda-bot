export type SubmitKeyMode = 'enter' | 'modifier-enter'
export type AppearanceTheme = 'system' | 'light' | 'dark'
export type ApprovalMode = 'request_approval' | 'on_risk' | 'full_access' | 'custom'

export interface SettingsState {
  baseUrl: string
  token: string
  submitKeyMode: SubmitKeyMode
  appearanceTheme: AppearanceTheme
  approvalMode?: ApprovalMode
}

export interface QuickPrompt {
  id: string
  text: string
  createdAt: number
  updatedAt: number
  usedAt: number
  useCount: number
}

export type StorageState = Partial<SettingsState> & {
  browserSessionId?: string
  workspaceChannelSources?: string[]
  uiLanguage?: string
  quickPrompts?: QuickPrompt[]
  skillsRevision?: string
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
  incognito?: boolean
}

export type ChromeRuntimeOnInstalledDetails = {
  reason: string
  previousVersion?: string
  id?: string
}

export type ChromeContextMenuClickInfo = {
  menuItemId?: string | number
  pageUrl?: string
  frameUrl?: string
  frameId?: number
}

export type ChromeDownloadItem = {
  id?: number
  url?: string
  finalUrl?: string
  filename?: string
  state?: string
  paused?: boolean
  error?: string
  bytesReceived?: number
  totalBytes?: number
  startTime?: string
  endTime?: string
  exists?: boolean
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
  full_page?: boolean
  viewport_width?: number
  viewport_height?: number
  device_scale_factor?: number
  highlight?: boolean
  bypass_cache?: boolean
  behavior?: ScrollBehavior
  filename?: string
  save_as?: boolean
  download_id?: number
  files?: string[]
  path?: string
  accept?: boolean
  prompt_text?: string
  max_chars?: number
  timeout_ms?: number
  reason?: string
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
  pageElementRequest?: unknown
}

export type ChromeMessageSender = {
  id?: string
  url?: string
  tab?: ChromeTabInfo
  frameId?: number
}

export type ExtensionResponse<Result = unknown> =
  | { ok: true; result?: Result; status?: string }
  | { ok: false; error: string; status?: string }

export type ChromeRuntimeMessageListener = (
  message: ExtensionMessage,
  sender: ChromeMessageSender,
  sendResponse: (response: ExtensionResponse) => void
) => boolean | void

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
  addListener(listener: Listener, ...extraParameters: unknown[]): void
  removeListener(listener: Listener): void
}

export type ChromeWebNavigationDetails = {
  tabId: number
  frameId: number
  parentFrameId?: number
  processId?: number
  url: string
  timeStamp?: number
  error?: string
  transitionType?: string
  transitionQualifiers?: string[]
}

export type ChromeWebNavigationTabReplacedDetails = {
  replacedTabId: number
  tabId: number
  timeStamp?: number
}

export type ChromeWebNavigationFrame = {
  frameId: number
  parentFrameId?: number
  processId?: number
  url: string
  errorOccurred?: boolean
}

export interface ChromeApi {
  runtime: {
    lastError?: { message?: string }
    onInstalled: ChromeEvent<(details: ChromeRuntimeOnInstalledDetails) => void>
    onStartup: ChromeEvent<() => void>
    getURL(path: string): string
    sendMessage<Result>(message: ExtensionMessage): Promise<ExtensionResponse<Result>>
    onMessage: {
      addListener(listener: ChromeRuntimeMessageListener): void
      removeListener(listener: ChromeRuntimeMessageListener): void
    }
  }
  management?: {
    getSelf(): Promise<{ installType?: string }>
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
    isAllowedFileSchemeAccess?(callback: (isAllowedAccess: boolean) => void): void
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
    session?: {
      get(keys: string[] | string): Promise<Record<string, unknown>>
      set(items: Record<string, unknown>): Promise<void>
      remove(keys: string[] | string): Promise<void>
    }
    onChanged?: {
      removeListener?(
        callback: (
          changes: Record<string, { newValue?: unknown; oldValue?: unknown }>,
          areaName: string
        ) => void
      ): void
      addListener?(
        callback: (
          changes: Record<string, { newValue?: unknown; oldValue?: unknown }>,
          areaName: string
        ) => void
      ): void
    }
  }
  contextMenus?: {
    create(properties: { id: string; title: string; contexts: string[] }): void | Promise<void>
    remove?(menuItemId: string): void | Promise<void>
    update?(menuItemId: string, properties: { title?: string }): void | Promise<void>
    onClicked: ChromeEvent<
      (info: ChromeContextMenuClickInfo, tab?: ChromeTabInfo | undefined) => void
    >
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
    goBack?(tabId?: number): Promise<void>
    goForward?(tabId?: number): Promise<void>
    captureVisibleTab(windowId: number | undefined, options: { format: 'png' }): Promise<string>
    onActivated: ChromeEvent<(activeInfo: { tabId: number; windowId: number }) => void>
    onUpdated: ChromeEvent<
      (
        tabId: number,
        changeInfo: { title?: string; url?: string; status?: string },
        tab: ChromeTabInfo
      ) => void
    >
  }
  windows?: {
    update(windowId: number, updateInfo: { focused?: boolean }): Promise<unknown>
    onFocusChanged?: ChromeEvent<(windowId: number) => void>
  }
  downloads?: {
    download(options: { url: string; filename?: string; saveAs?: boolean }): Promise<number>
    search(query: {
      id?: number
      limit?: number
      orderBy?: string[]
      state?: string
    }): Promise<ChromeDownloadItem[]>
    cancel(downloadId: number): Promise<void>
    show?(downloadId: number): void | Promise<void>
  }
  webNavigation?: {
    onBeforeNavigate?: ChromeEvent<(details: ChromeWebNavigationDetails) => void>
    onCommitted?: ChromeEvent<(details: ChromeWebNavigationDetails) => void>
    onDOMContentLoaded?: ChromeEvent<(details: ChromeWebNavigationDetails) => void>
    onCompleted?: ChromeEvent<(details: ChromeWebNavigationDetails) => void>
    onErrorOccurred?: ChromeEvent<(details: ChromeWebNavigationDetails) => void>
    onReferenceFragmentUpdated?: ChromeEvent<(details: ChromeWebNavigationDetails) => void>
    onHistoryStateUpdated?: ChromeEvent<(details: ChromeWebNavigationDetails) => void>
    onTabReplaced?: ChromeEvent<(details: ChromeWebNavigationTabReplacedDetails) => void>
    getFrame?(details: {
      tabId: number
      frameId: number
      processId?: number
    }): Promise<ChromeWebNavigationFrame | null>
    getAllFrames?(details: { tabId: number }): Promise<ChromeWebNavigationFrame[] | null>
  }
  debugger?: {
    attach(target: { tabId: number }, requiredVersion: string): Promise<void>
    detach(target: { tabId: number }): Promise<void>
    onEvent?: ChromeEvent<
      (source: { tabId?: number }, method: string, params?: Record<string, unknown>) => void
    >
    sendCommand<Result = unknown>(
      target: { tabId: number },
      method: string,
      commandParams?: Record<string, unknown>
    ): Promise<Result>
  }
  scripting: {
    executeScript<Result, Args>(details: {
      target: { tabId: number; frameIds?: number[]; allFrames?: boolean }
      world?: 'ISOLATED' | 'MAIN'
      func: (args: Args) => Result | Promise<Result>
      args: [Args]
      files?: never
    }): Promise<Array<{ result: Awaited<Result> }>>
    executeScript(details: {
      target: { tabId: number; frameIds?: number[]; allFrames?: boolean }
      world?: 'ISOLATED' | 'MAIN'
      files: string[]
      func?: never
      args?: never
    }): Promise<Array<{ result: unknown }>>
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
