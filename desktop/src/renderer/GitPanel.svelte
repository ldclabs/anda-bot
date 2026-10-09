<script lang="ts">
  import { onMount, untrack } from 'svelte'
  import { Minus, Plus } from '@lucide/svelte'
  import type { DesktopClient } from './client.svelte'
  import type { GitFile, GitSnapshot, GitRequest } from '../shared/workbench'
  import { label, type Label } from './labels'
  import { diffLineKind } from './presentation'
  import { tip } from './tooltip'
  import DropdownMenu from '$lib/anda/DropdownMenu.svelte'
  let {
    client,
    workspace,
    sessionFiles = [],
    focus = null
  }: {
    client: DesktopClient
    workspace: string
    /** Workspace-relative paths this chat's agent edited. */
    sessionFiles?: string[]
    /** A file the transcript asked to show. */
    focus?: { id: number; path: string } | null
  } = $props()
  const t = (key: Label) => label(client.preferences.language, key)
  let snapshot = $state<GitSnapshot | null>(null)
  let error = $state('')
  let busy = $state(false)
  let diff = $state('')
  let diffUntracked = $state(false)
  let selected = $state('')
  let scope = $state<'all' | 'chat'>('all')
  const chatPaths = $derived(new Set(sessionFiles))
  const files = $derived(
    (snapshot?.files || []).filter((file) => scope === 'all' || chatPaths.has(file.path))
  )
  const stagedFiles = $derived(files.filter((file) => ![' ', '?'].includes(file.index)))
  const unstagedFiles = $derived(files.filter((file) => file.worktree !== ' '))
  const diffLines = $derived(diff.split('\n'))
  let message = $state('')
  let branch = $state('')
  let base = $state('HEAD')
  const tabs = ['changes', 'gitHistory', 'worktrees'] as const
  let tab = $state<(typeof tabs)[number]>('changes')
  let generation = 0
  let disposed = false
  export async function refresh() {
    const id = ++generation
    try {
      const result = await window.anda.git<GitSnapshot>({ action: 'status', workspace })
      if (!disposed && id === generation) {
        snapshot = result
        error = ''
      }
    } catch (e) {
      if (!disposed && id === generation) error = String(e)
    }
  }
  async function act(request: GitRequest) {
    busy = true
    error = ''
    try {
      const result = await window.anda.git<{ path?: string; name?: string }>(request)
      if (request.action === 'commit') message = ''
      if (result?.path) {
        await client.rpc('register_workspace', [result.path])
        await client.savePreferences({
          projects: [
            ...client.preferences.projects.filter((p) => p.path !== result.path),
            {
              id: crypto.randomUUID(),
              path: result.path,
              name: result.name || branch || result.path.split(/[\\/]/).at(-1) || 'Worktree'
            }
          ]
        })
      }
      if (request.action === 'worktree-archive')
        await client.savePreferences({
          projects: client.preferences.projects.filter((p) => p.path !== request.path)
        })
      await refresh()
      diff = ''
      selected = ''
    } catch (e) {
      error = String(e)
    } finally {
      busy = false
    }
  }
  async function preview(file: GitFile, staged: boolean) {
    const selection = `${staged}:${file.path}`
    selected = selection
    try {
      const result = await window.anda.git<string>({
        action: 'diff',
        workspace,
        path: file.path,
        staged
      })
      if (selected === selection) {
        diff = result || t('empty')
        // An untracked file's preview is its content, which is all new.
        diffUntracked = file.index === '?'
      }
    } catch (e) {
      error = String(e)
    }
  }
  function stats(path: string) {
    const stat = snapshot?.stats[path]
    return stat && stat.additions !== null ? stat : null
  }
  let focused = 0
  $effect(() => {
    const request = focus
    const current = snapshot
    if (!request || !current || request.id === focused) return
    focused = request.id
    untrack(() => {
      tab = 'changes'
      const file = current.files.find((f) => f.path === request.path)
      if (!file) return
      if (!chatPaths.has(file.path)) scope = 'all'
      void preview(file, file.worktree === ' ')
    })
  })
  onMount(() => {
    void refresh()
    const timer = setInterval(() => {
      if (!busy && !document.hidden) void refresh()
    }, 5000)
    return () => {
      disposed = true
      clearInterval(timer)
    }
  })
</script>

