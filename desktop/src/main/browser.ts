import {
  BrowserWindow,
  WebContentsView,
  session,
  dialog,
  clipboard,
  shell,
  type DownloadItem,
  type WebContents
} from 'electron'
import { createHash } from 'node:crypto'
import { basename } from 'node:path'
import { realpath, stat } from 'node:fs/promises'
import type { BrowserRequest, BrowserState, BrowserDownload } from '../shared/browser'
import type {
  BrowserActionArgs,
  BrowserCommand
} from '../../../chrome-extension/src/lib/service-worker/types'
import {
  pageActionDispatcher,
  resolveInputTarget
} from '../../../chrome-extension/src/lib/service-worker/page-scripts'

interface Tab {
  view: WebContentsView
  error?: string
  ready?: Promise<void>
}
interface Context {
  source: string
  session: string
  active: number | null
  tabs: Map<number, Tab>
  downloads: Map<number, { view: BrowserDownload; item: DownloadItem }>
  bounds?: Electron.Rectangle
  visible: boolean
}
export function browserUrl(raw: unknown): string {
  if (typeof raw !== 'string' || raw.length > 8192) throw new Error('Invalid browser URL')
  if (raw === 'about:blank') return raw
  const url = new URL(raw)
  if (!['https:', 'http:'].includes(url.protocol) || url.username || url.password)
    throw new Error('Only HTTP and HTTPS pages can be opened in this browser.')
  return url.href
}
export class BrowserService {
  private contexts = new Map<string, Context>()
  private jobs = new Map<number, Promise<unknown>>()
  private downloadId = 0
  private attached = new Set<WebContentsView>()
  private browsing = session.fromPartition('persist:anda-browser')
  private grants = new Set<string>()
  constructor(
    private window: () => BrowserWindow | null,
    private emit: (state: BrowserState) => void,
    private register: (name: string) => Promise<void>,
    private profile: string
  ) {
    this.browsing.setPermissionCheckHandler(
      (wc, permission, origin, details) =>
        Boolean(wc && this.findContents(wc.id)) &&
        (permission !== 'media' || details.mediaType === 'audio') &&
        this.grants.has(`${origin}:${permission}`)
    )
    this.browsing.setPermissionRequestHandler((wc, permission, callback, details) => {
      if (
        !this.findContents(wc.id) ||
        !['media', 'geolocation', 'notifications', 'fullscreen'].includes(permission)
      )
        return callback(false)
      if (
        permission === 'media' &&
        (!('mediaTypes' in details) ||
          !details.mediaTypes?.length ||
          details.mediaTypes.some((kind) => kind !== 'audio'))
      )
        return callback(false)
      const origin = new URL(details.requestingUrl).origin
      if (origin === 'null') return callback(false)
      void dialog
        .showMessageBox({
          type: 'question',
          message: `${origin} requests ${permission === 'media' ? 'microphone access' : permission}`,
          buttons: ['Deny', 'Allow'],
          defaultId: 0,
          cancelId: 0
        })
        .then(({ response }) => {
          if (response === 1) this.grants.add(`${origin}:${permission}`)
          callback(response === 1)
        })
        .catch(() => callback(false))
    })
    this.browsing.on('will-download', (_event, item, contents) => {
      const context = contents && this.findContents(contents.id)
      if (!context) {
        item.cancel()
        return
      }
      const id = ++this.downloadId
      const view: BrowserDownload = {
        id,
        name: basename(item.getFilename()),
        received: 0,
        total: item.getTotalBytes(),
        state: 'progressing'
      }
      context.downloads.set(id, { view, item })
      item.setSaveDialogOptions({ title: 'Save browser download', defaultPath: view.name })
      item.on('updated', (_event, state) => {
        view.received = item.getReceivedBytes()
        view.state = state
        this.changed(context)
      })
      item.once('done', (_event, state) => {
        view.state = state
        this.changed(context)
      })
      this.changed(context)
    })
  }
  private context(source: string): Context {
    if (
      typeof source !== 'string' ||
      !source ||
      source.length > 2048 ||
      source.includes(':reply_target:')
    )
      throw new Error('Browser requires a local chat')
    let context = this.contexts.get(source)
    if (!context) {
      context = {
        source,
        session: `browser:desktop:${createHash('sha256').update(this.profile).update(source).digest('hex').slice(0, 32)}`,
        tabs: new Map(),
        downloads: new Map(),
        active: null,
        visible: false
      }
      this.contexts.set(source, context)
    }
    return context
  }
  async registerSource(source: string): Promise<string> {
    const context = this.context(source)
    await this.register(context.session)
    return context.session
  }
  async reconnect(): Promise<void> {
    for (const context of this.contexts.values()) await this.register(context.session)
  }
  private findContents(id: number): Context | undefined {
    return [...this.contexts.values()].find((c) => c.tabs.has(id))
  }
  private state(context: Context): BrowserState {
    return {
      source: context.source,
      session: context.session,
      active: context.active,
      tabs: [...context.tabs.entries()].map(([id, tab]) => ({
        id,
        title: tab.view.webContents.getTitle() || 'New tab',
        url: tab.view.webContents.getURL(),
        loading: tab.view.webContents.isLoading(),
        canBack: tab.view.webContents.navigationHistory.canGoBack(),
        canForward: tab.view.webContents.navigationHistory.canGoForward(),
        error: tab.error
      })),
      downloads: [...context.downloads.values()].map((d) => ({ ...d.view }))
    }
  }
  private changed(context: Context): void {
    this.emit(this.state(context))
  }
  private layout(): void {
    const window = this.window()
    if (!window || window.isDestroyed()) return
    for (const context of this.contexts.values())
      for (const [id, tab] of context.tabs) {
        const visible =
          context.visible && context.active === id && context.bounds && !window.isMinimized()
        if (visible) {
          if (!this.attached.has(tab.view)) {
            window.contentView.addChildView(tab.view)
            this.attached.add(tab.view)
          }
          tab.view.setBounds(context.bounds!)
          tab.view.setVisible(true)
        } else tab.view.setVisible(false)
      }
  }
  private newTab(context: Context, url = 'about:blank', active = true): Tab {
    browserUrl(url)
    if ([...this.contexts.values()].reduce((n, c) => n + c.tabs.size, 0) >= 32)
      throw new Error('Close a browser tab before opening another.')
    const view = new WebContentsView({
      webPreferences: {
        session: this.browsing,
        sandbox: true,
        contextIsolation: true,
        nodeIntegration: false,
        webSecurity: true,
        backgroundThrottling: true
      }
    })
    const tab: Tab = { view }
    const wc = view.webContents
    const id = wc.id
    // Agent-owned tabs must have a real viewport even before the user opens
    // the browser panel. Keep the native view attached but hidden.
    view.setBounds(context.bounds || { x: 0, y: 0, width: 1280, height: 800 })
    view.setVisible(false)
    const parent = this.window()
    if (parent && !parent.isDestroyed()) {
      parent.contentView.addChildView(view)
      this.attached.add(view)
    }
    context.tabs.set(wc.id, tab)
    if (active || context.active === null) context.active = wc.id
    wc.setWindowOpenHandler(({ url }) => {
      try {
        this.newTab(context, browserUrl(url))
      } catch {}
      return { action: 'deny' }
    })
    const protect = (event: Electron.Event, url: string) => {
      try {
        browserUrl(url)
      } catch {
        event.preventDefault()
      }
    }
    wc.on('will-navigate', protect)
    wc.on('will-redirect', protect)
    wc.on('did-start-loading', () => this.changed(context))
    wc.on('did-stop-loading', () => this.changed(context))
    wc.on('page-title-updated', () => this.changed(context))
    wc.on('did-navigate', () => this.changed(context))
    wc.on('did-navigate-in-page', () => this.changed(context))
    wc.on('did-fail-load', (_event, code, description, _url, main) => {
      if (main && code !== -3) {
        tab.error = description
        this.changed(context)
      }
    })
    wc.on('render-process-gone', () => {
      tab.error = 'Page process stopped. Reload the tab.'
      this.changed(context)
    })
    wc.on('destroyed', () => {
      context.tabs.delete(id)
      const parent = this.window()
      if (this.attached.has(view) && parent && !parent.isDestroyed())
        parent.contentView.removeChildView(view)
      this.attached.delete(view)
      if (context.active === id) context.active = context.tabs.keys().next().value ?? null
      this.layout()
      this.changed(context)
    })
    tab.ready = wc.loadURL(url).catch((e) => {
      if (e.code !== 'ERR_ABORTED' && e.errno !== -3) {
        tab.error = e.message
        this.changed(context)
      }
    })
    this.layout()
    this.changed(context)
    return tab
  }
  private tab(context: Context, id?: number): Tab {
    const tab = context.tabs.get(id ?? context.active ?? -1)
    if (!tab) throw new Error('No browser tab selected. Open a tab first.')
    return tab
  }
  async request(request: BrowserRequest): Promise<BrowserState> {
    const context = this.context(request.source)
    switch (request.action) {
      case 'state':
        break
      case 'new':
        this.newTab(context)
        break
      case 'bounds': {
        const window = this.window()
        const bounds = window?.getContentBounds()
        if (
          !bounds ||
          ![request.x, request.y, request.width, request.height].every(Number.isFinite)
        )
          throw new Error('Invalid browser bounds')
        const zoom = window!.webContents.getZoomFactor()
        const x = Math.max(0, Math.round(request.x * zoom)),
          y = Math.max(0, Math.round(request.y * zoom))
        context.bounds = {
          x,
          y,
          width: Math.max(0, Math.min(Math.round(request.width * zoom), bounds.width - x)),
          height: Math.max(0, Math.min(Math.round(request.height * zoom), bounds.height - y))
        }
        if (request.visible) for (const other of this.contexts.values()) other.visible = false
        context.visible = request.visible && context.bounds.width > 0 && context.bounds.height > 0
        this.layout()
        return this.state(context)
      }
      case 'select':
        this.tab(context, request.id)
        context.active = request.id
        this.layout()
        break
      case 'close':
        this.tab(context, request.id).view.webContents.close()
        break
      case 'navigate': {
        const tab = this.tab(context, request.id)
        await tab.ready
        tab.error = undefined
        await tab.view.webContents.loadURL(browserUrl(request.url)).catch((error) => {
          if (error.code !== 'ERR_ABORTED' && error.errno !== -3) throw error
        })
        break
      }
      case 'back':
        this.tab(context, request.id).view.webContents.navigationHistory.goBack()
        break
      case 'forward':
        this.tab(context, request.id).view.webContents.navigationHistory.goForward()
        break
      case 'reload': {
        const tab = this.tab(context, request.id)
        tab.error = undefined
        tab.view.webContents.reload()
        break
      }
      case 'find':
        if (request.text) this.tab(context, request.id).view.webContents.findInPage(request.text)
        else this.tab(context, request.id).view.webContents.stopFindInPage('clearSelection')
        break
      case 'download-cancel':
        context.downloads.get(request.id)?.item.cancel()
        break
      case 'download-open': {
        const d = context.downloads.get(request.id)
        if (!d || d.view.state !== 'completed') throw new Error('Download is not complete')
        shell.showItemInFolder(d.item.getSavePath())
        break
      }
      default:
        throw new Error('Unsupported browser operation')
    }
    this.changed(context)
    return this.state(context)
  }
  private async cdp(
    wc: WebContents,
    method: string,
    params?: Record<string, unknown>
  ): Promise<any> {
    if (!wc.debugger.isAttached()) wc.debugger.attach('1.3')
    return wc.debugger.sendCommand(method, params)
  }
  private waitLoaded(wc: WebContents, timeout = 30000): Promise<void> {
    if (!wc.isLoading()) return Promise.resolve()
    return new Promise((resolve, reject) => {
      const done = () => {
        cleanup()
        resolve()
      }
      const closed = () => {
        cleanup()
        reject(new Error('Browser tab closed during navigation'))
      }
      const timer = setTimeout(
        () => {
          cleanup()
          wc.stop()
          reject(new Error('Page load timed out'))
        },
        Math.min(120000, Math.max(1000, timeout))
      )
      const cleanup = () => {
        clearTimeout(timer)
        wc.off('did-stop-loading', done)
        wc.off('destroyed', closed)
      }
      wc.once('did-stop-loading', done)
      wc.once('destroyed', closed)
    })
  }
  private async evaluate(wc: WebContents, code: string, args: BrowserActionArgs): Promise<any> {
    let contextId: number | undefined
    if (args.frame_id && args.frame_id !== 0) {
      const frame = wc.mainFrame.framesInSubtree.find((f) => f.routingId === args.frame_id)
      if (!frame) throw new Error('Frame no longer exists')
      return frame.executeJavaScript(code, true)
    }
    if (args.world?.trim().toUpperCase() !== 'MAIN') {
      const tree = await this.cdp(wc, 'Page.getFrameTree')
      contextId = (
        await this.cdp(wc, 'Page.createIsolatedWorld', {
          frameId: tree.frameTree.frame.id,
          worldName: 'Anda page tools'
        })
      ).executionContextId
    }
    const result = await this.cdp(wc, 'Runtime.evaluate', {
      expression: code,
      contextId,
      awaitPromise: true,
      returnByValue: true,
      userGesture: true
    })
    if (result.exceptionDetails)
      throw new Error(
        result.exceptionDetails.exception?.description || result.exceptionDetails.text
      )
    return result.result?.value
  }
  async execute(command: BrowserCommand): Promise<unknown> {
    const context = [...this.contexts.values()].find((c) => c.session === command.session)
    if (!context) throw new Error('Browser session is not owned by this desktop')
    const args = command.args || {}
    const action = args.action
    if (action === 'list_tabs')
      return {
        tabs: this.state(context).tabs.map((t) => ({ ...t, active: t.id === context.active }))
      }
    if (action === 'launch_browser') {
      if (args.url || !context.tabs.size)
        this.newTab(context, browserUrl(args.url || 'about:blank'))
      return { launched: false, connected: true, session: context.session }
    }
    if (action === 'open_tab' || (action === 'navigate' && !context.active)) {
      const tab = this.newTab(context, browserUrl(args.url || 'about:blank'), args.active !== false)
      await this.waitLoaded(tab.view.webContents, args.timeout_ms)
      if (tab.error) throw new Error(tab.error)
      return { opened: true, tab: this.summary(tab.view.webContents) }
    }
    if (action === 'list_downloads') return { downloads: this.state(context).downloads }
    if (action === 'cancel_download' || action === 'open_download') {
      await this.request({
        action: action === 'cancel_download' ? 'download-cancel' : 'download-open',
        source: context.source,
        id: args.download_id!
      })
      return { ok: true }
    }
    if (action === 'clear_browser_cache') {
      await this.browsing.clearCache()
      return { cleared: true }
    }
    const tab = this.tab(context, args.tab_id)
    const wc = tab.view.webContents
    if (action === 'handle_dialog') return this.pageAction(context, wc, args)
    if (action === 'get_current_tab') return { tab: this.summary(wc) }
    if (action === 'close_tab' || action === 'switch_tab') {
      await this.request({
        action: action === 'close_tab' ? 'close' : 'select',
        source: context.source,
        id: wc.id
      })
      return { ok: true }
    }
    const previous = this.jobs.get(wc.id) || Promise.resolve()
    const job = previous
      .catch(() => {})
      .then(async () => {
        let timer: NodeJS.Timeout | undefined
        try {
          const result = await Promise.race([
            this.pageAction(context, wc, args),
            new Promise<never>((_resolve, reject) => {
              timer = setTimeout(
                () => {
                  void this.cdp(wc, 'Runtime.terminateExecution').catch(() => {})
                  reject(
                    new Error(
                      'Browser action timed out; its outcome may be unknown. Inspect the page before retrying.'
                    )
                  )
                },
                Math.min(120000, Math.max(1000, args.timeout_ms || 30000))
              )
            })
          ])
          return result
        } finally {
          clearTimeout(timer)
        }
      })
    this.jobs.set(wc.id, job)
    try {
      return await job
    } finally {
      if (this.jobs.get(wc.id) === job) this.jobs.delete(wc.id)
    }
  }
  private summary(wc: WebContents) {
    return { id: wc.id, url: wc.getURL(), title: wc.getTitle(), window_id: this.window()?.id }
  }
  private async pageAction(
    context: Context,
    wc: WebContents,
    args: BrowserActionArgs
  ): Promise<unknown> {
    if (args.action !== 'handle_dialog') await this.waitLoaded(wc, args.timeout_ms)
    const tab = this.summary(wc)
    switch (args.action) {
      case 'navigate':
        await wc.loadURL(browserUrl(args.url))
        return { navigated: true, tab: this.summary(wc) }
      case 'reload':
        args.bypass_cache ? wc.reloadIgnoringCache() : wc.reload()
        return { reloaded: true, tab }
      case 'go_back':
        wc.navigationHistory.goBack()
        return { went_back: true, tab }
      case 'go_forward':
        wc.navigationHistory.goForward()
        return { went_forward: true, tab }
      case 'download':
        wc.downloadURL(browserUrl(args.url))
        return { started: true }
      case 'get_frames':
        return {
          frames: wc.mainFrame.framesInSubtree.map((f) => ({
            frame_id: f.routingId,
            url: f.url,
            name: f.name,
            parent_frame_id: f.parent?.routingId
          }))
        }
      case 'get_accessibility_tree':
        return { tab, nodes: (await this.cdp(wc, 'Accessibility.getFullAXTree')).nodes }
      case 'handle_dialog':
        await this.cdp(wc, 'Page.handleJavaScriptDialog', {
          accept: args.accept ?? false,
          promptText: args.prompt_text
        })
        return { handled: true }
      case 'screenshot': {
        const overridden = Boolean(
          args.viewport_width || args.viewport_height || args.device_scale_factor
        )
        if (overridden) {
          const current = await this.evaluate(wc, '({width:innerWidth,height:innerHeight})', {})
          const width = args.viewport_width || current.width,
            height = args.viewport_height || current.height,
            scale = args.device_scale_factor || 1
          if (
            ![width, height].every(Number.isInteger) ||
            width < 1 ||
            height < 1 ||
            width * height * scale * scale > 32_000_000 ||
            scale < 0.1 ||
            scale > 5
          )
            throw new Error('Screenshot viewport exceeds the supported size')
          await this.cdp(wc, 'Emulation.setDeviceMetricsOverride', {
            width,
            height,
            deviceScaleFactor: scale,
            mobile: false
          })
        }
        try {
          let clip:
            | { x: number; y: number; width: number; height: number; scale: number }
            | undefined
          if (args.full_page) {
            const metrics = await this.cdp(wc, 'Page.getLayoutMetrics')
            const size = metrics.cssContentSize || metrics.contentSize
            if (size.width * size.height > 32_000_000 || size.height > 16384)
              throw new Error(
                'Page is too large for a full-page screenshot; capture the viewport or a selector.'
              )
            clip = { x: 0, y: 0, width: size.width, height: size.height, scale: 1 }
          }
          if (args.selector) {
            const box = await this.evaluate(
              wc,
              `(() => { const e = document.querySelector(${JSON.stringify(args.selector)}); if (!e) throw new Error('Element not found'); const r = e.getBoundingClientRect(); return {x:r.x+scrollX,y:r.y+scrollY,width:r.width,height:r.height,scale:1} })()`,
              args
            )
            if (!box.width || !box.height || box.width * box.height > 32_000_000)
              throw new Error('Element screenshot is empty or too large')
            clip = box
          }
          const shot = await this.cdp(wc, 'Page.captureScreenshot', {
            format: 'png',
            captureBeyondViewport: Boolean(clip),
            clip
          })
          return { tab, mime_type: 'image/png', data_url: `data:image/png;base64,${shot.data}` }
        } finally {
          if (overridden) await this.cdp(wc, 'Emulation.clearDeviceMetricsOverride').catch(() => {})
        }
      }
      case 'print_to_pdf': {
        const pdf = await wc.printToPDF({ printBackground: true })
        return {
          tab,
          mime_type: 'application/pdf',
          data_url: `data:application/pdf;base64,${pdf.toString('base64')}`
        }
      }
      case 'get_cookies':
        return {
          cookies: await this.browsing.cookies.get({
            url: browserUrl(args.url || wc.getURL()),
            ...(args.name ? { name: args.name } : {})
          })
        }
      case 'set_cookie':
        await this.browsing.cookies.set({
          url: browserUrl(args.url || wc.getURL()),
          name: args.name!,
          value: args.value || '',
          path: args.path,
          domain: args.domain,
          secure: args.secure,
          httpOnly: args.http_only,
          sameSite: args.same_site,
          expirationDate: args.expiration_date
        })
        return { set: true }
      case 'delete_cookie':
        await this.browsing.cookies.remove(browserUrl(args.url || wc.getURL()), args.name!)
        return { deleted: true }
      case 'copy_to_clipboard':
        clipboard.writeText(args.text || '')
        return { copied: true }
      case 'upload_file': {
        // The browser tool may select files only after the local user reviews
        // them; a website or prompt cannot silently upload arbitrary paths.
        const paths = await Promise.all((args.files || []).map((p) => realpath(p)))
        if (
          !paths.length ||
          paths.length > 20 ||
          (await Promise.all(paths.map((p) => stat(p)))).some((s) => !s.isFile())
        )
          throw new Error('Select up to 20 regular files')
        const choice = await dialog.showMessageBox({
          type: 'question',
          message: `Upload files to ${new URL(wc.getURL()).origin}?`,
          detail: paths.join('\n'),
          buttons: ['Cancel', 'Upload'],
          defaultId: 0,
          cancelId: 0
        })
        if (choice.response !== 1) throw new Error('Upload cancelled')
        const root = await this.cdp(wc, 'DOM.getDocument', { depth: -1 })
        const node = await this.cdp(wc, 'DOM.querySelector', {
          nodeId: root.root.nodeId,
          selector: args.selector || 'input[type=file]'
        })
        await this.cdp(wc, 'DOM.setFileInputFiles', { nodeId: node.nodeId, files: paths })
        return { uploaded: true, count: paths.length }
      }
      case 'click':
      case 'hover':
      case 'type_text': {
        const point = await this.evaluate(
          wc,
          `(${resolveInputTarget.toString()})(${JSON.stringify(args)})`,
          args
        )
        if (Number.isFinite(point.x) && Number.isFinite(point.y)) {
          await this.cdp(wc, 'Input.dispatchMouseEvent', {
            type: 'mouseMoved',
            x: point.x,
            y: point.y
          })
          if (args.action !== 'hover') {
            await this.cdp(wc, 'Input.dispatchMouseEvent', {
              type: 'mousePressed',
              x: point.x,
              y: point.y,
              button: 'left',
              clickCount: 1
            })
            await this.cdp(wc, 'Input.dispatchMouseEvent', {
              type: 'mouseReleased',
              x: point.x,
              y: point.y,
              button: 'left',
              clickCount: 1
            })
          }
          if (args.action === 'type_text' && point.native_text_input) {
            if (args.value !== 'append') {
              await this.cdp(wc, 'Input.dispatchKeyEvent', {
                type: 'keyDown',
                key: 'a',
                code: 'KeyA',
                modifiers: process.platform === 'darwin' ? 4 : 2,
                commands: ['selectAll']
              })
              await this.cdp(wc, 'Input.dispatchKeyEvent', {
                type: 'keyUp',
                key: 'a',
                code: 'KeyA'
              })
            }
            await this.cdp(wc, 'Input.insertText', { text: args.text || '' })
          } else if (args.action === 'type_text')
            return this.evaluate(
              wc,
              `(${pageActionDispatcher.toString()})(${JSON.stringify(args)})`,
              args
            )
          return { performed: true, tab: this.summary(wc) }
        }
        break
      }
      case 'press_key': {
        const key = args.key || 'Enter'
        const names: Record<string, number> = {
          Enter: 13,
          Tab: 9,
          Escape: 27,
          Backspace: 8,
          Delete: 46,
          ArrowLeft: 37,
          ArrowUp: 38,
          ArrowRight: 39,
          ArrowDown: 40
        }
        await this.cdp(wc, 'Input.dispatchKeyEvent', {
          type: 'keyDown',
          key,
          windowsVirtualKeyCode: names[key],
          ...(key === 'Enter' ? { text: '\r' } : {})
        })
        await this.cdp(wc, 'Input.dispatchKeyEvent', {
          type: 'keyUp',
          key,
          windowsVirtualKeyCode: names[key]
        })
        return { pressed: key }
      }
      case 'execute_javascript': {
        if (!args.code || args.code.length > 100000) throw new Error('Invalid page script')
        const world = args.world?.trim().toUpperCase()
        const options = {
          ...args,
          world:
            world === 'MAIN' || world === 'DEBUGGER' || (!world && args.use_bridge !== false)
              ? 'MAIN'
              : 'ISOLATED'
        }
        const result = await this.evaluate(
          wc,
          `(${pageActionDispatcher.toString()})(${JSON.stringify(args)})`,
          options
        )
        return { tab, ...result }
      }
    }
    return this.evaluate(wc, `(${pageActionDispatcher.toString()})(${JSON.stringify(args)})`, args)
  }
  hide(): void {
    for (const context of this.contexts.values()) context.visible = false
    this.layout()
  }
  destroy(): void {
    for (const context of this.contexts.values())
      for (const tab of [...context.tabs.values()])
        tab.view.webContents.close({ waitForBeforeUnload: false })
    this.contexts.clear()
    this.attached.clear()
  }
}
