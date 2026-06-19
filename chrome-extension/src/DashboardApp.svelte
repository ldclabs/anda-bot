<script lang="ts">
  import BrainApp from './BrainApp.svelte'
  import ConfigApp from './ConfigApp.svelte'
  import BookmarksWorkspace from '$lib/anda/dashboard/BookmarksWorkspace.svelte'
  import { andaClient } from '$lib/anda/client/side-panel.svelte'
  import { applyAppearanceTheme } from '$lib/anda/theme'
  import { buttonClass, nativeSelectClass } from '$lib/anda/ui'
  import { getMessage } from '$lib/i18n'
  import { cn } from '$lib/utils'
  import {
    Bookmark,
    BrainCircuit,
    Database,
    PanelLeftClose,
    PanelLeftOpen,
    RefreshCw,
    Save,
    Settings
  } from '@lucide/svelte'
  import { onMount } from 'svelte'

  type WorkspaceId = 'brain' | 'bookmarks' | 'config'
  const dashboardNavCollapsedStorageKey = 'andaDashboardNavCollapsed'

  interface Workspace {
    id: WorkspaceId
    label: string
    detail: string
    icon: typeof BrainCircuit
  }

  const workspaces: Workspace[] = [
    {
      id: 'brain',
      label: 'Brain',
      detail: getMessage('dashboardBrainDetail'),
      icon: BrainCircuit
    },
    {
      id: 'bookmarks',
      label: getMessage('bookmarks'),
      detail: getMessage('dashboardBookmarksDetail'),
      icon: Bookmark
    },
    {
      id: 'config',
      label: 'Config',
      detail: getMessage('dashboardConfigDetail'),
      icon: Settings
    }
  ]

  let activeWorkspace = $state<WorkspaceId>(workspaceFromHash())
  let navCollapsed = $state(loadNavCollapsed())

  const active = $derived(
    workspaces.find((workspace) => workspace.id === activeWorkspace) || workspaces[0]
  )
  const connectionLabel = $derived(
    andaClient.settings.baseUrl
      ? `${andaClient.settings.baseUrl.replace(/^https?:\/\//, '')}`
      : getMessage('dashboardNoConnection')
  )
  $effect(() => applyAppearanceTheme(andaClient.settings.appearanceTheme))

  onMount(() => {
    const syncHash = () => {
      activeWorkspace = workspaceFromHash()
    }
    window.addEventListener('hashchange', syncHash)
    andaClient.init().catch((error) => {
      andaClient.status = 'extension unavailable'
      console.error('Failed to initialize Anda dashboard client', error)
    })

    return () => {
      window.removeEventListener('hashchange', syncHash)
      andaClient.destroy()
    }
  })

  function workspaceFromHash(): WorkspaceId {
    const hash = window.location.hash.replace(/^#\/?/, '')
    if (hash === 'bookmarks' || hash === 'config' || hash === 'brain') {
      return hash
    }
    return 'brain'
  }

  function selectWorkspace(workspace: WorkspaceId) {
    activeWorkspace = workspace
    window.history.replaceState(null, '', `#${workspace}`)
  }

  function loadNavCollapsed(): boolean {
    if (typeof localStorage === 'undefined') {
      return false
    }
    try {
      return localStorage.getItem(dashboardNavCollapsedStorageKey) === 'true'
    } catch {
      return false
    }
  }

  function setNavCollapsed(collapsed: boolean) {
    navCollapsed = collapsed
    try {
      localStorage.setItem(dashboardNavCollapsedStorageKey, String(collapsed))
    } catch {
      // Keep the in-memory state when storage is unavailable.
    }
  }

  function reloadPage() {
    window.location.reload()
  }

  function saveActionLabel(): string {
    if (activeWorkspace === 'config') {
      return getMessage('dashboardSaveInConfig')
    }
    return getMessage('dashboardNoPendingSave')
  }
</script>

<svelte:head>
  <title>{getMessage('dashboardPageTitle')}</title>
</svelte:head>

<div
  class={cn(
    'grid h-screen w-screen overflow-hidden bg-background text-foreground transition-[grid-template-columns] duration-150',
    navCollapsed ? 'grid-cols-[4.25rem_minmax(0,1fr)]' : 'grid-cols-[13.75rem_minmax(0,1fr)]'
  )}
>
  <aside class="flex min-h-0 flex-col border-r bg-sidebar text-sidebar-foreground">
    <div
      class={cn(
        'flex h-16 shrink-0 items-center border-b',
        navCollapsed ? 'justify-center px-2' : 'justify-between gap-2 px-3'
      )}
    >
      {#if navCollapsed}
        <button
          type="button"
          class={buttonClass('ghost', 'icon-sm')}
          title={getMessage('dashboardExpandNavigation')}
          aria-label={getMessage('dashboardExpandNavigation')}
          onclick={() => setNavCollapsed(false)}
        >
          <PanelLeftOpen class="size-4" />
        </button>
      {:else}
        <div class="flex min-w-0 items-center gap-2">
          <div class="grid size-8 shrink-0 place-items-center rounded-md border bg-background">
            <Database class="size-4" />
          </div>
          <div class="min-w-0">
            <h1 class="truncate text-sm font-bold">{getMessage('dashboardTitle')}</h1>
            <p class="truncate text-[11px] text-muted-foreground">{connectionLabel}</p>
          </div>
        </div>
        <button
          type="button"
          class={buttonClass('ghost', 'icon-sm')}
          title={getMessage('dashboardCollapseNavigation')}
          aria-label={getMessage('dashboardCollapseNavigation')}
          onclick={() => setNavCollapsed(true)}
        >
          <PanelLeftClose class="size-4" />
        </button>
      {/if}
    </div>

    <nav class={cn('grid content-start gap-1 p-2', navCollapsed && 'place-items-center')}>
      {#each workspaces as workspace}
        {@const Icon = workspace.icon}
        <button
          type="button"
          class={cn(
            'grid min-w-0 items-center rounded-md text-left transition',
            navCollapsed
              ? 'size-10 place-items-center px-0 py-0'
              : 'grid-cols-[auto_minmax(0,1fr)] gap-2 px-2.5 py-2',
            activeWorkspace === workspace.id
              ? 'bg-background text-foreground shadow-xs'
              : 'text-muted-foreground hover:bg-background/70 hover:text-foreground'
          )}
          title={navCollapsed ? workspace.label : undefined}
          aria-label={workspace.label}
          aria-pressed={activeWorkspace === workspace.id}
          onclick={() => selectWorkspace(workspace.id)}
        >
          <Icon class="size-4 shrink-0" />
          {#if !navCollapsed}
            <span class="grid min-w-0 gap-0.5">
              <span class="truncate text-sm font-semibold">{workspace.label}</span>
              <span class="truncate text-[10px]">{workspace.detail}</span>
            </span>
          {/if}
        </button>
      {/each}
    </nav>

    <div class={cn('mt-auto grid gap-2 border-t p-2', navCollapsed && 'place-items-center')}>
      {#if !navCollapsed}
        <label class="grid gap-1 text-[11px] font-medium text-muted-foreground">
          {getMessage('appearanceTheme')}
          <select
            class={nativeSelectClass('h-8 text-xs')}
            bind:value={andaClient.settings.appearanceTheme}
          >
            <option value="system">{getMessage('appearanceSystem')}</option>
            <option value="light">{getMessage('appearanceLight')}</option>
            <option value="dark">{getMessage('appearanceDark')}</option>
          </select>
        </label>
      {/if}
      <button
        type="button"
        class={buttonClass(
          'outline',
          navCollapsed ? 'icon-sm' : 'sm',
          navCollapsed ? 'bg-background' : 'w-full justify-start bg-background text-xs'
        )}
        title={getMessage('dashboardConnectionSettings')}
        aria-label={getMessage('dashboardConnectionSettings')}
        onclick={() => selectWorkspace('config')}
      >
        <Settings class="size-3.5" />
        {#if !navCollapsed}
          <span class="truncate">{getMessage('dashboardConnectionSettings')}</span>
        {/if}
      </button>
    </div>
  </aside>

  <section class="grid min-h-0 min-w-0 grid-rows-[auto_minmax(0,1fr)]">
    <header
      class="flex h-16 min-w-0 shrink-0 flex-wrap items-center justify-between gap-3 border-b bg-background px-4"
    >
      <div class="flex min-w-0 items-center gap-3">
        {#if active.icon}
          {@const ActiveIcon = active.icon}
          <div class="grid size-9 shrink-0 place-items-center rounded-md border bg-card">
            <ActiveIcon class="size-4" />
          </div>
        {/if}
        <div class="min-w-0">
          <div class="flex min-w-0 items-center gap-2 text-sm font-bold">
            <span class="truncate">{active.label}</span>
            <span class="text-muted-foreground">/</span>
            <span class="truncate text-muted-foreground">
              {activeWorkspace === 'brain'
                ? getMessage('dashboardGraphCrumb')
                : activeWorkspace === 'bookmarks'
                  ? getMessage('dashboardLibraryCrumb')
                  : getMessage('dashboardStudioCrumb')}
            </span>
          </div>
          <p class="truncate text-xs text-muted-foreground">{active.detail}</p>
        </div>
      </div>

      <div class="flex min-w-0 flex-1 items-center justify-end gap-2">
        <button type="button" class={buttonClass('outline', 'sm')} onclick={reloadPage}>
          <RefreshCw class="size-3.5" />
          {getMessage('refresh')}
        </button>
        <button
          type="button"
          class={buttonClass(activeWorkspace === 'config' ? 'default' : 'outline', 'sm')}
          onclick={() => selectWorkspace('config')}
        >
          <Save class="size-3.5" />
          {saveActionLabel()}
        </button>
      </div>
    </header>

    <main class="min-h-0 min-w-0 overflow-hidden bg-background">
      {#if activeWorkspace === 'brain'}
        <BrainApp embedded />
      {:else if activeWorkspace === 'bookmarks'}
        <BookmarksWorkspace />
      {:else}
        <ConfigApp embedded />
      {/if}
    </main>
  </section>
</div>
