import {
  app,
  autoUpdater,
  BrowserWindow,
  Menu,
  Tray,
  nativeImage,
  ipcMain,
  dialog,
  Notification,
  shell,
  nativeTheme,
  powerMonitor,
  systemPreferences,
  screen,
  protocol,
  net,
  clipboard
} from 'electron'
import { join, resolve } from 'node:path'
import { homedir } from 'node:os'
import { pathToFileURL } from 'node:url'
import { createHash } from 'node:crypto'
import { mkdir, readFile, realpath, writeFile } from 'node:fs/promises'
import { GitService } from './git'
import { TerminalService } from './terminal'
import { BrowserService } from './browser'
import { DesktopUpdater } from './updater'
import type { Bootstrap, DaemonView, NativeEvent, Preferences } from '../shared/contract'
import { DesktopStore } from './store'
import { DaemonClient, type InstallReport } from './daemon-client'
import { isOlderRelease } from './update-machine'
import {
  appPermissionAllowed,
  authorizeWorkspace,
  externalUrl,
  navigationSource,
  rendererAssetPath
} from './policy'
import { label, type Label } from '../renderer/labels'

protocol.registerSchemesAsPrivileged([
  {
    scheme: 'anda-app',
    privileges: {
      standard: true,
      secure: true,
      supportFetchAPI: true,
      corsEnabled: true,
      stream: true
    }
  }
])
app.setName('Anda')
const homeArg = process.argv.find((arg) => arg.startsWith('--anda-home='))?.slice(12)
const profileArg = process.argv.find((arg) => arg.startsWith('--anda-profile='))?.slice(15)
const testMode = !app.isPackaged && process.env.ANDA_DESKTOP_TEST === '1'
if (testMode) {
  app.commandLine.appendSwitch('use-fake-device-for-media-stream')
  app.commandLine.appendSwitch('use-fake-ui-for-media-stream')
}
if (process.platform === 'win32') app.setAppUserModelId('org.ldclabs.anda.desktop')
if (testMode && !process.env.ANDA_DESKTOP_USER_DATA)
  throw new Error('Desktop tests require an isolated profile')
const home = resolve(
  testMode
    ? join(process.env.ANDA_DESKTOP_USER_DATA!, 'anda-home')
    : homeArg || process.env.ANDA_HOME || join(homedir(), '.anda')
)
if (testMode && process.env.ANDA_DESKTOP_USER_DATA)
  app.setPath('userData', process.env.ANDA_DESKTOP_USER_DATA)
else if (profileArg) app.setPath('userData', resolve(profileArg))
else if (homeArg || process.env.ANDA_HOME)
  app.setPath(
    'userData',
    `${app.getPath('userData')}-${createHash('sha256').update(home).digest('hex').slice(0, 12)}`
  )
const hasLock = app.requestSingleInstanceLock()
if (!hasLock) app.quit()

let window: BrowserWindow | null = null
let hideAfterFullScreen = false
let tray: Tray | null = null
let quitting = false
let stateLoaded = false
let boundsTimer: NodeJS.Timeout | undefined
let store: DesktopStore
let daemon: DaemonClient
let git: GitService
let terminals: TerminalService
let browser: BrowserService
let updater: DesktopUpdater
let quitPrompt = false
let upgradeNoticeShown = false
let pendingNavigation: string | null = null
let rendererReady = false
let pendingMenuAction: 'new-chat' | 'settings' | 'updates' | null = null
let reconnectTimer: NodeJS.Timeout | undefined
const firstReconnectDelay = 10_000
let reconnectDelay = firstReconnectDelay
const notificationTimes = new Map<string, number>()
const applicationUrl = 'anda-app://app/index.html'
const rendererUrl = process.env.ELECTRON_RENDERER_URL

