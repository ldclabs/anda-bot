import { EventEmitter } from 'node:events'
import { createReadStream } from 'node:fs'
import { access, readFile, writeFile, mkdir, rename, rm } from 'node:fs/promises'
import { execFile } from 'node:child_process'
import { promisify } from 'node:util'
import { join } from 'node:path'
import { homedir } from 'node:os'
import { randomUUID, createHash } from 'node:crypto'
import { parse as parseYaml } from 'yaml'
import WebSocket from 'ws'
import type { AppInitialize, AppSubmit, SubmissionReceipt } from '../shared/app-protocol'
import type { DaemonView, PendingSubmission } from '../shared/contract'
import { loopbackBaseUrl, validateRpc } from './policy'
import { DesktopStore } from './store'

const run = promisify(execFile)
const executable = process.platform === 'win32' ? 'anda.exe' : 'anda'
/** Printed by `anda install --json`. */
export interface InstallReport {
  action: 'installed' | 'upgraded' | 'current' | 'kept_newer' | 'homebrew'
  path: string
  version: string | null
  skills_installed: boolean
  path_updated: boolean
  launcher_retired: boolean
  launcher_started_at_login: boolean
}
/** Printed by `anda update --check[-if-due] --json`. */
export interface RuntimeUpdateState {
  status: string
  current_tag: string
  latest_tag?: string | null
  downloaded_path?: string | null
  error?: string | null
}
/** The release tag a downloaded runtime update would install, if any. */
export function downloadedRelease(state?: RuntimeUpdateState): string | null {
  return state?.status === 'downloaded' &&
    state.latest_tag &&
    state.latest_tag !== state.current_tag &&
    state.downloaded_path
    ? state.latest_tag
    : null
}
/** Installs other tools and a development build may use, in lookup order. */
function installedCandidates(): string[] {
  if (process.platform === 'win32')
    return [
      join(
        process.env.LOCALAPPDATA || join(homedir(), 'AppData', 'Local'),
        'Programs',
        'AndaBot',
        executable
      ),
      join(homedir(), 'bin', executable)
    ]
  return [
    join(homedir(), '.local', 'bin', executable),
    '/opt/homebrew/bin/anda',
    '/usr/local/bin/anda'
  ]
}
async function exists(path: string): Promise<boolean> {
  try {
    await access(path)
    return true
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code === 'ENOENT') return false
    throw error
  }
}
type Waiting = {
  resolve(value: unknown): void
  reject(error: Error): void
  timer: NodeJS.Timeout
}

