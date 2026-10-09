<script lang="ts">
  import { onMount } from 'svelte'
  import { Terminal } from '@xterm/xterm'
  import { FitAddon } from '@xterm/addon-fit'
  import { SearchAddon } from '@xterm/addon-search'
  import '@xterm/xterm/css/xterm.css'
  import {
    SCROLLBACK,
    appendScrollback,
    type TerminalSession,
    type TerminalEvent
  } from '../shared/workbench'
  import { ChevronDown, ChevronUp, SquareTerminal, X } from '@lucide/svelte'
  import { label, type Label } from './labels'
  let { workspace, language, dark }: { workspace: string; language: string; dark: boolean } =
    $props()
  const t = (key: Label) => label(language, key)
  // The terminal follows the app theme; ANSI colours stay legible on both.
  const themes = {
    light: {
      background: '#fcfcfb',
      foreground: '#292926',
      cursor: '#292926',
      selectionBackground: '#d9d9d3',
      black: '#292926',
      red: '#b3412f',
      green: '#3f7a3a',
      yellow: '#8a6a12',
      blue: '#2f5f9e',
      magenta: '#8b4a8f',
      cyan: '#2b7a7a',
      white: '#9a9a94',
      brightBlack: '#6f6f69',
      brightRed: '#c9503c',
      brightGreen: '#4c8f46',
      brightYellow: '#9c7a17',
      brightBlue: '#3a70b5',
      brightMagenta: '#a05aa4',
      brightCyan: '#358f8f',
      brightWhite: '#b5b5ae'
    },
    dark: {
      background: '#1b1b1a',
      foreground: '#e8e8e2',
      cursor: '#e8e7df',
      selectionBackground: '#3a3a37'
    }
  }
  let host: HTMLDivElement
  let terminal: Terminal
  let fit: FitAddon
  const search = new SearchAddon()
  let sessions = $state<TerminalSession[]>([])
  let active = $state('')
  let error = $state('')
  let query = $state('')
  let finding = $state(false)
  let findInput = $state<HTMLInputElement | null>(null)
  let disposed = false
  // Showing a session replays its output, and xterm answers the terminal
  // queries in it (device attributes, cursor position, colors) again. Those
  // answers belong to the past, so input is held back until the replay is
  // parsed. reset() keeps queued writes, so a newer replay owns the gate.
  let replays = 0
  let replaying = 0
  function fail(e: unknown) {
    error = e instanceof Error ? e.message : String(e)
  }
  function select(session: TerminalSession) {
    active = session.id
    terminal.reset()
    const replay = ++replays
    replaying = replay
    terminal.write(session.output.slice(-SCROLLBACK), () => {
      if (replaying === replay) replaying = 0
    })
    void window.anda
      .terminal({ action: 'ack', id: session.id, sequence: session.sequence })
      .catch(() => {})
    terminal.focus()
    resize()
  }
  function resize() {
    if (!terminal || !host?.clientWidth) return
    fit.fit()
    if (active && sessions.find((s) => s.id === active)?.exited === undefined)
      void window.anda
        .terminal({
          action: 'resize',
          id: active,
          cols: Math.min(500, Math.max(2, terminal.cols)),
          rows: Math.min(300, Math.max(1, terminal.rows))
        })
        .catch(fail)
  }
  /** ⌘F and the header's search button. */
  export function toggleFind() {
    finding = !finding
    if (finding) queueMicrotask(() => findInput?.select())
    else {
      search.clearDecorations()
      terminal?.focus()
    }
  }
  export async function create() {
    error = ''
    try {
      const session = await window.anda.terminal<TerminalSession>({
        action: 'create',
        workspace,
        cols: terminal.cols,
        rows: terminal.rows
      })
      if (disposed) return
      sessions = [...sessions, session]
      select(session)
    } catch (e) {
      fail(e)
    }
  }
  async function close(id: string) {
    try {
      await window.anda.terminal({ action: 'close', id })
      sessions = sessions.filter((s) => s.id !== id)
      if (active === id) {
        active = ''
        terminal.reset()
        if (sessions[0]) select(sessions[0])
      }
    } catch (e) {
      fail(e)
    }
  }
  onMount(() => {
    terminal = new Terminal({
      fontSize: 12,
      fontFamily: 'ui-monospace, SFMono-Regular, Menlo, Consolas, monospace',
      cursorBlink: true,
      scrollback: 5000,
      theme: dark ? themes.dark : themes.light
    })
    fit = new FitAddon()
    terminal.loadAddon(fit)
    terminal.loadAddon(search)
    terminal.open(host)
    fit.fit()
    terminal.onData((data) => {
      if (active && !replaying)
        void window.anda.terminal({ action: 'input', id: active, data }).catch(fail)
    })
    const observer = new ResizeObserver(resize)
    observer.observe(host)
    const unsubscribe = window.anda.onEvent((event) => {
      if (event.type !== 'terminal') return
      const message = event.value as TerminalEvent
      const session = sessions.find((s) => s.id === message.id)
      if (!session || message.sequence <= session.sequence) return
      session.sequence = message.sequence
      if (message.data) {
        session.output = appendScrollback(session.output, message.data)
        if (active === session.id)
          terminal.write(message.data, () => {
            void window.anda
              .terminal({ action: 'ack', id: session.id, sequence: message.sequence })
              .catch(() => {})
          })
        else
          void window.anda
            .terminal({ action: 'ack', id: session.id, sequence: message.sequence })
            .catch(() => {})
      }
      if (message.exited !== undefined) {
        session.exited = message.exited
        if (active === session.id) terminal.write(`\r\n[exit ${message.exited}]\r\n`)
      }
    })
    void window.anda
      .terminal<TerminalSession[]>({ action: 'list', workspace })
      .then((list) => {
        if (!disposed) {
          sessions = list
          if (list[0]) select(list[0])
        }
      })
      .catch(fail)
    return () => {
      disposed = true
      unsubscribe()
      observer.disconnect()
      terminal.dispose()
    }
  })
  $effect(() => {
    const theme = dark ? themes.dark : themes.light
    if (terminal) terminal.options.theme = theme
  })