<div class="git-panel">
  <div class="workbench-tabs">
    {#each tabs as key (key)}<button class:active={tab === key} onclick={() => (tab = key)}
        >{t(key)}</button
      >{/each}
  </div>
  {#if error}<p class="workbench-error" role="alert">{error}</p>{/if}
  {#if snapshot}
    {#if tab === 'changes'}
      {#if sessionFiles.length}<div class="segmented" role="group" aria-label={t('changes')}>
          <button aria-pressed={scope === 'chat'} onclick={() => (scope = 'chat')}
            >{t('thisChat')}</button
          ><button aria-pressed={scope === 'all'} onclick={() => (scope = 'all')}
            >{t('allChanges')}</button
          >
        </div>{/if}
      {#each [true, false] as staged (staged)}
        {@const list = staged ? stagedFiles : unstagedFiles}
        {#if list.length}
          <h3 class="workbench-section">{t(staged ? 'staged' : 'unstaged')}</h3>
          {#each list as file (file.path)}
            {@const stat = stats(file.path)}
            <div class="git-file">
              <button
                class:selected={selected === `${staged}:${file.path}`}
                onclick={() => preview(file, staged)}
                ><code data-status={staged ? file.index : file.worktree}
                  >{staged ? file.index : file.worktree}</code
                ><span title={file.path}>{file.path}</span>{#if stat}<small class="line-stats"
                    ><ins>+{stat.additions}</ins><del>−{stat.deletions}</del></small
                  >{/if}</button
              ><button
                class="icon-button"
                disabled={busy}
                aria-label={t(staged ? 'unstage' : 'stage')}
                use:tip={t(staged ? 'unstage' : 'stage')}
                onclick={() =>
                  act({
                    action: staged ? 'unstage' : 'stage',
                    workspace,
                    paths: [file.path, ...(file.previousPath ? [file.previousPath] : [])],
                    revision: snapshot!.revision
                  })}
                >{#if staged}<Minus size={13} />{:else}<Plus size={13} />{/if}</button
              >
            </div>
          {/each}
        {/if}
      {/each}
      {#if !files.length}<p class="workbench-empty">{t('empty')}</p>{/if}
      <div class="git-commit">
        <textarea aria-label={t('message')} placeholder={t('message')} bind:value={message} rows="2"
        ></textarea><button
          class="primary"
          disabled={busy ||
            !message.trim() ||
            !snapshot.files.some((f) => ![' ', '?'].includes(f.index))}
          onclick={() =>
            act({ action: 'commit', workspace, message, revision: snapshot!.revision })}
          >{t('commit')}</button
        >
      </div>
      {#if selected}<pre class="git-diff">{#each diffLines as line, index (index)}<span
              class="diff-line"
              data-kind={diffUntracked ? 'add' : diffLineKind(line)}>{line}{'\n'}</span
            >{/each}</pre>{/if}
    {:else if tab === 'gitHistory'}
      <div class="git-history">
        {#each snapshot.log as entry}<p>
            <code>{entry.hash}</code><span>{entry.subject}</span>
          </p>{/each}
      </div>
    {:else}
      <div class="worktree-create">
        <input placeholder={t('branch')} aria-label={t('branch')} bind:value={branch} />
        <DropdownMenu
          items={['HEAD', ...snapshot.branches].map((name) => ({ value: name, label: name }))}
          bind:value={base}
          ariaLabel="Base branch"
          searchable
          searchPlaceholder={t('find')}
        /><button
          disabled={busy || !branch.trim() || !snapshot.head}
          onclick={() =>
            act({
              action: 'worktree-create',
              workspace,
              branch,
              base,
              revision: snapshot!.revision
            })}>{t('createWorktree')}</button
        >
      </div>
      {#each snapshot.worktrees as tree}<div class="worktree-item">
          <strong>{tree.branch || tree.head.slice(0, 8)}</strong><small>{tree.path}</small
          >{#if tree.path !== snapshot.root}<button
              disabled={busy}
              onclick={() =>
                act({
                  action: 'worktree-archive',
                  workspace,
                  path: tree.path,
                  revision: snapshot!.revision
                })}>{t('archive')}</button
            >{/if}
        </div>{/each}
      {#each snapshot.archives as archive}<div class="worktree-item">
          <strong>{archive.name}</strong><small>{new Date(archive.time).toLocaleString()}</small
          ><button
            disabled={busy}
            onclick={() =>
              act({
                action: 'worktree-restore',
                workspace,
                id: archive.id,
                revision: snapshot!.revision
              })}>{t('restore')}</button
          >
        </div>{/each}
    {/if}
  {/if}
</div>
