<script lang="ts" module>
  export type PanelTab = 'resources' | 'changes' | 'terminal' | 'browser'
  export interface FileRequest {
    id: number
    path: string
    line?: number
  }
</script>

<script lang="ts">
  /**
   * The right-hand workbench. The chat header's toggles choose the panel;
   * this header names it and carries the panel's own commands, maximize and
   * close. Resources also previews workspace files the transcript links to.
   */
  import { tick, untrack } from 'svelte'
  import {
    FolderOpen,
    Globe,
    GitCompare,
    Maximize2,
    Minimize2,
    Paperclip,
    Plus,
    RefreshCw,
    Search,
    SquareTerminal,
    X
  } from '@lucide/svelte'
  import { base64ToBytes } from '$lib/utils/base64'
  import { formatFileSize } from '$lib/utils/format'
  import type { ChatAttachment, Resource } from '$lib/anda/client/types'
  import type { DesktopClient } from './client.svelte'
  import type { WorkspaceFilePreview } from '../shared/workbench'
  import { shortcutLabel } from '../shared/shortcuts'
  import { folderName } from './chat-list'
  import type { Label } from './labels'
  import { tip } from './tooltip'
  import TerminalPanel from './TerminalPanel.svelte'
  import GitPanel from './GitPanel.svelte'
  import BrowserPanel from './BrowserPanel.svelte'

  let {
    client,
    t,
    tab,
    dark,
    branch,
    resources,
    sessionFiles,
    fileRequest,
    changeFocus,
    maximized = $bindable(false),
    onClose
  }: {
    client: DesktopClient
    t: (key: Label) => string
    tab: PanelTab
    dark: boolean
    branch: string
    resources: ChatAttachment[]
    sessionFiles: string[]
    fileRequest: FileRequest | null
    changeFocus: { id: number; path: string } | null
    maximized?: boolean
    onClose: () => void
  } = $props()

  const titles: Record<PanelTab, Label> = {
    resources: 'resources',
    changes: 'changes',
    terminal: 'terminal',
    browser: 'browser'
  }
  const icons = {
    resources: Paperclip,
    changes: GitCompare,
    terminal: SquareTerminal,
    browser: Globe
  }
  const Icon = $derived(icons[tab])
  const context = $derived(
    tab === 'changes'
      ? branch
      : tab === 'terminal' && client.workspace
        ? folderName(client.workspace)
        : ''
  )
  const keys = (action: Parameters<typeof shortcutLabel>[0]) =>
    shortcutLabel(action, client.platform)

  let terminal = $state<TerminalPanel | null>(null)
  let git = $state<GitPanel | null>(null)
  let browser = $state<BrowserPanel | null>(null)

  /** ⌘F: the open panel's find bar. */
  export function find() {
    if (tab === 'terminal') terminal?.toggleFind()
    else if (tab === 'browser') browser?.toggleFind()
  }

  // Resources: a chat's attachments, and workspace files opened from the transcript.
  let selectedResource = $state<Resource | null>(null)
  let selectedIndex = $state(-1)
  let previewText = $state('')
  let previewUrl = $state('')
  let previewError = $state('')
  let generation = 0
  let file = $state<{
    path: string
    line?: number
    preview?: WorkspaceFilePreview
    error?: string
  } | null>(null)
  let fileLines = $state<HTMLElement | null>(null)
  const lines = $derived(file?.preview?.text.split('\n') || [])

  function clearPreview() {
    generation++
    selectedResource = null
    selectedIndex = -1
    previewText = ''
    previewError = ''
    if (previewUrl) URL.revokeObjectURL(previewUrl)
    previewUrl = ''
  }
  $effect(() => {
    void client.activeSource
    untrack(() => {
      clearPreview()
      file = null
    })
  })
  $effect(() => () => {
    if (previewUrl) URL.revokeObjectURL(previewUrl)
  })

  async function showResource(attachment: ChatAttachment, index: number) {
    clearPreview()
    file = null
    const current = generation
    selectedIndex = index
    selectedResource = attachment.resource
    try {
      const resource = await client.loadResource(attachment.resource)
      if (current !== generation) return
      if (resource) selectedResource = resource
      if (!resource?.blob) {
        previewError = t('noDownload')
        return
      }
      const bytes = base64ToBytes(resource.blob)
      const mime = resource.mime_type || 'application/octet-stream'
      if (mime.startsWith('image/') || mime === 'application/pdf' || mime.startsWith('audio/'))
        previewUrl = URL.createObjectURL(new Blob([bytes], { type: mime }))
      else previewText = new TextDecoder().decode(bytes.slice(0, 512_000))
    } catch (error) {
      if (current === generation) previewError = String(error)
    }
  }

  let lastFileRequest = 0
  $effect(() => {
    const request = fileRequest
    if (!request || request.id === lastFileRequest) return
    lastFileRequest = request.id
    untrack(() => void openFile(request))
  })
  async function openFile(request: FileRequest) {
    clearPreview()
    const current = generation
    const workspace = client.workspace
    file = { path: request.path, line: request.line }
    if (!workspace) {
      file = { ...file, error: t('chooseProject') }
      return
    }
    try {
      const preview = await window.anda.workspaceFile<WorkspaceFilePreview>({
        action: 'read',
        workspace,
        path: request.path
      })
      if (current !== generation) return
      file = { path: preview.path, line: request.line, preview }
      await tick()
      fileLines?.querySelector('.file-line.target')?.scrollIntoView({ block: 'center' })
    } catch (error) {
      if (current === generation)
        file = { ...file, error: error instanceof Error ? error.message : String(error) }
    }
  }
  async function reveal(path?: string) {
    if (!client.workspace) return
    try {
      await window.anda.workspaceFile({ action: 'reveal', workspace: client.workspace, path })
    } catch (error) {
      client.fail(error)
    }
  }
