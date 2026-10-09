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
  import { label, type Label } from './labels'
  let { workspace, language }: { workspace: string; language: string } = $props()
  const t = (key: Label) => label(language, key)
  let host: HTMLDivElement
  let terminal: Terminal
  let fit: FitAddon
  const search = new SearchAddon()
  let sessions = $state<TerminalSession[]>([])
  let active = $state('')
  let error = $state('')
  let query = $state('')
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
  async function create() {
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
      theme: { background: '#1d1e20', foreground: '#eeeeec', cursor: '#e8e7df' }
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
</script>

<div class="terminal-panel">
  <div class="workbench-toolbar">
    <button onclick={create}>{t('newTerminal')}</button><input
      aria-label={t('find')}
      placeholder={t('find')}
      bind:value={query}
      onkeydown={(event) => {
        if (event.key === 'Enter') search.findNext(query)
      }}
    />
  </div>
  <div class="workbench-tabs">
    {#each sessions as session, i}<div class:active={active === session.id}>
        <button onclick={() => select(session)}
          >{i + 1} · {session.title}{session.exited !== undefined ? ' ◦' : ''}</button
        ><button aria-label={t('close')} onclick={() => close(session.id)}>×</button>
      </div>{/each}
  </div>
  {#if error}<p class="workbench-error" role="alert">{error}</p>{/if}
  <div class="terminal-surface" bind:this={host}></div>
</div>
