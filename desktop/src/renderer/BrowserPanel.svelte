<script lang="ts">
  import { onMount } from 'svelte'
  import { ArrowLeft, ArrowRight, RotateCw, Plus, X, Download } from '@lucide/svelte'
  import type { BrowserState, BrowserRequest } from '../shared/browser'
  import { wb } from './workbench-labels'
  let { source, language }: { source: string; language: string } = $props()
  const t = (key: Parameters<typeof wb>[1]) => wb(language, key)
  let browserState = $state<BrowserState>({
    source: '',
    session: '',
    active: null,
    tabs: [],
    downloads: []
  })
  const active = $derived(browserState.tabs.find((tab) => tab.id === browserState.active))
  let address = $state('')
  let error = $state('')
  let downloads = $state(false)
  let find = $state('')
  let host: HTMLDivElement
  let disposed = false
  let frame = 0
  function update(next: BrowserState) {
    browserState = next
    address = next.tabs.find((tab) => tab.id === next.active)?.url || ''
  }
  async function request(value: BrowserRequest) {
    error = ''
    try {
      const next = await window.anda.browser(value)
      if (!disposed) update(next)
    } catch (e) {
      error = String(e)
    }
  }
  function position() {
    cancelAnimationFrame(frame)
    frame = requestAnimationFrame(() => {
      if (disposed || !host) return
      const rect = host.getBoundingClientRect()
      const obscured = Boolean(
        document.querySelector('.modal-backdrop, [role="dialog"], [role="menu"]')
      )
      void window.anda
        .browser({
          action: 'bounds',
          source,
          visible: !obscured && !document.hidden,
          x: rect.x,
          y: rect.y,
          width: rect.width,
          height: rect.height
        })
        .catch(() => {})
    })
  }
  onMount(() => {
    void request({ action: 'state', source })
    const unsubscribe = window.anda.onEvent((event) => {
      if (event.type === 'browser' && (event.value as BrowserState).source === source)
        update(event.value as BrowserState)
    })
    const resize = new ResizeObserver(position)
    resize.observe(host)
    const overlays = new MutationObserver(position)
    overlays.observe(document.body, { subtree: true, childList: true })
    window.addEventListener('resize', position)
    document.addEventListener('visibilitychange', position)
    position()
    return () => {
      disposed = true
      cancelAnimationFrame(frame)
      unsubscribe()
      resize.disconnect()
      overlays.disconnect()
      window.removeEventListener('resize', position)
      document.removeEventListener('visibilitychange', position)
      void window.anda
        .browser({ action: 'bounds', source, visible: false, x: 0, y: 0, width: 0, height: 0 })
        .catch(() => {})
    }
  })
</script>

<div class="browser-panel">
  <div class="workbench-tabs browser-tabs">
    {#each browserState.tabs as tab}<div class:active={tab.id === browserState.active}>
        <button onclick={() => request({ action: 'select', source, id: tab.id })}
          >{tab.loading ? '◦ ' : ''}{tab.title}</button
        ><button
          aria-label={t('close')}
          onclick={() => request({ action: 'close', source, id: tab.id })}><X size={12} /></button
        >
      </div>{/each}<button title={t('newTab')} onclick={() => request({ action: 'new', source })}
      ><Plus size={14} /></button
    >
  </div>
  <form
    class="browser-address"
    onsubmit={(event) => {
      event.preventDefault()
      if (active)
        void request({
          action: 'navigate',
          source,
          id: active.id,
          url:
            /^https?:\/\//i.test(address) || address === 'about:blank'
              ? address
              : `https://${address}`
        })
    }}
  >
    <button
      type="button"
      title={t('back')}
      disabled={!active?.canBack}
      onclick={() => request({ action: 'back', source, id: active!.id })}
      ><ArrowLeft size={15} /></button
    ><button
      type="button"
      title={t('forward')}
      disabled={!active?.canForward}
      onclick={() => request({ action: 'forward', source, id: active!.id })}
      ><ArrowRight size={15} /></button
    ><button
      type="button"
      title={t('reload')}
      disabled={!active}
      onclick={() => request({ action: 'reload', source, id: active!.id })}
      ><RotateCw size={14} /></button
    >
    <input
      aria-label={t('address')}
      placeholder="https://"
      disabled={!active}
      bind:value={address}
    /><button
      type="button"
      title={t('downloads')}
      onclick={() => {
        downloads = !downloads
        position()
      }}><Download size={15} /></button
    >
  </form>
  {#if error || active?.error}<p class="workbench-error" role="alert">
      {error || active?.error}
    </p>{/if}
  {#if downloads}<div class="browser-downloads">
      {#each browserState.downloads as item}<div>
          <span>{item.name} · {item.state}</span><button
            onclick={() =>
              request({
                action: item.state === 'completed' ? 'download-open' : 'download-cancel',
                source,
                id: item.id
              })}>{t(item.state === 'completed' ? 'open' : 'cancel')}</button
          >
        </div>{/each}
    </div>{/if}
  {#if active}<div class="browser-find">
      <input
        aria-label={t('search')}
        placeholder={t('search')}
        bind:value={find}
        onkeydown={(event) => {
          if (event.key === 'Enter')
            void request({ action: 'find', source, id: active!.id, text: find })
        }}
      />
    </div>{/if}
  <div class="browser-surface" bind:this={host}>
    {#if !active}<button class="primary-button" onclick={() => request({ action: 'new', source })}
        >{t('newTab')}</button
      >{/if}
  </div>
</div>
