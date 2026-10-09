<script lang="ts" module>
  export type SettingsCategory = 'general' | 'appearance' | 'config' | 'audio' | 'runtime'
</script>

<script lang="ts">
  /**
   * Settings, one category at a time from a side list: General (model
   * connection, notifications, login start), Appearance, the agent's
   * configuration, Audio and the Runtime the app talks to.
   */
  import { AudioLines, Palette, Server, Settings2, SlidersHorizontal } from '@lucide/svelte'
  import DropdownMenu from '$lib/anda/DropdownMenu.svelte'
  import ConfigApp from '$extension/ConfigApp.svelte'
  import type { DesktopClient } from './client.svelte'
  import type { Preferences } from '../shared/contract'
  import type { Label } from './labels'
  import AudioPanel from './AudioPanel.svelte'
  import LocaleSwitcher from './LocaleSwitcher.svelte'
  import type { RuntimeActions } from './Sidebar.svelte'

  let {
    client,
    t,
    category = $bindable('general'),
    runtime
  }: {
    client: DesktopClient
    t: (key: Label) => string
    category?: SettingsCategory
    runtime: RuntimeActions
  } = $props()

  const categories: { id: SettingsCategory; label: Label; icon: typeof Settings2 }[] = [
    { id: 'general', label: 'general', icon: Settings2 },
    { id: 'appearance', label: 'theme', icon: Palette },
    { id: 'config', label: 'config', icon: SlidersHorizontal },
    { id: 'audio', label: 'audio', icon: AudioLines },
    { id: 'runtime', label: 'runtime', icon: Server }
  ]
  const themeItems = $derived<{ value: Preferences['theme']; label: string }[]>([
    { value: 'system', label: t('system') },
    { value: 'light', label: t('light') },
    { value: 'dark', label: t('dark') }
  ])

  async function preference(patch: Partial<Preferences>) {
    try {
      await client.savePreferences(patch)
    } catch (error) {
      client.fail(error)
    }
  }
  async function chooseBinary() {
    try {
      client.connection = await window.anda.chooseBinary()
      if (client.authorized) await client.refresh()
    } catch (error) {
      client.fail(error)
    }
  }
</script>

{#snippet toggle(checked: boolean, name: string, onchange: (value: boolean) => void)}
  <button
    type="button"
    role="switch"
    class="switch"
    aria-checked={checked}
    aria-label={name}
    onclick={() => onchange(!checked)}><span></span></button
  >
{/snippet}

<div class="settings-layout">
  <nav class="settings-nav" aria-label={t('settings')}>
    {#each categories as item (item.id)}<button
        class:active={category === item.id}
        aria-current={category === item.id ? 'page' : undefined}
        onclick={() => (category = item.id)}><item.icon size={15} />{t(item.label)}</button
      >{/each}
  </nav>
  {#if category === 'config'}<div class="management-page">
      <ConfigApp
        onModelsChanged={async () => {
          await window.anda.connect()
          if (client.authorized) await client.refreshModelState()
        }}
      />
    </div>
  {:else if category === 'audio'}<AudioPanel {client} />
  {:else if category === 'appearance'}<div class="settings-page">
      <h1>{t('theme')}</h1>
      <div class="setting-row">
        <span>{t('colorTheme')}</span><DropdownMenu
          items={themeItems}
          value={client.preferences.theme}
          onSelect={(theme) => void preference({ theme })}
          ariaLabel={t('theme')}
          align="end"
        />
      </div>
      <div class="setting-row">
        <span>{t('language')}</span><LocaleSwitcher {client} />
      </div>
    </div>
  {:else if category === 'runtime'}<div class="settings-page">
      <h1>{t('runtime')}</h1>
      <section class="runtime-card" aria-label={t('runtime')}>
        <div class="runtime-status">
          <span class:online={client.authorized} class="connection-dot"></span>
          <strong>{client.authorized ? t('connected') : t('disconnected')}</strong>
          {#if client.connection.version}<span class="muted">v{client.connection.version}</span
            >{/if}
        </div>
        {#if client.connection.error}<p class="workbench-error">{client.connection.error}</p>{/if}
        <dl>
          <dt>{t('executable')}</dt>
          <dd class="runtime-path">{client.connection.binary || t('noBinary')}</dd>
          <dt>{t('andaHome')}</dt>
          <dd class="runtime-path">{client.connection.home}</dd>
        </dl>
      </section>
      <div class="settings-buttons">
        <button onclick={() => void chooseBinary()}>{t('chooseBinary')}</button><button
          onclick={() => runtime.showLogs()}>{t('logs')}</button
        ><button onclick={() => void runtime.copyToken()}>{t('extensionToken')}</button><button
          onclick={() => void runtime.checkUpdate()}>{t('update')}</button
        >
      </div>
      <h2>{t('daemonControl')}</h2>
      <div class="settings-buttons">
        {#if client.authorized}<button onclick={() => void runtime.restart()}
            >{t('restartDaemon')}</button
          ><button class="danger" onclick={() => runtime.stop()}>{t('stopDaemon')}</button
          >{:else}<button class="primary" onclick={() => void runtime.reconnect()}
            >{t('reconnect')}</button
          >{/if}
      </div>
    </div>
  {:else}<div class="settings-page">
      <h1>{t('general')}</h1>
      <div class="setting-row">
        <span class="setting-label"
          ><span>{t('modelConnection')}</span><small>{t('modelConnectionHint')}</small></span
        >
        <button class="primary" onclick={() => (client.modelSetupOpen = true)}
          >{t('connectModel')}</button
        >
      </div>
      <div class="setting-row">
        <span class="setting-label"
          ><span>{t('notifications')}</span><small>{t('notificationsHint')}</small></span
        >
        {@render toggle(client.preferences.notifications, t('notifications'), (notifications) =>
          preference({ notifications })
        )}
      </div>
      <div class="setting-row">
        <span class="setting-label"><span>{t('login')}</span><small>{t('loginHint')}</small></span>
        {@render toggle(client.preferences.launchAtLogin, t('login'), (launchAtLogin) =>
          preference({ launchAtLogin })
        )}
      </div>
    </div>{/if}
</div>