function emit(event: NativeEvent): void {
  window?.webContents.send('anda:event', event)
}
function t(key: Label): string {
  return label(store.state.preferences.language, key)
}
/** Explicit reconnects start the service; retries after a backoff only reconnect. */
function connectNow(start = true): Promise<DaemonView> {
  reconnectDelay = firstReconnectDelay
  return daemon.connect(start)
}
/** Shows `body` in the window when it is open, else as a system notification. */
function inform(body: string): void {
  if (window?.isVisible()) emit({ type: 'update', value: body })
  else if (Notification.isSupported()) new Notification({ title: 'Anda', body }).show()
}
async function setLaunchAtLogin(enabled: boolean): Promise<void> {
  store.state.preferences.launchAtLogin = enabled
  if (app.isPackaged) app.setLoginItemSettings({ openAtLogin: enabled, args: ['--hidden'] })
  await store.save()
  emit({ type: 'preferences', value: store.state.preferences })
}
/**
 * Mirrors the UI language to `launcher/ui.json` in the Anda home: the daemon
 * localizes approval cards from it and the Chrome extension follows it. The
 * path is the one the retired tray launcher wrote.
 */
async function persistUiLanguage(language: string): Promise<void> {
  const tag = language.toLowerCase().startsWith('zh') ? 'zh-Hans' : language.split(/[-_]/)[0]
  const content = JSON.stringify({
    language: ['en', 'zh-Hans', 'fr', 'es', 'ru', 'ar'].includes(tag!) ? tag : 'en'
  })
  const path = join(home, 'launcher', 'ui.json')
  if ((await readFile(path, 'utf8').catch(() => '')) === content) return
  await mkdir(join(home, 'launcher'), { recursive: true })
  await writeFile(path, content)
}
/** Follows up on the bundled runtime's `anda install` report. */
async function onInstalled(report: InstallReport): Promise<void> {
  if (report.launcher_started_at_login && !store.state.preferences.launchAtLogin)
    // The retired launcher kept a tray at login; this app's tray takes over.
    await setLaunchAtLogin(true)
  // First-use model setup (including the login-start preference) lives in the
  // renderer, after we know whether the shared daemon actually needs a model.
  if (report.launcher_retired) inform(t('launcherRetired'))
  const bundled = await daemon.bundledRelease()
  if (report.action === 'homebrew' && report.version && bundled)
    if (isOlderRelease(report.version, bundled)) inform(t('brewOutdated'))
}
async function controlDaemon(action: 'stop' | 'restart'): Promise<DaemonView> {
  const options: Electron.MessageBoxOptions = {
    type: 'warning',
    message: action === 'stop' ? 'Stop the Anda daemon?' : 'Restart the Anda daemon?',
    detail:
      'Running agent tasks, IM channels, and scheduled jobs will be interrupted. Closing the desktop window alone keeps them running.',
    buttons: ['Cancel', action === 'stop' ? 'Stop daemon' : 'Restart daemon'],
    defaultId: 0,
    cancelId: 0
  }
  const result = await (window
    ? dialog.showMessageBox(window, options)
    : dialog.showMessageBox(options))
  if (result.response !== 1) return daemon.view
  clearTimeout(reconnectTimer)
  return daemon.control(action)
}
async function copyExtensionToken(): Promise<void> {
  await clipboard.writeText(await daemon.extensionToken())
  inform(t('tokenCopied'))
}
function refreshTray(): void {
  if (!tray) return
  const view = daemon.view
  const release = updater?.runtimeRelease
  tray.setContextMenu(
    Menu.buildFromTemplate([
      { label: t('openAnda'), click: show },
      { label: t('newChat'), click: () => menuAction('new-chat') },
      { type: 'separator' },
      {
        label: view.connected
          ? `${t('serviceRunning')}${view.version ? ` · v${view.version}` : ''}`
          : t('serviceStopped'),
        enabled: false
      },
      { label: t('restartDaemon'), click: () => void controlDaemon('restart').catch(() => {}) },
      { label: t('update'), click: () => runUpdate(() => updater.check()) },
      ...(release
        ? [
            {
              label: `${t('installUpdate')} (anda ${release})`,
              click: () => runUpdate(() => updater.installRuntime())
            }
          ]
        : []),
      ...(updater?.desktopRelease
        ? [
            {
              label: t('desktopUpdateAvailable').replace('{version}', updater.desktopRelease),
              click: () => runUpdate(() => updater.checkDesktop())
            }
          ]
        : []),
      {
        label: t('extensionToken'),
        click: () =>
          void copyExtensionToken().catch((error) =>
            inform(error instanceof Error ? error.message : String(error))
          )
      },
      { label: t('logs'), click: () => void showLogs() },
      { type: 'separator' },
      { label: t('quitDesktop'), click: () => app.quit() }
    ])
  )
}
function openExternal(url: unknown): void {
  try {
    void shell.openExternal(externalUrl(url)).catch(() => {})
  } catch {
    /* Non-web schemes never launch. */
  }
}
function runUpdate(action: () => Promise<string>): Promise<string> {
  menuAction('updates')
  // The updater publishes progress and retains the result for a loading renderer.
  return action().catch((error) => (error instanceof Error ? error.message : String(error)))
}
function menuAction(value: 'new-chat' | 'settings' | 'updates'): void {
  show()
  if (rendererReady) emit({ type: 'menu', value })
  else pendingMenuAction = value
}
function showLogs(): Promise<string> {
  return shell.openPath(join(home, 'logs'))
}
function titleBarOverlay(): Electron.TitleBarOverlayOptions {
  return nativeTheme.shouldUseDarkColors
    ? { color: '#1b1b1a', symbolColor: '#e8e8e2', height: 42 }
    : { color: '#f4f4f3', symbolColor: '#252525', height: 42 }
}
function show(): void {
  if (!stateLoaded) return
  hideAfterFullScreen = false
  if (!window) createWindow()
  if (window?.isMinimized()) window.restore()
  window?.show()
  window?.focus()
}
function navigate(raw: string): void {
  const source = navigationSource(raw)
  if (!source) return
  pendingNavigation = source
  if (app.isReady()) {
    show()
    emit({ type: 'navigate', value: source })
  }
}
app.on('second-instance', (_event, argv) => {
  if (!app.isReady()) return
  show()
  for (const arg of argv) if (arg.startsWith('anda:')) navigate(arg)
})
app.on('open-url', (event, url) => {
  event.preventDefault()
  navigate(url)
})
app.on('activate', () => {
  if (app.isReady()) show()
})
app.on('before-quit', (event) => {
  if (quitting) return
  if (terminals?.running) {
    event.preventDefault()
    if (quitPrompt) return
    quitPrompt = true
    void dialog
      .showMessageBox({
        type: 'warning',
        message: `Close ${terminals.running} running terminal(s) and quit?`,
        detail: 'Commands in these terminals will stop. Agent tasks in the daemon continue.',
        buttons: ['Cancel', 'Close terminals and quit'],
        cancelId: 0,
        defaultId: 0
      })
      .then(async ({ response }) => {
        quitPrompt = false
        if (response === 1) {
          await terminals.closeAll()
          app.quit()
        }
      })
    return
  }
  quitting = true
  updater?.stop()
  browser?.destroy()
  clearTimeout(reconnectTimer)
  clearTimeout(boundsTimer)
  daemon?.disconnect()
  if (stateLoaded) {
    event.preventDefault()
    if (window && !window.isDestroyed()) store.state.windowBounds = window.getNormalBounds()
    void store
      .save()
      .catch(() => {})
      .finally(() => app.quit())
  }
})
app.on('window-all-closed', () => {
  /* Tray owns the UI lifetime until explicit Quit. */
})
// quitAndInstall closes every window before `before-quit`; a hidden window
// would keep the update (and the stopped runtime) waiting indefinitely.
autoUpdater.on('before-quit-for-update', () => {
  quitting = true
  updater?.stop()
})