export class DaemonClient extends EventEmitter {
  view: DaemonView
  manuallyStopped = false
  private bearer = ''
  private socket: WebSocket | null = null
  private waiting = new Map<number, Waiting>()
  private sequence = 0
  private connectionId = 0
  private connecting?: Promise<DaemonView>
  private heartbeat?: NodeJS.Timeout
  private appTransport = false
  private credentialAt = 0
  private configWrites: Promise<unknown> = Promise.resolve()
  private browserSessions = new Set<string>()
  private resolving?: Promise<string>
  /** The outcome of the last `anda install` from the bundle. */
  installReport?: InstallReport
  /**
   * `installRuntime` is set for packaged apps: their bundle carries an `anda`
   * that is only ever installed to the shared CLI location, never run as the
   * daemon, so the desktop, terminals and the extension share one runtime.
   */
  constructor(
    readonly home: string,
    private resources: string,
    private store: DesktopStore,
    private mockUrl?: string,
    private installRuntime = false
  ) {
    super()
    this.manuallyStopped = Boolean(store.state.daemonStopped)
    this.view = {
      connected: false,
      home,
      baseUrl: 'http://127.0.0.1:8042',
      binary: null
    }
  }
  private get bundled(): string {
    return join(this.resources, 'runtime', executable)
  }
  /**
   * Whether the shared `anda` is the one this app installs from its bundle on
   * start, so a desktop update also updates it. Known once `discover` ran.
   */
  get bundleOwnsRuntime(): boolean {
    const action = this.installReport?.action
    return Boolean(action && action !== 'homebrew' && !this.store.state.binary)
  }
  /** The release bundled with this app, as `vX.Y.Z`. */
  async bundledRelease(): Promise<string | null> {
    try {
      const manifest = JSON.parse(
        await readFile(join(this.resources, 'runtime/manifest.json'), 'utf8')
      )
      const version = String(manifest.version || '')
        .split(/\s+/)
        .pop()
      return version ? `v${version.replace(/^v/, '')}` : null
    } catch {
      return null
    }
  }
  /** Resolves the shared `anda` CLI once per process (and after a failure). */
  async discover(): Promise<string> {
    const chosen = this.store.state.binary
    if (chosen) {
      if (await exists(chosen)) return chosen
      throw new Error('The anda executable chosen in Settings no longer exists.')
    }
    this.resolving ||= this.resolveRuntime().catch((error) => {
      this.resolving = undefined
      throw error
    })
    return this.resolving
  }
  private async resolveRuntime(): Promise<string> {
    if (!this.installRuntime) {
      // Development builds prefer an installed CLI, then the repository build.
      for (const candidate of [...installedCandidates(), this.bundled])
        if (await exists(candidate)) return candidate
      throw new Error(
        'Anda runtime was not found. Choose an installed anda executable in Settings.'
      )
    }
    await this.verifyBundle()
    let output: string
    try {
      output = await this.exec(this.bundled, ['install', '--json'], { timeout: 120_000 })
    } catch {
      throw new Error('Anda could not install its command-line runtime. Open the logs for details.')
    }
    const report = JSON.parse(output) as InstallReport
    this.installReport = report
    this.emit('installed', report)
    return report.path
  }
  /** Refuses a bundle whose runtime is not the one this package was built with. */
  private async verifyBundle(): Promise<void> {
    try {
      const manifest = JSON.parse(
        await readFile(join(this.resources, 'runtime/manifest.json'), 'utf8')
      )
      if (manifest.platform !== process.platform || manifest.arch !== process.arch)
        throw new Error('mismatch')
      // Stream the digest instead of holding the whole runtime in memory.
      const hash = createHash('sha256')
      for await (const chunk of createReadStream(this.bundled)) hash.update(chunk)
      if (manifest.sha256 !== hash.digest('hex')) throw new Error('mismatch')
    } catch {
      throw new Error('Bundled runtime verification failed. Reinstall a complete desktop package.')
    }
  }
  private exec(binary: string, args: string[], options: { timeout?: number; input?: string } = {}) {
    const pending = run(binary, ['--home', this.home, ...args], {
      timeout: options.timeout ?? 45_000,
      maxBuffer: 2 * 1024 * 1024,
      windowsHide: true
    })
    if (options.input !== undefined) {
      pending.child.stdin?.on('error', () => {}) // The child exit rejects `pending`.
      pending.child.stdin?.end(options.input)
    }
    return pending.then(({ stdout }) => stdout.trim())
  }
  private async command(args: string[], input?: string, timeout?: number): Promise<string> {
    if (this.mockUrl) throw new Error('Native daemon commands are disabled in mock tests')
    const binary = this.view.binary || (await this.discover())
    this.view.binary = binary
    try {
      return await this.exec(binary, args, { input, timeout })
    } catch (error) {
      const stdout = (error as { stdout?: string }).stdout?.trim()
      if (stdout?.startsWith('{')) return stdout
      // CLI output can contain identity/provider details; don't forward it to the renderer.
      throw new Error(`Anda ${args[0]} failed. Inspect the daemon log for details.`)
    }
  }
  /** Connects to the daemon; `start` also launches it when it is not running. */
  connect(start = true): Promise<DaemonView> {
    this.manuallyStopped = false
    this.store.state.daemonStopped = false
    if (
      this.view.connected &&
      this.socket?.readyState === WebSocket.OPEN &&
      Date.now() - this.credentialAt < 20 * 3600_000
    )
      return Promise.resolve({ ...this.view })
    if (this.connecting) return this.connecting
    this.connecting = this.establish(start).finally(() => {
      this.connecting = undefined
    })
    return this.connecting
  }
  async control(action: 'stop' | 'restart'): Promise<DaemonView> {
    this.manuallyStopped = true
    this.disconnect()
    try {
      await this.command([action])
      this.store.state.daemonStopped = action === 'stop'
      await this.store.save()
      if (action === 'restart') return this.connect()
      this.view.error = 'Anda daemon was stopped. Reconnect to start it again.'
      this.emit('change', this.view)
      return { ...this.view }
    } catch (error) {
      this.view.error = error instanceof Error ? error.message : 'Daemon control failed'
      this.emit('change', this.view)
      throw error
    }
  }
  private async establish(start: boolean): Promise<DaemonView> {
    clearInterval(this.heartbeat)
    try {
      if (this.mockUrl) {
        this.view.baseUrl = loopbackBaseUrl(this.mockUrl)
        this.bearer = 'desktop-test-token'
      } else {
        this.view.binary = await this.discover()
        const status = JSON.parse(await this.command(['status', '--json'])) as {
          state: string
        }
        if (status.state === 'not_running') {
          if (!start) throw new Error('The Anda service is not running. Reconnect to start it.')
          await this.command(['start'])
        } else if (!['running', 'gateway_running', 'process_unresponsive'].includes(status.state))
          throw new Error(
            'The existing daemon is unresponsive. Restart it explicitly before reconnecting.'
          )
        const token = JSON.parse(
          await this.command(['browser', 'token', '--json', '--days', '1'])
        ) as { gateway_url: string; token: string }
        this.view.baseUrl = loopbackBaseUrl(token.gateway_url)
        if (!token.token) throw new Error('Could not obtain a local desktop credential')
        this.bearer = token.token
      }
      this.credentialAt = Date.now()
      {
        const status = (await fetch(`${this.view.baseUrl}/daemon/status`, {
          signal: AbortSignal.timeout(10_000),
          redirect: 'error'
        }).then((r) => (this.mockUrl && !r.ok ? {} : r.json()))) as { needs_setup?: boolean }
        this.view.needsSetup = status.needs_setup === true
        delete this.view.setupIssues
        if (this.view.needsSetup) {
          this.view.connected = false
          const config = (await this.config('GET')) as { setup_issues?: unknown }
          if (Array.isArray(config.setup_issues))
            this.view.setupIssues = config.setup_issues.filter(
              (issue): issue is string => typeof issue === 'string'
            )
          this.view.error =
            'Connect ChatGPT or configure an API provider in Settings to get started.'
          this.heartbeat = setInterval(() => {
            void fetch(`${this.view.baseUrl}/daemon/status`, {
              signal: AbortSignal.timeout(2000),
              redirect: 'error'
            })
              .then((r) => r.json() as Promise<{ needs_setup?: boolean }>)
              .then((status) => {
                if (status.needs_setup !== true) return this.connect()
              })
              .catch(() => {})
          }, 2000)
          this.emit('change', this.view)
          return { ...this.view }
        }
      }
      await this.openSocket(false)
      this.view.connected = true
      delete this.view.error
      await this.request('information', [])
      const capabilities = (await this.request('capabilities', [])) as {
        desktop?: {
          protocol?: number
          runtime_version?: string
          app_transport?: boolean
        }
      }
      this.view.desktopProtocol = capabilities.desktop?.protocol || 0
      this.view.version = capabilities.desktop?.runtime_version
      if (capabilities.desktop?.app_transport) {
        await this.openSocket(true)
        const initialized = (await this.request('initialize', {})) as AppInitialize
        if (
          initialized.protocolVersion !== 1 ||
          !initialized.capabilities.stateInvalidation ||
          !initialized.capabilities.submissionReceipts
        )
          throw new Error('Unsupported desktop application protocol')
        this.view.liveEvents = true
        this.view.connected = true
        await this.reconcileSubmissions()
      }
      this.heartbeat = setInterval(() => {
        void this.request('ping', [])
          .then(() => this.reconcileSubmissions())
          .catch(() => this.socket?.terminate())
      }, 25_000)
      this.emit('change', this.view)
    } catch (error) {
      this.view.connected = false
      this.view.needsSetup = false
      this.view.error = error instanceof Error ? error.message : 'Connection failed'
      this.emit('change', this.view)
    }
    return { ...this.view }
  }
  private async openSocket(appTransport: boolean): Promise<void> {
    this.disconnect()
    this.appTransport = appTransport
    const connectionId = ++this.connectionId
    const ws = new WebSocket(
      `${this.view.baseUrl.replace(/^http/, 'ws')}${appTransport ? '/ws/app/v1' : '/ws/engine/default'}`,
      {
        headers: { Authorization: `Bearer ${this.bearer}` },
        maxPayload: 40 * 1024 * 1024
      }
    )
    this.socket = ws
    ws.on('message', (data) => {
      if (this.socket === ws) this.receive(data.toString(), connectionId)
    })
    ws.on('close', () => {
      if (this.socket !== ws) return
      this.view.connected = false
      this.failPending(new Error('WebSocket connection closed; write results may be unknown'))
      this.emit('change', this.view)
    })
    ws.on('error', () => {
      /* Handled by the connect promise or close event. */
    })
    await new Promise<void>((resolve, reject) => {
      const timer = setTimeout(() => {
        ws.terminate()
        reject(new Error('Local daemon connection timed out'))
      }, 12_000)
      ws.once('open', () => {
        clearTimeout(timer)
        resolve()
      })
      ws.once('error', () => {
        clearTimeout(timer)
        reject(new Error('Could not connect to the local Anda daemon'))
      })
    })
  }
  private receive(raw: string, connectionId: number): void {
    let message: {
      id?: number
      method?: string
      params?: unknown
      result?: unknown
      error?: unknown
    }
    try {
      message = JSON.parse(raw)
    } catch {
      return
    }
    if (message.method === 'state/changed' && this.appTransport) {
      this.emit('state', message.params)
      return
    }
    if (message.method === 'browser_action') {
      this.emit('browser-action', { ...message, connectionId })
      return
    }
    if (message.method || typeof message.id !== 'number') return
    const waiter = this.waiting.get(message.id)
    if (!waiter) return
    this.waiting.delete(message.id)
    clearTimeout(waiter.timer)
    if (message.error != null)
      waiter.reject(
        new Error(
          typeof message.error === 'string'
            ? message.error
            : (message.error as { message?: string }).message || 'Daemon request failed'
        )
      )
    else waiter.resolve(message.result)
  }
  private request(method: string, params: unknown): Promise<unknown> {
    const ws = this.socket
    if (!ws || ws.readyState !== WebSocket.OPEN)
      return Promise.reject(new Error('WebSocket is not connected'))
    const id = ++this.sequence
    return new Promise((resolve, reject) => {
      const timeout = method === 'ping' ? 10_000 : 30 * 60_000
      const timer = setTimeout(() => {
        this.waiting.delete(id)
        reject(new Error('WebSocket request timed out; write results may be unknown'))
      }, timeout)
      this.waiting.set(id, { resolve, reject, timer })
      ws.send(
        JSON.stringify({ ...(this.appTransport ? { jsonrpc: '2.0' } : {}), id, method, params }),
        (error) => {
          if (error) {
            this.waiting.delete(id)
            clearTimeout(timer)
            reject(new Error('WebSocket send failed; write results may be unknown'))
          }
        }
      )
    })
  }
  async rpc(method: string, params: unknown[], submissionId?: string): Promise<unknown> {
    validateRpc(method, params)
    if (
      submissionId !== undefined &&
      (typeof submissionId !== 'string' ||
        !/^[0-9a-f]{8}-(?:[0-9a-f]{4}-){3}[0-9a-f]{12}$/i.test(submissionId) ||
        this.store.state.pending.some((p) => p.id === submissionId))
    )
      throw new Error('Invalid or pending submission ID')
    if (this.manuallyStopped)
      throw new Error('The daemon is stopped. Reconnect explicitly to start it.')
    if (!this.view.connected || Date.now() - this.credentialAt > 20 * 3600_000) await this.connect()
    if (!this.view.connected) throw new Error(this.view.error || 'Daemon unavailable')
    if (method !== 'agent_run') return this.request(method, params)
    const input = params[0] as {
      prompt: string
      meta?: Record<string, unknown>
    }
    if (/^\/(stop|cancel)(?:\s|$)/.test(input.prompt)) return this.request(method, params)
    const source = String(input.meta?.source || '')
    if (source.startsWith('desktop:') && input.meta?.workspace && !this.view.desktopProtocol)
      throw new Error(
        'Restart the daemon from Settings to enable desktop workspaces with the bundled runtime.'
      )
    if (!source) throw new Error('Chat source is required')
    if (this.store.state.pending.some((p) => p.source === source && p.state === 'unknown'))
      throw new Error(
        'A previous submission is unconfirmed. Review the conversation before sending again.'
      )
    const submission = {
      id: submissionId || randomUUID(),
      source,
      prompt: input.prompt.slice(0, 2000),
      time: Date.now(),
      state: 'sending' as const,
      receipt: this.appTransport
    }
    this.store.state.pending.push(submission)
    await this.store.save()
    try {
      if (submission.receipt) {
        const receipt = (await this.request('chat/submit', {
          requestId: submission.id,
          input
        } satisfies AppSubmit)) as SubmissionReceipt
        // Main surviving a renderer reload is not proof that the UI received
        // the result. Keep its durable reference until the renderer ACKs it.
        if (receipt.state === 'completed' || receipt.state === 'failed')
          await this.settlePending(submission.id, receipt.state)
        return this.receiptResult(receipt)
      }
      const result = await this.request(method, params)
      await this.settlePending(submission.id, null)
      return result
    } catch (error) {
      if (
        this.store.state.pending.some(
          (p) => p.id === submission.id && ['completed', 'failed'].includes(p.state)
        )
      )
        throw error
      const text = error instanceof Error ? error.message : String(error)
      if (/WebSocket|SUBMISSION_UNKNOWN|outcome unknown/.test(text)) {
        await this.settlePending(submission.id, 'unknown')
        throw new Error(
          '[SUBMISSION_UNKNOWN] The connection was lost. Check the conversation before submitting again.'
        )
      }
      await this.settlePending(submission.id, null)
      throw error
    }
  }
  /** Records a submission's new state, or forgets it with `null`, then persists. */
  private async settlePending(id: string, state: PendingSubmission['state'] | null): Promise<void> {
    this.store.state.pending = state
      ? this.store.state.pending.map((p) => (p.id === id ? { ...p, state } : p))
      : this.store.state.pending.filter((p) => p.id !== id)
    await this.store.save()
  }
  private receiptResult(value: unknown): unknown {
    const receipt = value as SubmissionReceipt
    if (receipt.state === 'completed') return receipt.result
    if (receipt.state === 'failed') throw new Error(receipt.error || 'Submission failed')
    throw new Error(
      '[SUBMISSION_UNKNOWN] The daemon cannot yet confirm this submission. Do not resend.'
    )
  }
  async registerBrowserSession(session: string): Promise<void> {
    if (this.manuallyStopped) throw new Error('The daemon is stopped')
    await this.connect()
    if (!this.view.liveEvents)
      throw new Error(
        'Restart the daemon with the current desktop runtime to use its browser tools.'
      )
    // The daemon keeps a registration until this socket closes.
    const socket = this.socket
    if (this.browserSessions.has(session)) return
    await this.request('browser_register', [{ session, title: 'Anda Desktop' }])
    if (this.socket === socket) this.browserSessions.add(session)
  }
  async maintenance(
    action: 'begin' | 'renew' | 'release' | 'shutdown',
    token?: string
  ): Promise<{ token: string; ready: boolean }> {
    const response = await fetch(`${this.view.baseUrl}/daemon/maintenance`, {
      method: 'POST',
      headers: { Authorization: `Bearer ${this.bearer}`, 'Content-Type': 'application/json' },
      body: JSON.stringify({ action, token }),
      signal: AbortSignal.timeout(15_000),
      redirect: 'error'
    })
    if (!response.ok)
      throw new Error(
        `Runtime maintenance failed (${response.status}). Keep the current installation and retry later.`
      )
    return response.json()
  }
  /** Stops a drained daemon before installation can outlast its maintenance lease. */
  async stopForUpdate(token: string): Promise<void> {
    this.manuallyStopped = true
    await this.maintenance('shutdown', token)
    this.disconnect()
    for (let attempt = 0; attempt < 30; attempt++) {
      const status = JSON.parse(await this.command(['status', '--json']))
      if (status.state === 'not_running') return
      await new Promise((resolve) => setTimeout(resolve, 1000))
    }
    throw new Error('Runtime did not stop. Installation was not started.')
  }
  /** Checks for (and downloads) a new `anda` release without installing it. */
  async runtimeUpdateState(force: boolean): Promise<RuntimeUpdateState> {
    const output = await this.command(
      ['update', force ? '--check' : '--check-if-due', '--json'],
      undefined,
      10 * 60_000
    )
    return JSON.parse(output) as RuntimeUpdateState
  }
  /**
   * Installs the downloaded release into the shared CLI location. Unix
   * replaces the binary in place; Windows schedules the replacement for when
   * `anda update` exits and reports completion next to the executable.
   */
  async applyRuntimeUpdate(): Promise<void> {
    const binary = await this.discover()
    const status = `${binary}.update-status`
    if (process.platform === 'win32') await rm(status, { force: true })
    await this.command(['update'], undefined, 10 * 60_000)
    if (process.platform !== 'win32') return
    const deadline = Date.now() + 70_000
    for (;;) {
      let result: string
      try {
        result = (await readFile(status, 'utf8')).replace(/^\uFEFF/, '').trim()
      } catch (error) {
        // An up-to-date binary schedules no replacement.
        if ((error as NodeJS.ErrnoException).code === 'ENOENT') return
        throw error
      }
      if (result === 'installed') return
      if (result !== 'pending') throw new Error(result)
      if (Date.now() > deadline) throw new Error('Timed out replacing the anda executable')
      await new Promise((resolve) => setTimeout(resolve, 200))
    }
  }
  /** Starts the stopped daemon after its runtime changed or an install failed. */
  async startRuntime(): Promise<DaemonView> {
    this.manuallyStopped = true
    this.disconnect()
    await this.command(['start'])
    this.store.state.daemonStopped = false
    await this.store.save()
    return this.connect()
  }
  /** A 30-day bearer token for the Chrome extension. */
  async extensionToken(): Promise<string> {
    const report = JSON.parse(await this.command(['browser', 'token', '--json'])) as {
      token?: string
    }
    if (!report.token) throw new Error('Could not create a Chrome extension token')
    return report.token
  }
  browserReply(id: number, session: string, result: unknown, connectionId: number): void {
    if (this.socket?.readyState === WebSocket.OPEN && connectionId === this.connectionId)
      this.socket.send(
        JSON.stringify({ ...(this.appTransport ? { jsonrpc: '2.0' } : {}), id, session, result })
      )
  }
  private async reconcileSubmissions(): Promise<void> {
    if (!this.appTransport) return
    let changed = false
    for (const pending of [...this.store.state.pending]) {
      if (!pending.receipt || pending.state !== 'unknown') continue
      let receipt: SubmissionReceipt | null
      try {
        receipt = (await this.request('submission/read', {
          source: pending.source,
          requestId: pending.id
        })) as SubmissionReceipt | null
      } catch {
        // An unreadable receipt stays unknown for the user to review; it must
        // not keep the whole connection down.
        continue
      }
      if (receipt?.state === 'completed' || receipt?.state === 'failed') {
        await this.settlePending(pending.id, receipt.state)
        changed = true
      }
    }
    if (changed) {
      this.emit('submissions', this.store.state.pending)
      this.emit('state', { recovered: true })
    }
  }
  /** Null once the submission is no longer pending: acknowledged, or never
   * receipt-backed, so there is nothing to restore. The renderer's copy of the
   * pending list can trail an acknowledgement it has just made. */
  async readSubmission(id: string): Promise<SubmissionReceipt | null> {
    const pending = this.store.state.pending.find((p) => p.id === id && p.receipt)
    if (!pending) return null
    if (this.manuallyStopped) throw new Error('Reconnect to read the submission receipt')
    await this.connect()
    return this.request('submission/read', {
      source: pending.source,
      requestId: pending.id
    }) as Promise<SubmissionReceipt | null>
  }
  async acknowledgeSubmission(id: string): Promise<void> {
    if (typeof id !== 'string') throw new Error('Invalid submission')
    await this.settlePending(id, null)
    this.emit('submissions', this.store.state.pending)
  }
  async chatgpt(request: unknown): Promise<unknown> {
    if (
      !request ||
      typeof request !== 'object' ||
      !('method' in request) ||
      ![
        'accounts',
        'login_start',
        'login_status',
        'login_cancel',
        'account_select',
        'logout',
        'models',
        'model_select'
      ].includes(String(request.method)) ||
      Buffer.byteLength(JSON.stringify(request)) > 16 * 1024
    )
      throw new Error('Invalid ChatGPT request')
    if (!this.bearer) await this.connect()
    if (!this.bearer) throw new Error('Anda daemon is unavailable')
    const response = await fetch(`${this.view.baseUrl}/daemon/chatgpt`, {
      method: 'POST',
      headers: { Authorization: `Bearer ${this.bearer}`, 'Content-Type': 'application/json' },
      body: JSON.stringify(request),
      redirect: 'error',
      signal: AbortSignal.timeout(60_000)
    })
    const result = (await response.json()) as { error?: string }
    if (!response.ok) throw new Error(result.error || `ChatGPT request failed (${response.status})`)
    return result
  }
  async config(
    method: 'GET' | 'PUT',
    content?: string,
    expectedRevision?: string
  ): Promise<unknown> {
    if (!this.view.connected && !this.view.needsSetup) {
      const request = this.configWrites
        .catch(() => {})
        .then(() => this.offlineConfig(method, content, expectedRevision))
      this.configWrites = request
      return request
    }
    const response = await fetch(`${this.view.baseUrl}/daemon/config`, {
      method,
      headers: {
        Authorization: `Bearer ${this.bearer}`,
        'Content-Type': 'application/json'
      },
      ...(method === 'PUT'
        ? {
            body: JSON.stringify({
              content,
              expected_revision: expectedRevision
            })
          }
        : {}),
      signal: AbortSignal.timeout(30_000),
      redirect: 'error'
    })
    if (response.status === 409)
      throw new Error('Configuration changed since it was loaded. Reload before saving.')
    if (!response.ok) throw new Error(`Configuration request failed (${response.status})`)
    return response.json()
  }
  private async offlineConfig(
    method: 'GET' | 'PUT',
    content?: string,
    expectedRevision?: string
  ): Promise<unknown> {
    const path = join(this.home, 'config.yaml')
    let current: string
    try {
      current = await readFile(path, 'utf8')
    } catch (error) {
      if ((error as NodeJS.ErrnoException).code !== 'ENOENT')
        throw new Error('Configuration could not be read')
      try {
        current = await readFile(join(this.resources, 'config-template.yaml'), 'utf8')
      } catch {
        current = await readFile(join(this.resources, '../../anda_bot/assets/config.yaml'), 'utf8')
      }
    }
    const revision = (value: string) => createHash('sha3-384').update(value).digest('base64url')
    if (method === 'PUT') {
      if (expectedRevision && expectedRevision !== revision(current))
        throw new Error('Configuration changed since it was loaded. Reload before saving.')
      // Never race the live daemon's own config lock, even if its gateway is down.
      let pid: number | undefined
      try {
        pid = Number((await readFile(join(this.home, 'anda-daemon.pid'), 'utf8')).trim())
      } catch (error) {
        if ((error as NodeJS.ErrnoException).code !== 'ENOENT') throw error
      }
      if (pid && Number.isSafeInteger(pid) && pid > 0) {
        let active = true
        try {
          process.kill(pid, 0)
        } catch (error) {
          if ((error as NodeJS.ErrnoException).code === 'ESRCH') active = false
        }
        if (active)
          throw new Error('Stop the unresponsive daemon before editing its configuration offline.')
      }
      const next = content || ''
      // Uses the Rust parser without loading credentials, creating a DB or starting services.
      await this.command(['validate-config'], next)
      await mkdir(this.home, { recursive: true, mode: 0o700 })
      const temporary = `${path}.${randomUUID()}.tmp`
      try {
        await writeFile(`${path}.desktop-backup`, current, { mode: 0o600 })
        await writeFile(temporary, next, { mode: 0o600 })
        await rename(temporary, path)
      } catch {
        throw new Error('Could not save configuration; the previous copy is preserved.')
      }
      current = next
    }
    let config: unknown
    try {
      config = parseYaml(current)
    } catch {
      throw new Error(
        'Configuration contains invalid YAML. Repair config.yaml before reconnecting.'
      )
    }
    return { path, content: current, config, revision: revision(current) }
  }
  private failPending(error: Error): void {
    for (const waiter of this.waiting.values()) {
      clearTimeout(waiter.timer)
      waiter.reject(error)
    }
    this.waiting.clear()
  }
  disconnect(): void {
    clearInterval(this.heartbeat)
    const ws = this.socket
    this.socket = null
    this.browserSessions.clear()
    ws?.close()
    this.failPending(new Error('WebSocket disconnected'))
    this.view.connected = false
    this.view.liveEvents = false
    this.view.needsSetup = false
  }
}
