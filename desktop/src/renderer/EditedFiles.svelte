<script lang="ts">
  /**
   * The files a turn edited, under its last message: each with the lines
   * the edits added and removed. A file opens its diff in Changes when the
   * workspace is a Git repository, its content otherwise.
   */
  import { FileDiff, FilePen } from '@lucide/svelte'
  import type { EditedFile } from './transcript'
  import { workspaceRelative } from './transcript'
  import type { Label } from './labels'

  let {
    files,
    workspace,
    t,
    onOpen
  }: {
    files: EditedFile[]
    workspace: string | undefined
    t: (key: Label, values?: Record<string, string>) => string
    onOpen: (path: string) => void
  } = $props()

  const additions = $derived(files.reduce((sum, file) => sum + file.additions, 0))
  const deletions = $derived(files.reduce((sum, file) => sum + file.deletions, 0))
</script>

{#snippet stats(added: number, removed: number)}
  <small class="line-stats"
    >{#if added}<ins>+{added}</ins>{/if}{#if removed}<del>−{removed}</del>{/if}</small
  >
{/snippet}

<section class="edited-files" aria-label={t('editedFiles', { count: String(files.length) })}>
  <header>
    <FilePen size={14} />
    <span
      >{files.length === 1
        ? t('editedFile')
        : t('editedFiles', { count: String(files.length) })}</span
    >
    {@render stats(additions, deletions)}
  </header>
  {#each files as file (file.path)}
    {@const path = workspaceRelative(file.path, workspace)}
    <button onclick={() => onOpen(path)} title={path}>
      <FileDiff size={14} />
      <span>{path}</span>
      {@render stats(file.additions, file.deletions)}
    </button>
  {/each}
</section>