</script>

<div class="terminal-panel" class:dark>
  {#if sessions.length}<div class="workbench-tabs terminal-tabs">
      {#each sessions as session, i (session.id)}<div class:active={active === session.id}>
          <button onclick={() => select(session)}
            >{i + 1} · {session.title}{session.exited !== undefined ? ' ◦' : ''}</button
          ><button aria-label={t('close')} onclick={() => close(session.id)}><X size={12} /></button
          >
        </div>{/each}
    </div>{/if}
  {#if finding}<div class="find-bar">
      <input
        bind:this={findInput}
        aria-label={t('find')}
        placeholder={t('find')}
        bind:value={query}
        onkeydown={(event) => {
          if (event.key === 'Enter')
            event.shiftKey ? search.findPrevious(query) : search.findNext(query)
          if (event.key === 'Escape') {
            event.stopPropagation()
            toggleFind()
          }
        }}
      /><button
        class="icon-button"
        aria-label={t('previous')}
        onclick={() => search.findPrevious(query)}><ChevronUp size={14} /></button
      ><button class="icon-button" aria-label={t('next')} onclick={() => search.findNext(query)}
        ><ChevronDown size={14} /></button
      ><button class="icon-button" aria-label={t('close')} onclick={toggleFind}
        ><X size={14} /></button
      >
    </div>{/if}
  {#if error}<p class="workbench-error" role="alert">{error}</p>{/if}
  <div class="terminal-surface" class:empty={!sessions.length} bind:this={host}></div>
  {#if !sessions.length}<div class="terminal-empty">
      <SquareTerminal size={22} />
      <button class="primary" onclick={create}>{t('startTerminal')}</button>
    </div>{/if}
</div>
