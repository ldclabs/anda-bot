import { EventEmitter } from 'node:events'
import { access, readFile, writeFile, mkdir, rename } from 'node:fs/promises'
import { execFile } from 'node:child_process'
import { promisify } from 'node:util'
import { join } from 'node:path'
import { homedir } from 'node:os'
import { randomUUID, createHash } from 'node:crypto'
import { parse as parseYaml } from 'yaml'
import WebSocket from 'ws'
import type { AppInitialize, AppSubmit, SubmissionReceipt } from '../shared/app-protocol'
import type { DaemonView } from '../shared/contract'
import { loopbackBaseUrl, validateRpc } from './policy'
import { DesktopStore } from './store'

const run = promisify(execFile)
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
  constructor(
    readonly home: string,
    private resources: string,
    private store: DesktopStore,
    private mockUrl?: string
  ) {
    super()
    this.manuallyStopped = Boolean(store.state.daemonStopped)
    this.view = {
      connected: false,
      home,
      baseUrl: 'http://127.0.0.1:8042',
      binary: null,
      managed: false
    }
  }
  async discover(): Promise<string> {
    const candidates = [
      this.store.state.binary,
      join(this.resources, 'runtime', process.platform === 'win32' ? 'anda.exe' : 'anda'),
      join(homedir(), '.local', 'bin', process.platform === 'win32' ? 'anda.exe' : 'anda'),
      '/opt/homebrew/bin/anda',
      '/usr/local/bin/anda'
    ]
    const bundled = join(
      this.resources,
      'runtime',
      process.platform === 'win32' ? 'anda.exe' : 'anda'
    )
    for (const candidate of candidates) {
      if (!candidate) continue
      try {
        await access(candidate)
      } catch (error) {
        if ((error as NodeJS.ErrnoException).code === 'ENOENT') continue
        throw error
      }
      if (candidate === bundled) {
        try {
          const manifest = JSON.parse(
            await readFile(join(this.resources, 'runtime/manifest.json'), 'utf8')
          )
          if (
            manifest.platform !== process.platform ||
            manifest.arch !== process.arch ||
            manifest.sha256 !==
              createHash('sha256')
                .update(await readFile(candidate))
                .digest('hex')
          )
            throw new Error('mismatch')
        } catch {
          throw new Error(
            'Bundled runtime verification failed. Reinstall a complete desktop package.'
          )
        }
      }
      return candidate
    }
    throw new Error('Anda runtime was not found. Choose an installed anda executable in Settings.')
  }
  private async command(args: string[], input?: string): Promise<string> {
    if (this.mockUrl) throw new Error('Native daemon commands are disabled in mock tests')
    const binary = this.view.binary || (await this.discover())
    try {
      const pending = run(binary, ['--home', this.home, ...args], {
        timeout: 45_000,
        maxBuffer: 2 * 1024 * 1024,
        windowsHide: true,
        env: {
          ...process.env,
          ANDA_DESKTOP_MANAGED_RUNTIME:
            binary ===
            join(this.resources, 'runtime', process.platform === 'win32' ? 'anda.exe' : 'anda')
              ? '1'
              : undefined
        }
      })
      if (input !== undefined) {
        pending.child.stdin?.on('error', () => {}) // The child exit rejects `pending`.
        pending.child.stdin?.end(input)
      }
      const { stdout } = await pending
      return stdout.trim()
    } catch (error) {
      const stdout = (error as { stdout?: string }).stdout?.trim()
      if (stdout?.startsWith('{')) return stdout
      // CLI output can contain identity/provider details; don't forward it to the renderer.
      throw new Error(`Anda ${args[0]} failed. Inspect the daemon log for details.`)
    }
  }
  connect(): Promise<DaemonView> {
    this.manuallyStopped = false
    this.store.state.daemonStopped = false
    if (
      this.view.connected &&
      this.socket?.readyState === WebSocket.OPEN &&
      Date.now() - this.credentialAt < 20 * 3600_000
    )
      return Promise.resolve({ ...this.view })
    if (this.connecting) return this.connecting
    this.connecting = this.establish().finally(() => {
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
  private async establish(): Promise<DaemonView> {
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
          await this.command(['start'])
          this.view.managed =
            this.view.binary ===
            join(this.resources, 'runtime', process.platform === 'win32' ? 'anda.exe' : 'anda')
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
      await this.openSocket(false)
      this.view.connected = true
      delete this.view.error
      await this.request('information', [])
      const capabilities = (await this.request('capabilities', [])) as {
        desktop?: {
          protocol?: number
          runtime_version?: string
          managed_runtime?: boolean
          runtime_path?: string
          app_transport?: boolean
        }
      }
      this.view.desktopProtocol = capabilities.desktop?.protocol || 0
      this.view.version = capabilities.desktop?.runtime_version
      const runtimePath = capabilities.desktop?.runtime_path
      const bundled = join(
        this.resources,
        'runtime',
        process.platform === 'win32' ? 'anda.exe' : 'anda'
      )
      this.view.managed = Boolean(capabilities.desktop?.managed_runtime && runtimePath === bundled)
      this.view.runtimeOwnership =
        capabilities.desktop?.managed_runtime && !runtimePath
          ? 'unknown'
          : this.view.managed
            ? 'managed'
            : 'external'
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
  async rpc(method: string, params: unknown[]): Promise<unknown> {
    validateRpc(method, params)
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
      id: randomUUID(),
      source,
      prompt: input.prompt.slice(0, 2000),
      time: Date.now(),
      state: 'sending' as const,
      receipt: this.appTransport
    }
    this.store.state.pending.push(submission)
    await this.store.save()
    try {
      const result = this.appTransport
        ? this.receiptResult(
            await this.request('chat/submit', {
              requestId: submission.id,
              input
            } satisfies AppSubmit)
          )
        : await this.request(method, params)
      this.store.state.pending = this.store.state.pending.filter((p) => p.id !== submission.id)
      await this.store.save()
      return result
    } catch (error) {
      const text = error instanceof Error ? error.message : String(error)
      if (/WebSocket|SUBMISSION_UNKNOWN|outcome unknown/.test(text)) {
        this.store.state.pending = this.store.state.pending.map((p) =>
          p.id === submission.id ? { ...p, state: 'unknown' } : p
        )
        await this.store.save()
        throw new Error(
          '[SUBMISSION_UNKNOWN] The connection was lost. Check the conversation before submitting again.'
        )
      }
      this.store.state.pending = this.store.state.pending.filter((p) => p.id !== submission.id)
      await this.store.save()
      throw error
    }
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
    await this.request('browser_register', [{ session, title: 'Anda Desktop' }])
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
      const receipt = (await this.request('submission/read', {
        source: pending.source,
        requestId: pending.id
      })) as SubmissionReceipt | null
      if (receipt && ['completed', 'failed'].includes(receipt.state)) {
        this.store.state.pending = this.store.state.pending.map((p) =>
          p.id === pending.id ? { ...p, state: receipt.state as 'completed' | 'failed' } : p
        )
        changed = true
      }
    }
    if (changed) {
      await this.store.save()
      this.emit('submissions', this.store.state.pending)
      this.emit('state', { recovered: true })
    }
  }
  async readSubmission(id: string): Promise<SubmissionReceipt | null> {
    const pending = this.store.state.pending.find((p) => p.id === id && p.receipt)
    if (!pending) throw new Error('No recorded submission with this ID')
    if (this.manuallyStopped) throw new Error('Reconnect to read the submission receipt')
    await this.connect()
    return this.request('submission/read', {
      source: pending.source,
      requestId: pending.id
    }) as Promise<SubmissionReceipt | null>
  }
  async config(
    method: 'GET' | 'PUT',
    content?: string,
    expectedRevision?: string
  ): Promise<unknown> {
    if (!this.view.connected) {
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
    ws?.close()
    this.failPending(new Error('WebSocket disconnected'))
    this.view.connected = false
    this.view.liveEvents = false
  }
}
