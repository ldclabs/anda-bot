<script lang="ts">
  /** A workbench panel in a window of its own; its menu puts it back on the main window's canvas. */
  import { onDestroy } from 'svelte'
  import { X } from '@lucide/svelte'
  import { applyAppearanceTheme } from '$lib/anda/theme'
  import type { PanelClient } from './panel-client.svelte'
  import { label, type Label } from './labels'
  import WorkbenchPanel from './WorkbenchPanel.svelte'
  let { client }: { client: PanelClient } = $props()
  const t = (key: Label) => label(client.language, key)
  let dark = $state(document.documentElement.classList.contains('dark'))
  let panel = $state<WorkbenchPanel | null>(null)

  $effect(() => applyAppearanceTheme(client.preferences.theme))
  $effect(() => {
    const observer = new MutationObserver(
      () => (dark = document.documentElement.classList.contains('dark'))
    )
    observer.observe(document.documentElement, { attributes: true, attributeFilter: ['class'] })
    return () => observer.disconnect()
  })
  $effect(() => {
    document.documentElement.lang = client.language.replace('_', '-')
    document.documentElement.dir = client.language === 'ar' ? 'rtl' : 'ltr'
  })
  $effect(() => {
    if (client.ready) document.title = `${t(client.tab)} · Anda`
  })
  const stop = window.anda.onEvent((event) => {
    if (event.type === 'menu' && event.value === 'find') panel?.find()
  })
  onDestroy(() => {
    stop()
    client.dispose()
  })
</script>

<div class="panel-window resource-panel controls-left controls-right">
  {#if client.ready}<WorkbenchPanel
      bind:this={panel}
      {client}
      {t}
      tab={client.tab}
      {dark}
      branch={client.context.branch}
      resources={client.context.resources}
      sessionFiles={client.context.sessionFiles}
      fileRequest={client.context.fileRequest}
      changeFocus={client.context.changeFocus}
      edge={null}
      detached
      onMove={(edge) => client.dock(edge)}
      onReturn={() => client.dock()}
    />{/if}
  {#if client.systemMessage}<div class="status-banner error" role="alert">
      <span>{client.systemMessage}</span><button
        class="icon-button"
        aria-label={t('close')}
        onclick={() => (client.systemMessage = null)}><X size={14} /></button
      >
    </div>{/if}
</div>