function createWindow(): void {
  rendererReady = false
  const saved = store?.state.windowBounds
  const valid =
    saved &&
    Object.values(saved).every(Number.isFinite) &&
    saved.width >= 760 &&
    saved.height >= 560
  const visible =
    valid &&
    screen
      .getAllDisplays()
      .some(
        ({ workArea: area }) =>
          saved.x < area.x + area.width - 80 &&
          saved.x + saved.width > area.x + 80 &&
          saved.y < area.y + area.height - 80 &&
          saved.y + saved.height > area.y + 80
      )
  window = new BrowserWindow({
    width: valid ? saved.width : 1280,
    height: valid ? saved.height : 860,
    ...(visible ? { x: saved.x, y: saved.y } : {}),
    minWidth: 760,
    minHeight: 560,
    show: false,
    title: 'Anda',
    backgroundColor: nativeTheme.shouldUseDarkColors ? '#191919' : '#fcfcfb',
    titleBarStyle: 'hidden',
    ...(process.platform === 'darwin'
      ? { trafficLightPosition: { x: 18, y: 20 } }
      : { titleBarOverlay: titleBarOverlay() }),
    webPreferences: {
      preload: join(__dirname, '../preload/index.js'),
      contextIsolation: true,
      sandbox: true,
      nodeIntegration: false,
      webSecurity: true
    }
  })
  const rememberBounds = () => {
    clearTimeout(boundsTimer)
    boundsTimer = setTimeout(() => {
      if (window && !window.isDestroyed()) {
        store.state.windowBounds = window.getNormalBounds()
        void store.save().catch(() => {})
      }
    }, 400)
  }
  window.on('resize', rememberBounds)
  window.on('move', rememberBounds)
  const contents = window.webContents
  contents.setWindowOpenHandler(({ url }) => {
    openExternal(url)
    return { action: 'deny' }
  })
  contents.on('will-navigate', (event, url) => {
    if (url === contents.getURL()) return
    // Local/arbitrary navigation is blocked; only web links leave the app.
    event.preventDefault()
    openExternal(url)
  })
  contents.session.setPermissionRequestHandler((webContents, permission, callback, details) => {
    if (!appPermissionAllowed(contents.id, webContents.id, permission, details))
      return callback(false)
    if (permission === 'media' && !testMode && process.platform === 'darwin')
      void systemPreferences
        .askForMediaAccess('microphone')
        .then(callback)
        .catch(() => callback(false))
    else callback(true)
  })
  contents.session.setPermissionCheckHandler((webContents, permission, _origin, details) =>
    appPermissionAllowed(contents.id, webContents?.id, permission, details)
  )
  contents.session.on('will-download', (_event, item) => {
    if (!/^blob:|^data:/.test(item.getURL())) item.cancel()
  })
  const mainWindow = window
  let leavingFullScreenForClose = false
  mainWindow.on('leave-full-screen', () => {
    leavingFullScreenForClose = false
    if (!hideAfterFullScreen) return
    hideAfterFullScreen = false
    if (!quitting && !mainWindow.isDestroyed()) mainWindow.hide()
  })
  mainWindow.on('close', (event) => {
    if (quitting) return
    event.preventDefault()
    if (leavingFullScreenForClose) {
      hideAfterFullScreen = true
      return
    }
    if (process.platform === 'darwin' && mainWindow.isFullScreen()) {
      // Hiding a native fullscreen window leaves an empty macOS Space behind.
      // Wait for the asynchronous exit before hiding; show() cancels the hide.
      hideAfterFullScreen = true
      leavingFullScreenForClose = true
      mainWindow.setFullScreen(false)
    } else mainWindow.hide()
  })
  window.on('closed', () => {
    window = null
    hideAfterFullScreen = false
    rendererReady = false
  })
  window.once('ready-to-show', () => {
    if (!process.argv.includes('--hidden')) window?.show()
  })
  contents.on('did-finish-load', () => {
    if (pendingNavigation) {
      emit({ type: 'navigate', value: pendingNavigation })
      pendingNavigation = null
    }
  })
  contents.on('did-start-loading', () => {
    rendererReady = false
  })
  contents.on('render-process-gone', () => {
    rendererReady = false
    void dialog
      .showMessageBox({
        type: 'error',
        message: 'Anda’s window needs to reload',
        detail: 'Your background tasks continue in the daemon.',
        buttons: ['Reload', 'Close']
      })
      .then(({ response }) => {
        if (response === 0) contents.reload()
      })
  })
  if (rendererUrl && !app.isPackaged) void window.loadURL(rendererUrl)
  else void window.loadURL(applicationUrl)
}

