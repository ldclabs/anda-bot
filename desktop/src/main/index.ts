import {
  app,
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
  net
} from 'electron'
import { join, resolve } from 'node:path'
import { homedir } from 'node:os'
import { pathToFileURL } from 'node:url'
import { createHash } from 'node:crypto'
import { realpath, stat } from 'node:fs/promises'
import { GitService } from './git'
import { TerminalService } from './terminal'
import { BrowserService } from './browser'
import { DesktopUpdater } from './updater'
import type { Bootstrap, NativeEvent, Preferences } from '../shared/contract'
import { DesktopStore } from './store'
import { DaemonClient } from './daemon-client'
import { appPermissionAllowed, externalUrl, navigationSource, rendererAssetPath } from './policy'

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
let pendingNavigation: string | null = null
let reconnectTimer: NodeJS.Timeout | undefined
const notificationTimes = new Map<string, number>()
const rendererPath = join(__dirname, '../renderer/index.html')
const applicationUrl = 'anda-app://app/index.html'
const rendererUrl = process.env.ELECTRON_RENDERER_URL

function emit(event: NativeEvent): void {
  window?.webContents.send('anda:event', event)
}
function show(): void {
  if (!stateLoaded) return
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

function createWindow(): void {
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
      : {
          titleBarOverlay: {
            color: '#f4f4f3',
            symbolColor: '#252525',
            height: 42
          }
        }),
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
    try {
      void shell.openExternal(externalUrl(url)).catch(() => {})
    } catch {
      /* Non-web schemes never launch. */
    }
    return { action: 'deny' }
  })
  contents.on('will-navigate', (event, url) => {
    if (url === contents.getURL()) return
    event.preventDefault()
    try {
      void shell.openExternal(externalUrl(url)).catch(() => {})
    } catch {
      /* Block local/arbitrary navigation. */
    }
  })
  contents.session.setPermissionRequestHandler((webContents, permission, callback, details) => {
    if (webContents.id === contents.id && details.isMainFrame && permission === 'speaker-selection')
      return callback(true)
    if (
      webContents.id !== contents.id ||
      permission !== 'media' ||
      !details.isMainFrame ||
      !('mediaTypes' in details) ||
      !details.mediaTypes ||
      details.mediaTypes.some((type) => type !== 'audio')
    )
      return callback(false)
    if (testMode) callback(true)
    else if (process.platform === 'darwin')
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
  window.on('close', (event) => {
    if (!quitting) {
      event.preventDefault()
      window?.hide()
    }
  })
  window.on('closed', () => {
    window = null
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
  contents.on('render-process-gone', () => {
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
  const state = daemon.manuallyStopped ? daemon.view : await daemon.connect()
  return {
    daemon: state,
    preferences: store.state.preferences,
    platform: process.platform,
    version: app.getVersion(),
    pending: store.state.pending
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
    testMode ? process.env.ANDA_DESKTOP_TEST_URL : undefined
  )
  daemon.on('change', (state) => {
    emit({ type: 'connection', value: state })
    if (state.connected && state.liveEvents) void browser?.reconnect().catch(() => {})
    clearTimeout(reconnectTimer)
    if (!state.connected && !quitting && !daemon.manuallyStopped)
      reconnectTimer = setTimeout(() => {
        void daemon.connect()
      }, 10_000)
  })
  nativeTheme.themeSource = store.state.preferences.theme
  daemon.on('state', (value) => emit({ type: 'state', value }))
  daemon.on('submissions', (value) => emit({ type: 'submissions', value }))
  const authorizeWorkspace = async (path: string): Promise<string> => {
    if (typeof path !== 'string' || !path || path.length > 8192)
      throw new Error('Choose a workspace first.')
    const allowed = [
      ...store.state.preferences.projects.map((p) => p.path),
      ...store.state.preferences.chats.flatMap((c) => (c.workspace ? [c.workspace] : []))
    ]
    const resolved = await realpath(path)
    const matches = await Promise.all(allowed.map((p) => realpath(p).catch(() => '')))
    if (!matches.includes(resolved) || !(await stat(resolved)).isDirectory())
      throw new Error('Select this folder as a project before using the workbench.')
    return resolved
  }
  git = new GitService(join(app.getPath('userData'), 'workbench'), authorizeWorkspace)
  terminals = new TerminalService(
    authorizeWorkspace,
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
    (value) => emit({ type: 'update', value })
  )
  void updater.recover().catch((error) => emit({ type: 'update', value: String(error) }))
  handle('anda:browser', (request) => browser.request(request))
  daemon.on('browser-action', async (message) => {
    const command = message.params
    try {
      daemon.browserReply(
        message.id,
        command.session,
        { ok: true, value: await browser.execute(command) },
        message.connectionId
      )
    } catch (error) {
      daemon.browserReply(
        message.id,
        command.session,
        {
          ok: false,
          value: null,
          error: error instanceof Error ? error.message : 'Browser action failed'
        },
        message.connectionId
      )
    }
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
  handle('anda:connect', () => daemon.connect())
  handle('anda:control', async (action) => {
    if (!['stop', 'restart'].includes(action)) throw new Error('Invalid daemon action')
    const result = await dialog.showMessageBox(window!, {
      type: 'warning',
      message: action === 'stop' ? 'Stop the Anda daemon?' : 'Restart the Anda daemon?',
      detail:
        'Running agent tasks, IM channels, and scheduled jobs will be interrupted. Closing the desktop window alone keeps them running.',
      buttons: ['Cancel', action === 'stop' ? 'Stop daemon' : 'Restart daemon'],
      defaultId: 0,
      cancelId: 0
    })
    if (result.response !== 1) return daemon.view
    clearTimeout(reconnectTimer)
    return daemon.control(action)
  })
  handle('anda:rpc', async (method, params) => {
    try {
      if (
        method === 'agent_run' &&
        daemon.view.liveEvents &&
        params?.[0]?.meta?.source?.startsWith('desktop:')
      ) {
        params[0].meta.browser_session = await browser.registerSource(params[0].meta.source)
      }
      return await daemon.rpc(method, params)
    } finally {
      if (method === 'agent_run') emit({ type: 'submissions', value: store.state.pending })
    }
  })
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
    return store.state.preferences
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
    if (path) await daemon.rpc('register_workspace', [path])
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
    return daemon.connect()
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
  handle('anda:submission:acknowledge', async (id: string) => {
    if (typeof id !== 'string') throw new Error('Invalid submission')
    store.state.pending = store.state.pending.filter((p) => p.id !== id)
    await store.save()
    emit({ type: 'submissions', value: store.state.pending })
  })
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
  handle('anda:logs', () => shell.openPath(join(home, 'logs')))
  handle('anda:update', () => updater.check())
  const menu: Electron.MenuItemConstructorOptions[] = [
    ...(process.platform === 'darwin' ? [{ role: 'appMenu' as const }] : []),
    {
      label: 'File',
      submenu: [
        {
          label: 'New Chat',
          accelerator: 'CmdOrCtrl+N',
          click: () => {
            show()
            emit({ type: 'menu', value: 'new-chat' })
          }
        },
        {
          label: 'Settings',
          accelerator: 'CmdOrCtrl+,',
          click: () => {
            show()
            emit({ type: 'menu', value: 'settings' })
          }
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
        {
          label: 'Open Logs',
          click: () => {
            void shell.openPath(join(home, 'logs'))
          }
        }
      ]
    }
  ]
  Menu.setApplicationMenu(Menu.buildFromTemplate(menu))
  createWindow()
  const iconPath = app.isPackaged
    ? join(process.resourcesPath, 'logo-tray.png')
    : resolve(__dirname, '../../../anda_bot/assets/logo-tray.png')
  const image = nativeImage.createFromPath(iconPath).resize({ width: 18, height: 18 })
  image.setTemplateImage(process.platform === 'darwin')
  tray = new Tray(image)
  tray.setToolTip('Anda')
  tray.setContextMenu(
    Menu.buildFromTemplate([
      { label: 'Open Anda', click: show },
      {
        label: 'New Chat',
        click: () => {
          show()
          emit({ type: 'menu', value: 'new-chat' })
        }
      },
      { type: 'separator' },
      {
        label: 'Quit Anda Desktop (keep daemon running)',
        click: () => app.quit()
      }
    ])
  )
  tray.on('click', show)
  powerMonitor.on('resume', () => {
    if (!daemon.manuallyStopped) void daemon.connect()
  })
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
