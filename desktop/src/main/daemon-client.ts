import { EventEmitter } from 'node:events'
import { access, readFile, writeFile, mkdir, rename } from 'node:fs/promises'
import { execFile } from 'node:child_process'
import { promisify } from 'node:util'
import { join } from 'node:path'
import { homedir } from 'node:os'
import { randomUUID, createHash } from 'node:crypto'
import { parse as parseYaml } from 'yaml'
import WebSocket from 'ws'
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
  private connecting?: Promise<DaemonView>
  private heartbeat?: NodeJS.Timeout
  private credentialAt = 0
  private configWrites: Promise<unknown> = Promise.resolve()
  constructor(
    readonly home: string,
    private resources: string,
    private store: DesktopStore,
    private mockUrl?: string
  ) {
    super()
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
    for (const candidate of candidates) {
      if (!candidate) continue
      try {
        await access(candidate)
        return candidate
      } catch {
        /* Try the next installation. */
      }
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
          ...(binary.startsWith(join(this.resources, 'runtime'))
            ? { ANDA_DESKTOP_MANAGED_RUNTIME: '1' }
            : {})
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
          this.view.managed = this.view.binary.startsWith(join(this.resources, 'runtime'))
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
      this.disconnect()
      const ws = new WebSocket(`${this.view.baseUrl.replace(/^http/, 'ws')}/ws/engine/default`, {
        headers: { Authorization: `Bearer ${this.bearer}` },
        maxPayload: 40 * 1024 * 1024
      })
      this.socket = ws
      ws.on('message', (data) => this.receive(data.toString()))
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
      this.view.connected = true
      delete this.view.error
      await this.request('information', [])
      const capabilities = (await this.request('capabilities', [])) as {
        desktop?: { protocol?: number; runtime_version?: string; managed_runtime?: boolean }
      }
      this.view.desktopProtocol = capabilities.desktop?.protocol || 0
      this.view.version = capabilities.desktop?.runtime_version
      this.view.managed = capabilities.desktop?.managed_runtime || false
      this.heartbeat = setInterval(() => {
        void this.request('ping', []).catch(() => ws.terminate())
      }, 25_000)
      this.emit('change', this.view)
    } catch (error) {
      this.view.connected = false
      this.view.error = error instanceof Error ? error.message : 'Connection failed'
      this.emit('change', this.view)
    }
    return { ...this.view }
  }
  private receive(raw: string): void {
    let message: {
      id?: number
      method?: string
      result?: unknown
      error?: unknown
    }
    try {
      message = JSON.parse(raw)
    } catch {
      return
    }
    if (message.method || typeof message.id !== 'number') return
    const waiter = this.waiting.get(message.id)
    if (!waiter) return
    this.waiting.delete(message.id)
    clearTimeout(waiter.timer)
    if (message.error != null)
      waiter.reject(
        new Error(typeof message.error === 'string' ? message.error : JSON.stringify(message.error))
      )
    else waiter.resolve(message.result)
  }
  private request(method: string, params: unknown[]): Promise<unknown> {
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
      ws.send(JSON.stringify({ id, method, params }), (error) => {
        if (error) {
          this.waiting.delete(id)
          clearTimeout(timer)
          reject(new Error('WebSocket send failed; write results may be unknown'))
        }
      })
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
      state: 'sending' as const
    }
    this.store.state.pending.push(submission)
    await this.store.save()
    try {
      const result = await this.request(method, params)
      this.store.state.pending = this.store.state.pending.filter((p) => p.id !== submission.id)
      await this.store.save()
      return result
    } catch (error) {
      const text = error instanceof Error ? error.message : String(error)
      if (/WebSocket/.test(text)) {
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
  }
}
