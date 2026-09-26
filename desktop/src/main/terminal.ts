import { utilityProcess, type UtilityProcess } from 'electron'
import { randomUUID } from 'node:crypto'
import { join, basename } from 'node:path'
import type { TerminalEvent, TerminalRequest, TerminalSession } from '../shared/workbench'

interface Session extends TerminalSession {
  child: UtilityProcess
  pending: string
  awaiting: boolean
  timer?: NodeJS.Timeout
}
export class TerminalService {
  private sessions = new Map<string, Session>()
  constructor(
    private authorize: (path: string) => Promise<string>,
    private emit: (event: TerminalEvent) => void,
    private canCreate: () => boolean = () => true
  ) {}
  get running(): number {
    return [...this.sessions.values()].filter((s) => s.exited === undefined).length
  }
  private snapshot(session: Session): TerminalSession {
    return {
      id: session.id,
      workspace: session.workspace,
      title: session.title,
      exited: session.exited,
      output: session.output,
      sequence: session.sequence
    }
  }
  private flush(session: Session): void {
    session.timer = undefined
    if (session.awaiting || !session.pending) return
    const data = session.pending.slice(0, 64 * 1024)
    session.pending = session.pending.slice(data.length)
    session.awaiting = true
    this.emit({ id: session.id, sequence: ++session.sequence, data })
  }
  uses(workspace: string): boolean {
    return [...this.sessions.values()].some(
      (s) => s.workspace === workspace && s.exited === undefined
    )
  }
  private dimensions(cols: number, rows: number): void {
    if (![cols, rows].every(Number.isInteger) || cols < 2 || cols > 500 || rows < 1 || rows > 300)
      throw new Error('Invalid terminal dimensions')
  }
  async request(request: TerminalRequest): Promise<unknown> {
    if (!request || typeof request.action !== 'string') throw new Error('Invalid terminal request')
    if (request.action === 'list') {
      const workspace = await this.authorize(request.workspace)
      return [...this.sessions.values()]
        .filter((s) => s.workspace === workspace)
        .map((s) => {
          s.pending = ''
          s.awaiting = false
          return this.snapshot(s)
        })
    }
    if (request.action === 'create') {
      this.dimensions(request.cols, request.rows)
      const workspace = await this.authorize(request.workspace)
      if (!this.canCreate())
        throw new Error('Wait for the update to finish before opening a terminal.')
      if (this.sessions.size >= 12)
        throw new Error('Close an existing terminal before opening another.')
      const child = utilityProcess.fork(join(__dirname, 'pty-host.js'), [], {
        serviceName: 'Anda Terminal',
        stdio: 'ignore'
      })
      const session: Session = {
        id: randomUUID(),
        workspace,
        title: basename(workspace),
        output: '',
        sequence: 0,
        child,
        pending: '',
        awaiting: false
      }
      this.sessions.set(session.id, session)
      const ready = new Promise<void>((resolve, reject) => {
        const timer = setTimeout(() => {
          child.kill()
          reject(new Error('Terminal startup timed out'))
        }, 8000)
        child.on('message', (message) => {
          if (message.type === 'ready') {
            clearTimeout(timer)
            resolve()
          } else if (message.type === 'error') {
            clearTimeout(timer)
            reject(new Error(message.error))
            this.emit({
              id: session.id,
              sequence: ++session.sequence,
              data: `\r\n${message.error}\r\n`
            })
          } else if (message.type === 'data') {
            const data = String(message.data)
            session.output = (session.output + data).slice(-512 * 1024)
            session.pending += data
            if (session.pending.length > 512 * 1024)
              session.pending =
                '\r\n[Earlier output truncated]\r\n' + session.pending.slice(-256 * 1024)
            session.timer ||= setTimeout(() => this.flush(session), 16)
            child.postMessage({ action: 'ack', length: data.length })
          } else if (message.type === 'exit') {
            session.exited = message.exited
            clearTimeout(session.timer)
            this.emit({
              id: session.id,
              sequence: ++session.sequence,
              data: session.pending,
              exited: session.exited
            })
            session.pending = ''
            session.awaiting = false
          }
        })
        child.on('exit', (code) => {
          clearTimeout(timer)
          reject(new Error('Terminal process stopped before startup'))
          if (session.exited === undefined) {
            session.exited = code
            this.emit({ id: session.id, sequence: ++session.sequence, exited: code })
          }
        })
      })
      child.postMessage({ action: 'start', workspace, cols: request.cols, rows: request.rows })
      try {
        await ready
      } catch (error) {
        child.kill()
        this.sessions.delete(session.id)
        throw error
      }
      session.pending = ''
      session.awaiting = false
      return this.snapshot(session)
    }
    const session = this.sessions.get(request.id)
    if (!session) throw new Error('Terminal no longer exists')
    if (request.action === 'ack') {
      if (request.sequence === session.sequence) {
        session.awaiting = false
        this.flush(session)
      }
      return
    }
    if (request.action === 'close') {
      session.child.postMessage({ action: 'close' })
      clearTimeout(session.timer)
      this.sessions.delete(session.id)
      return
    }
    if (session.exited !== undefined) throw new Error('Terminal has exited')
    if (request.action === 'input') {
      if (typeof request.data !== 'string' || request.data.length > 64 * 1024)
        throw new Error('Terminal input is too large')
    } else if (request.action === 'resize') this.dimensions(request.cols, request.rows)
    else throw new Error('Unsupported terminal operation')
    session.child.postMessage(request)
  }
  async closeAll(): Promise<void> {
    await Promise.all(
      [...this.sessions.values()].map(
        (s) =>
          new Promise<void>((resolve) => {
            if (s.exited !== undefined) return resolve()
            const timer = setTimeout(() => {
              s.child.kill()
              resolve()
            }, 3000)
            s.child.once('exit', () => {
              clearTimeout(timer)
              resolve()
            })
            s.child.postMessage({ action: 'close' })
          })
      )
    )
    this.sessions.clear()
  }
}