function checkSender(event: Electron.IpcMainInvokeEvent): void {
  if (
    !window ||
    event.sender.id !== window.webContents.id ||
    event.senderFrame !== event.sender.mainFrame
  )
    throw new Error('Untrusted IPC sender')
  const current = new URL(event.senderFrame.url)
  const expected = new URL(rendererUrl && !app.isPackaged ? rendererUrl : applicationUrl)
  if (
    current.protocol !== expected.protocol ||
    (current.protocol === 'file:'
      ? current.pathname !== expected.pathname
      : current.origin !== expected.origin)
  )
    throw new Error('Untrusted application origin')
}
function handle(channel: string, fn: (...args: any[]) => unknown): void {
  ipcMain.handle(channel, (event, ...args) => {
    checkSender(event)
    return fn(...args)
  })
}
function validatePreferences(patch: Partial<Preferences>): void {
  if (
    !patch ||
    typeof patch !== 'object' ||
    Array.isArray(patch) ||
    Buffer.byteLength(JSON.stringify(patch)) > 4 * 1024 * 1024
  )
    throw new Error('Invalid preferences')
  const allowed = new Set([
    'theme',
    'language',
    'submitKeyMode',
    'approvalMode',
    'notifications',
    'launchAtLogin',
    'sidebarWidth',
    'chats',
    'projects',
    'activeSource',
    'drafts'
  ])
  if (Object.keys(patch).some((key) => !allowed.has(key))) throw new Error('Unknown preference')
  if (patch.theme && !['system', 'light', 'dark'].includes(patch.theme))
    throw new Error('Invalid theme')
  if (
    patch.approvalMode &&
    !['on_risk', 'request_approval', 'full_access', 'custom'].includes(patch.approvalMode)
  )
    throw new Error('Invalid approval mode')
  if (
    patch.sidebarWidth !== undefined &&
    (typeof patch.sidebarWidth !== 'number' ||
      !(patch.sidebarWidth >= 120 && patch.sidebarWidth <= 800))
  )
    throw new Error('Invalid sidebar width')
  if (
    patch.chats &&
    (!Array.isArray(patch.chats) ||
      patch.chats.some((c) => typeof c.source !== 'string' || typeof c.title !== 'string'))
  )
    throw new Error('Invalid chats')
  if (
    patch.projects &&
    (!Array.isArray(patch.projects) ||
      patch.projects.some((p) => typeof p.path !== 'string' || typeof p.id !== 'string'))
  )
    throw new Error('Invalid projects')
  if (
    patch.drafts &&
    (typeof patch.drafts !== 'object' ||
      Object.values(patch.drafts).some((v) => typeof v !== 'string'))
  )
    throw new Error('Invalid drafts')
}

