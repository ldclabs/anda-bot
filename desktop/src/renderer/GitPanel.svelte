<script lang="ts">
  import { onMount } from 'svelte'
  import type { DesktopClient } from './client.svelte'
  import type { GitSnapshot, GitRequest } from '../shared/workbench'
  import { wb } from './workbench-labels'
  let { client, workspace }: { client: DesktopClient; workspace: string } = $props()
  const t = (key: Parameters<typeof wb>[1]) => wb(client.preferences.language, key)
  let snapshot = $state<GitSnapshot | null>(null)
  let error = $state('')
  let busy = $state(false)
  let diff = $state('')
  let selected = $state('')
  let message = $state('')
  let branch = $state('')
  let base = $state('HEAD')
  let tab = $state('changes')
  let generation = 0
  let disposed = false
  async function refresh() {
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
  async function preview(path: string, staged: boolean) {
    const selection = `${staged}:${path}`
    selected = selection
    try {
      const result = await window.anda.git<string>({ action: 'diff', workspace, path, staged })
      if (selected === selection) diff = result || t('empty')
    } catch (e) {
      error = String(e)
    }
  }
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
  <div class="workbench-toolbar">
    <strong>{snapshot?.branch || 'Git'}</strong><button disabled={busy} onclick={refresh}
      >{t('refresh')}</button
    >
  </div>
  <div class="workbench-tabs">
    {#each ['changes', 'history', 'worktrees'] as key}<button
        class:active={tab === key}
        onclick={() => (tab = key)}>{t(key as 'changes' | 'history' | 'worktrees')}</button
      >{/each}
  </div>
  {#if error}<p class="workbench-error" role="alert">{error}</p>{/if}
  {#if snapshot}
    {#if tab === 'changes'}
      {#each [true, false] as staged}
        <h3 class="workbench-section">{t(staged ? 'staged' : 'unstaged')}</h3>
        {#each snapshot.files.filter( (file) => (staged ? ![' ', '?'].includes(file.index) : file.worktree !== ' ') ) as file}
          <div class="git-file">
            <button
              class:selected={selected === `${staged}:${file.path}`}
              onclick={() => preview(file.path, staged)}
              ><code>{staged ? file.index : file.worktree}</code><span>{file.path}</span></button
            ><button
              disabled={busy}
              title={t(staged ? 'unstage' : 'stage')}
              onclick={() =>
                act({
                  action: staged ? 'unstage' : 'stage',
                  workspace,
                  paths: [file.path, ...(file.previousPath ? [file.previousPath] : [])],
                  revision: snapshot!.revision
                })}>{staged ? '−' : '+'}</button
            >
          </div>
        {/each}
      {/each}
      {#if !snapshot.files.length}<p class="workbench-empty">{t('empty')}</p>{/if}
      <div class="git-commit">
        <textarea aria-label={t('message')} placeholder={t('message')} bind:value={message} rows="2"
        ></textarea><button
          class="primary-button"
          disabled={busy ||
            !message.trim() ||
            !snapshot.files.some((f) => ![' ', '?'].includes(f.index))}
          onclick={() =>
            act({ action: 'commit', workspace, message, revision: snapshot!.revision })}
          >{t('commit')}</button
        >
      </div>
      {#if selected}<pre class="git-diff">{diff}</pre>{/if}
    {:else if tab === 'history'}
      <div class="git-history">
        {#each snapshot.log as entry}<p>
            <code>{entry.hash}</code><span>{entry.subject}</span>
          </p>{/each}
      </div>
    {:else}
      <div class="worktree-create">
        <input placeholder={t('branch')} aria-label={t('branch')} bind:value={branch} /><select
          aria-label="Base branch"
          bind:value={base}
          ><option value="HEAD">HEAD</option>{#each snapshot.branches as name}<option value={name}
              >{name}</option
            >{/each}</select
        ><button
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