</script>

<header class="panel-header">
  <span class="panel-title"
    ><Icon size={15} /><span>{t(titles[tab])}</span>{#if context}<small>{context}</small>{/if}</span
  >
  <div class="panel-actions">
    {#if tab === 'terminal' && client.workspace}
      <button
        class="icon-button"
        aria-label={t('find')}
        use:tip={{ text: t('find'), shortcut: keys('find') }}
        onclick={() => terminal?.toggleFind()}><Search size={15} /></button
      ><button
        class="icon-button"
        aria-label={t('newTerminal')}
        use:tip={t('newTerminal')}
        onclick={() => terminal?.create()}><Plus size={16} /></button
      >
    {:else if tab === 'browser'}
      <button
        class="icon-button"
        aria-label={t('find')}
        use:tip={{ text: t('find'), shortcut: keys('find') }}
        onclick={() => browser?.toggleFind()}><Search size={15} /></button
      ><button
        class="icon-button"
        aria-label={t('newTab')}
        use:tip={t('newTab')}
        onclick={() => browser?.newTab()}><Plus size={16} /></button
      >
    {:else if tab === 'changes' && client.workspace}
      <button
        class="icon-button"
        aria-label={t('refresh')}
        use:tip={t('refresh')}
        onclick={() => git?.refresh()}><RefreshCw size={14} /></button
      >
    {/if}
    <button
      class="icon-button"
      aria-label={maximized ? t('restorePanel') : t('maximizePanel')}
      aria-pressed={maximized}
      use:tip={maximized ? t('restorePanel') : t('maximizePanel')}
      onclick={() => (maximized = !maximized)}
      >{#if maximized}<Minimize2 size={15} />{:else}<Maximize2 size={15} />{/if}</button
    ><button class="icon-button" aria-label={t('close')} use:tip={t('close')} onclick={onClose}
      ><X size={16} /></button
    >
  </div>
</header>
{#if tab === 'browser'}
  {#key client.activeSource}<BrowserPanel
      bind:this={browser}
      source={client.activeSource}
      language={client.preferences.language}
    />{/key}
{:else if tab === 'terminal' || tab === 'changes'}
  {#if client.workspace}{#key client.workspace}
      {#if tab === 'terminal'}<TerminalPanel
          bind:this={terminal}
          workspace={client.workspace}
          language={client.preferences.language}
          {dark}
        />{:else}<GitPanel
          bind:this={git}
          {client}
          workspace={client.workspace}
          {sessionFiles}
          focus={changeFocus}
        />{/if}
    {/key}{:else}<p class="workbench-empty">
      {t('chooseProject')}
    </p>{/if}
{:else}
  <div class="resources-panel">
    {#if file}
      <section class="file-preview" aria-label={file.path}>
        <div class="file-preview-header">
          <code title={file.path}>{file.path}</code>
          {#if file.preview}<small
              >{formatFileSize(file.preview.size)}{file.preview.truncated
                ? ` · ${t('previewTruncated')}`
                : ''}</small
            >{/if}
          <button
            class="icon-button"
            aria-label={t('revealFile')}
            use:tip={t('revealFile')}
            onclick={() => void reveal(file?.path)}><FolderOpen size={15} /></button
          ><button
            class="icon-button"
            aria-label={t('close')}
            use:tip={t('close')}
            onclick={() => (file = null)}><X size={15} /></button
          >
        </div>
        {#if file.error}<p class="workbench-error" role="alert">{file.error}</p>
        {:else if file.preview?.binary}<p class="workbench-empty">{t('binaryFile')}</p>
        {:else if file.preview}<pre
            class="file-lines"
            bind:this={fileLines}>{#each lines as text, index (index)}<span
                class="file-line"
                class:target={index + 1 === file.line}
                data-line={index + 1}>{text}{'\n'}</span
              >{/each}</pre>
        {:else}<p class="workbench-empty">{t('loading')}</p>{/if}
      </section>
    {/if}
    {#if resources.length}<div class="resource-list">
        {#each resources as resource, index}<button
            class:selected={selectedIndex === index}
            onclick={() => void showResource(resource, index)}
            ><Paperclip size={14} /><span>{resource.name}</span></button
          >{/each}
      </div>{:else if !file}<div class="resource-empty">
        <Paperclip size={25} />
        <p>{t('noResources')}</p>
      </div>{/if}{#if selectedResource}<div class="resource-preview">
        <h3>{selectedResource.name}</h3>
        {#if previewError}<p>
            {previewError}
          </p>{:else if previewUrl && selectedResource.mime_type?.startsWith('image/')}<img
            src={previewUrl}
            alt={selectedResource.name}
          />{:else if previewUrl && selectedResource.mime_type === 'application/pdf'}<iframe
            title={selectedResource.name}
            src={previewUrl}
          ></iframe>{:else if previewUrl}<audio controls src={previewUrl}
          ></audio>{:else}<pre>{previewText}</pre>{/if}
      </div>{/if}
  </div>
{/if}
