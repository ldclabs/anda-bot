import { spawn, type IPty } from 'node-pty'
import { execFile } from 'node:child_process'
import { promisify } from 'node:util'

const port = process.parentPort
let terminal: IPty | undefined
let pending = 0
let closing = false
const execute = promisify(execFile)

async function close(): Promise<void> {
  if (closing) return
  closing = true
  const pid = terminal?.pid
  if (pid) {
    if (process.platform === 'win32') {
      await execute('taskkill', ['/PID', String(pid), '/T', '/F'], { windowsHide: true }).catch(
        () => {}
      )
    } else {
      // Job control gives foreground jobs their own process groups. Walk the
      // owned shell's descendants rather than only signalling its group.
      const { stdout } = await execute('/bin/ps', ['-axo', 'pid=,ppid=']).catch(() => ({
        stdout: ''
      }))
      const rows = stdout
        .trim()
        .split('\n')
        .map((s) => s.trim().split(/\s+/).map(Number))
      const children = new Set([pid])
      for (let changed = true; changed; ) {
        changed = false
        for (const [child, parent] of rows)
          if (children.has(parent) && !children.has(child)) {
            children.add(child)
            changed = true
          }
      }
      for (const child of [...children].reverse()) {
        try {
          process.kill(child, 'SIGTERM')
        } catch {}
      }
    }
    try {
      terminal?.kill()
    } catch {}
  }
  process.exit(0)
}

port.on('message', ({ data }) => {
  try {
    if (data.action === 'start' && !terminal) {
      const env = Object.fromEntries(
        Object.entries(process.env).filter(
          ([key, value]) =>
            value !== undefined && !/^(ELECTRON_|NODE_OPTIONS|ANDA_DESKTOP_)/.test(key)
        )
      ) as Record<string, string>
      const shell =
        process.platform === 'win32'
          ? process.env.COMSPEC || 'C:\\Windows\\System32\\cmd.exe'
          : process.env.SHELL || '/bin/zsh'
      terminal = spawn(shell, process.platform === 'win32' ? [] : ['-l'], {
        cwd: data.workspace,
        name: 'xterm-256color',
        cols: data.cols,
        rows: data.rows,
        env: { ...env, TERM: 'xterm-256color', COLORTERM: 'truecolor' }
      })
      terminal.onData((text) => {
        pending += text.length
        if (pending > 256 * 1024) terminal?.pause()
        port.postMessage({ type: 'data', data: text })
      })
      terminal.onExit(({ exitCode }) => {
        port.postMessage({ type: 'exit', exited: exitCode })
        if (!closing) process.exit(0)
      })
      port.postMessage({ type: 'ready' })
    } else if (data.action === 'input') terminal?.write(data.data)
    else if (data.action === 'resize') terminal?.resize(data.cols, data.rows)
    else if (data.action === 'ack') {
      pending = Math.max(0, pending - data.length)
      if (pending < 64 * 1024) terminal?.resume()
    } else if (data.action === 'close') void close()
  } catch (error) {
    port.postMessage({
      type: 'error',
      error: error instanceof Error ? error.message : 'Terminal failed'
    })
  }
})
process.on('SIGTERM', () => void close())
process.on('SIGHUP', () => void close())