async function bootstrap(): Promise<Bootstrap> {
  const updateRequested = pendingMenuAction === 'updates'
  // The update dialog must not wait for a slow or unavailable daemon connection.
  // Opening the window still starts the service; connection events update the renderer.
  if (updateRequested && !daemon.manuallyStopped) void daemon.connect().catch(() => {})
  const state = updateRequested || daemon.manuallyStopped ? daemon.view : await daemon.connect()
  return {
    daemon: state,
    preferences: store.state.preferences,
    platform: process.platform,
    version: app.getVersion(),
    pending: store.state.pending,
    update: updater.status,
    updateRequested
  }
}

async function setup(): Promise<void> {
  protocol.handle('anda-app', async (request) => {
    if (!['GET', 'HEAD'].includes(request.method)) return new Response(null, { status: 405 })
    try {
      return await net.fetch(
        pathToFileURL(rendererAssetPath(resolve(__dirname, '../renderer'), request.url)).href
      )
    } catch {
      return new Response('Not found', { status: 404 })
    }
  })
  store = new DesktopStore(join(app.getPath('userData'), 'desktop.json'))
  await store.load()
  stateLoaded = true
  store.state.preferences.language ||= testMode ? 'en' : app.getLocale()
  daemon = new DaemonClient(
    home,
    app.isPackaged ? process.resourcesPath : resolve(__dirname, '../../resources'),
    store,
    testMode ? process.env.ANDA_DESKTOP_TEST_URL : undefined,
    app.isPackaged && !testMode
  )
  daemon.on('installed', (report: InstallReport) => void onInstalled(report).catch(() => {}))
  daemon.on('change', (state) => {
    emit({ type: 'connection', value: state })
    refreshTray()
    const installed = daemon.installReport
    if (
      state.connected &&
      state.version &&
      installed?.action === 'upgraded' &&
      installed.version !== `v${state.version}` &&
      !upgradeNoticeShown
    ) {
      // The shared anda was upgraded under a daemon that still runs the old one.
      upgradeNoticeShown = true
      inform(t('restartToUpgrade').replace('{version}', installed.version || ''))
    }
    if (state.connected && state.liveEvents) void browser?.reconnect().catch(() => {})
    clearTimeout(reconnectTimer)
    if (state.connected) reconnectDelay = firstReconnectDelay
    else if (!quitting && !daemon.manuallyStopped) {
      // Retries only reconnect: a daemon stopped from the CLI stays stopped
      // until the user asks for it again. Back off up to 5 minutes.
      reconnectTimer = setTimeout(() => void daemon.connect(false), reconnectDelay)
      reconnectDelay = Math.min(reconnectDelay * 2, 5 * 60_000)
    }
  })
  void persistUiLanguage(store.state.preferences.language).catch(() => {})
  nativeTheme.themeSource = store.state.preferences.theme
  daemon.on('state', (value) => emit({ type: 'state', value }))
  daemon.on('submissions', (value) => emit({ type: 'submissions', value }))
  const workbenchWorkspace = (path: string) => authorizeWorkspace(path, store.state.preferences)
  git = new GitService(join(app.getPath('userData'), 'workbench'), workbenchWorkspace)
  terminals = new TerminalService(
    workbenchWorkspace,
    (value) => emit({ type: 'terminal', value }),
    () => !updater?.installing
  )
  browser = new BrowserService(
    () => window,
    (value) => emit({ type: 'browser', value }),
    (name) => daemon.registerBrowserSession(name),
    home
  )
  updater = new DesktopUpdater(
    daemon,
    store,
    () => terminals.running,
    inform,
    refreshTray,
    (value) => emit({ type: 'update-status', value })
  )
  void updater.recover().catch((error) => emit({ type: 'update', value: String(error) }))
  if (!testMode) updater.startAutomaticChecks()
  handle('anda:browser', (request) => browser.request(request))
  daemon.on('browser-action', async (message) => {
    const command = message.params
    const result = await browser.execute(command).then(
      (value) => ({ ok: true, value }),
      (error) => ({
        ok: false,
        value: null,
        error: error instanceof Error ? error.message : 'Browser action failed'
      })
    )
    daemon.browserReply(message.id, command.session, result, message.connectionId)
  })
  handle('anda:terminal', (request) => terminals.request(request))
  handle('anda:git', async (request) => {
    if (request?.action === 'worktree-archive') {
      if (terminals.uses(request.path))
        throw new Error('Close this worktree’s terminals before archiving it.')
      if (daemon.view.connected) {
        const response = (await daemon.rpc('tool_call', [
          { name: 'anda_bot_api', args: { type: 'ListSessions' } }
        ])) as {
          output: {
            result: Array<{
              workspace: string
              conversation_id: number
              has_goal: boolean
              background_task_count: number
            }>
          }
        }
        if (!Array.isArray(response.output?.result))
          throw new Error('Cannot verify active work. Try again after stopping the daemon.')
        const target = await realpath(request.path)
        for (const session of response.output.result) {
          if ((await realpath(session.workspace).catch(() => '')) !== target) continue
          if (session.has_goal || session.background_task_count)
            throw new Error('Stop the active agent task in this worktree before archiving it.')
          const conversation = (await daemon.rpc('tool_call', [
            {
              name: 'conversations_api',
              args: { type: 'GetConversation', _id: session.conversation_id }
            }
          ])) as { output: { result?: { status?: string } } }
          if (
            !['idle', 'completed', 'cancelled', 'failed'].includes(
              conversation.output?.result?.status || ''
            )
          )
            throw new Error(
              'An agent is still using this worktree. Finish or stop it before archiving.'
            )
        }
      }
      const result = await dialog.showMessageBox(window!, {
        type: 'warning',
        message: 'Archive this worktree?',
        detail: `${request.path}\nAnda will save a Git snapshot before removing the checkout. Stop any agents or other applications using this folder first.`,
        buttons: ['Cancel', 'Archive'],
        defaultId: 0,
        cancelId: 0
      })
      if (result.response !== 1) throw new Error('Archive cancelled')
    }
    return git.request(request)
  })
  handle('anda:bootstrap', bootstrap)
  handle('anda:ready', () => {
    rendererReady = true
    // Replay the latest status: a check may finish between bootstrap and ready.
    if (updater.status) emit({ type: 'update-status', value: updater.status })
    if (pendingMenuAction) {
      emit({ type: 'menu', value: pendingMenuAction })
      pendingMenuAction = null
    }
  })
  handle('anda:connect', () => connectNow())
  handle('anda:control', async (action) => {
    if (!['stop', 'restart'].includes(action)) throw new Error('Invalid daemon action')
    return controlDaemon(action)
  })
  handle('anda:rpc', async (method, params, submissionId) => {
    try {
      if (
        method === 'agent_run' &&
        daemon.view.liveEvents &&
        params?.[0]?.meta?.source?.startsWith('desktop:')
      ) {
        params[0].meta.browser_session = await browser.registerSource(params[0].meta.source)
      }
      return await daemon.rpc(method, params, submissionId)
    } finally {
      if (method === 'agent_run') emit({ type: 'submissions', value: store.state.pending })
    }
  })
  handle('anda:chatgpt', (request: unknown) => daemon.chatgpt(request))
  handle('anda:config', (method, content, revision) => {
    if (
      !['GET', 'PUT'].includes(method) ||
      (method === 'PUT' && (typeof content !== 'string' || content.length > 2_000_000))
    )
      throw new Error('Invalid configuration request')
    if (revision !== undefined && (typeof revision !== 'string' || revision.length > 200))
      throw new Error('Invalid revision')
    return daemon.config(method, content, revision)
  })
  handle('anda:preferences', async (patch: Partial<Preferences>) => {
    validatePreferences(patch)
    store.state.preferences = { ...store.state.preferences, ...patch }
    await store.save()
    if (patch.theme) nativeTheme.themeSource = patch.theme
    if (typeof patch.launchAtLogin === 'boolean' && app.isPackaged)
      app.setLoginItemSettings({
        openAtLogin: patch.launchAtLogin,
        args: ['--hidden']
      })
    if (patch.language) {
      await persistUiLanguage(patch.language).catch(() => {})
      refreshTray()
    }
  })
  handle('anda:storage:get', (keys: string[]) => {
    if (
      !Array.isArray(keys) ||
      keys.length > 30 ||
      keys.some((k) => typeof k !== 'string' || k.length > 200)
    )
      throw new Error('Invalid storage keys')
    return Object.fromEntries(
      keys
        .filter((k) => Object.hasOwn(store.state.storage, k))
        .map((k) => [k, store.state.storage[k]])
    )
  })
  handle('anda:storage:set', async (items: Record<string, unknown>) => {
    if (
      !items ||
      typeof items !== 'object' ||
      Array.isArray(items) ||
      Buffer.byteLength(JSON.stringify(items)) > 2 * 1024 * 1024 ||
      Object.keys(items).some((k) => /token|secret|password|__proto__|constructor/i.test(k))
    )
      throw new Error('Invalid desktop storage')
    store.state.storage = { ...store.state.storage, ...items }
    await store.save()
  })
  handle('anda:workspace', async () => {
    const result = await dialog.showOpenDialog(window!, {
      properties: ['openDirectory', 'createDirectory'],
      title: 'Choose a workspace'
    })
    const path = result.canceled ? null : result.filePaths[0]
    // Sending or opening a chat registers its workspace again, so a stopped
    // daemon must not prevent adding the project.
    if (path && daemon.view.connected)
      await daemon.rpc('register_workspace', [path]).catch(() => {})
    return path || null
  })
  handle('anda:binary', async () => {
    const result = await dialog.showOpenDialog(window!, {
      properties: ['openFile'],
      title: 'Choose the anda executable'
    })
    if (!result.canceled && result.filePaths[0]) {
      store.state.binary = result.filePaths[0]
      await store.save()
      daemon.disconnect()
    }
    return connectNow()
  })
  handle('anda:notify', (source: string, title: string, body: string) => {
    if (
      typeof source !== 'string' ||
      typeof title !== 'string' ||
      typeof body !== 'string' ||
      !store.state.preferences.notifications ||
      !Notification.isSupported()
    )
      return
    if (window?.isFocused() && store.state.preferences.activeSource === source) return
    const key = `${source}:${title}:${body}`
    if ((notificationTimes.get(key) || 0) + 60_000 > Date.now()) return
    if (notificationTimes.size > 500) notificationTimes.clear()
    notificationTimes.set(key, Date.now())
    const notification = new Notification({
      title: title.slice(0, 120),
      body: body.slice(0, 240)
    })
    notification.on('click', () => {
      show()
      emit({ type: 'navigate', value: source })
    })
    notification.show()
  })
  handle('anda:submission:read', (id: string) => daemon.readSubmission(id))
  handle('anda:submission:acknowledge', (id: string) => daemon.acknowledgeSubmission(id))
  handle('anda:external', (url: unknown) => shell.openExternal(externalUrl(url)))
  handle('anda:print', async (html: string) => {
    if (typeof html !== 'string' || Buffer.byteLength(html) > 4 * 1024 * 1024)
      throw new Error('Print content is too large')
    const preview = new BrowserWindow({
      show: false,
      parent: window || undefined,
      webPreferences: {
        sandbox: true,
        contextIsolation: true,
        nodeIntegration: false,
        javascript: false,
        partition: 'anda-print'
      }
    })
    preview.webContents.setWindowOpenHandler(() => ({ action: 'deny' }))
    preview.webContents.on('will-navigate', (event) => event.preventDefault())
    const csp = `<meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src 'unsafe-inline'; img-src data: blob:">`
    try {
      await preview.loadURL(
        'data:text/html;charset=utf-8,' + encodeURIComponent(html.replace('<head>', '<head>' + csp))
      )
      await new Promise<void>((resolve) =>
        preview.webContents.print({ silent: false, printBackground: false }, () => resolve())
      )
    } finally {
      preview.destroy()
    }
  })
  handle('anda:logs', showLogs)
  handle('anda:update', () => runUpdate(() => updater.check()))
  handle('anda:extension-token', copyExtensionToken)
  const menu: Electron.MenuItemConstructorOptions[] = [
    ...(process.platform === 'darwin' ? [{ role: 'appMenu' as const }] : []),
    {
      label: 'File',
      submenu: [
        {
          id: 'anda-new-chat',
          label: 'New Chat',
          accelerator: 'CmdOrCtrl+N',
          click: () => menuAction('new-chat')
        },
        {
          id: 'anda-settings',
          label: 'Settings',
          accelerator: 'CmdOrCtrl+,',
          click: () => menuAction('settings')
        },
        { type: 'separator' },
        { role: 'close' },
        ...(process.platform !== 'darwin' ? [{ role: 'quit' as const }] : [])
      ]
    },
    { role: 'editMenu' },
    { role: 'viewMenu' },
    { role: 'windowMenu' },
    {
      label: 'Help',
      submenu: [
        {
          label: 'Anda Documentation',
          click: () => {
            void shell.openExternal('https://anda.bot')
          }
        },
        { label: 'Open Logs', click: () => void showLogs() }
      ]
    }
  ]
  Menu.setApplicationMenu(Menu.buildFromTemplate(menu))
  // A login start stays in the tray; the window is created when first shown.
  // macOS login items get no arguments, so ask the system how it started us.
  const loginStart =
    process.argv.includes('--hidden') ||
    (process.platform === 'darwin' && app.isPackaged && app.getLoginItemSettings().wasOpenedAtLogin)
  if (!loginStart) createWindow()
  const iconPath = app.isPackaged
    ? join(process.resourcesPath, 'logo-tray.png')
    : resolve(__dirname, '../../../anda_bot/assets/logo-tray.png')
  const image = nativeImage.createFromPath(iconPath).resize({ width: 18, height: 18 })
  image.setTemplateImage(process.platform === 'darwin')
  tray = new Tray(image)
  tray.setToolTip('Anda')
  refreshTray()
  tray.on('click', show)
  powerMonitor.on('resume', () => {
    if (!daemon.manuallyStopped) void connectNow(false)
  })
  // The tray owns the service at login, before any window asks for it.
  if (!daemon.manuallyStopped) void connectNow()
  if (process.platform !== 'darwin')
    nativeTheme.on('updated', () => window?.setTitleBarOverlay(titleBarOverlay()))
}

if (hasLock)
  app
    .whenReady()
    .then(setup)
    .catch((error) => {
      dialog.showErrorBox(
        'Anda could not start',
        error instanceof Error ? error.message : String(error)
      )
      app.quit()
    })
