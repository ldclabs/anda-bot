<script lang="ts">
  /**
   * Imports MCP servers from the other clients on this computer: Claude
   * Desktop, Claude Code, Cursor, VS Code, Windsurf and Codex. The daemon
   * reads their files (never changing them) and shows each server redacted;
   * importing names servers by key, so the daemon reads the files again and
   * writes what they say. Plaintext tokens move to secrets unless the owner
   * keeps them, and a secret a server needs can be set here.
   */
  import { useAndaClient } from '$lib/anda/client/context'
  import { IMPORT_SOURCE_LABELS } from '$lib/anda/client/mcp'
  import type { McpImportCandidate, McpImportScan, McpReceipt } from '$lib/anda/client/types'
  import Modal from '$lib/anda/Modal.svelte'
  import { badgeClass, buttonClass, inputClass } from '$lib/anda/ui'
  import { getMessage } from '$lib/i18n'
  import { errorToMessage } from '$lib/service-worker/settings'
  import { cn } from '$lib/utils'
  import { AlertTriangle, LoaderCircle } from '@lucide/svelte'
  import { untrack } from 'svelte'

  let {
    open = $bindable(false),
    revision = '',
    onImported
  }: {
    open?: boolean
    /** The mcp.json revision the page shows: an import is refused when the file changed since. */
    revision?: string
    onImported: (receipt: McpReceipt) => void
  } = $props()

  const mcp = useAndaClient().mcp

  let scan = $state<McpImportScan | null>(null)
  let loading = $state(false)
  let busy = $state(false)
  let error = $state('')
  let picked = $state<Record<string, boolean>>({})
  let ids = $state<Record<string, string>>({})
  let secretValues = $state<Record<string, string>>({})
  let storeSecrets = $state(true)

  // Scans each time the dialog opens; the scan writes state this reads.
  $effect(() => {
    if (open) untrack(() => void load())
  })

  const groups = $derived(
    (scan?.files || []).map((file) => ({
      file,
      candidates: (scan?.candidates || []).filter((candidate) => candidate.path === file.path)
    }))
  )
  const chosen = $derived((scan?.candidates || []).filter((candidate) => picked[candidate.key]))
  const neededSecrets = $derived.by(() => {
    const needed = new Map<string, string>()
    for (const candidate of chosen) {
      for (const secret of candidate.needs_secrets || [])
        needed.set(secret.name, secret.description)
    }
    return [...needed].map(([name, description]) => ({ name, description }))
  })

  async function load() {
    loading = true
    error = ''
    scan = null
    try {
      const next = await mcp.importScan()
      scan = next
      picked = Object.fromEntries(
        next.candidates.map((candidate) => [
          candidate.key,
          candidate.status === 'new' || candidate.status === 'renamed'
        ])
      )
      ids = Object.fromEntries(next.candidates.map((candidate) => [candidate.key, candidate.id]))
      secretValues = {}
    } catch (err) {
      error = errorToMessage(err)
    } finally {
      loading = false
    }
  }

  function selectable(candidate: McpImportCandidate): boolean {
    return candidate.status !== 'invalid' && candidate.status !== 'exists'
  }

  async function runImport() {
    if (!chosen.length || busy) return
    busy = true
    error = ''
    try {
      const needed = new Set(neededSecrets.map((secret) => secret.name))
      const secrets = Object.fromEntries(
        Object.entries(secretValues).filter(([name, value]) => needed.has(name) && value.trim())
      )
      const receipt = await mcp.import({
        items: chosen.map((candidate) => {
          const id = (ids[candidate.key] || '').trim()
          return id && id !== candidate.id ? { key: candidate.key, id } : { key: candidate.key }
        }),
        secrets,
        store_secrets: storeSecrets,
        ...(revision ? { expected_revision: revision } : {})
      })
      open = false
      onImported(receipt)
    } catch (err) {
      error = errorToMessage(err)
    } finally {
      busy = false
    }
  }

  function statusLabel(candidate: McpImportCandidate): string {
    switch (candidate.status) {
      case 'new':
        return getMessage('mcpImportNew')
      case 'renamed':
        return getMessage('mcpImportRenamed', candidate.id)
      case 'exists':
        return getMessage('mcpImportExists', candidate.existing_id || candidate.name)
      case 'duplicate':
        return candidate.existing_id
          ? getMessage('mcpImportDuplicateOf', candidate.existing_id)
          : getMessage('mcpImportDuplicate')
      default:
        return getMessage('mcpImportInvalid')
    }
  }

  function statusTone(candidate: McpImportCandidate): string {
    switch (candidate.status) {
      case 'new':
        return 'border-emerald-500/50 text-emerald-700 dark:text-emerald-300'
      case 'renamed':
        return 'border-sky-500/50 text-sky-700 dark:text-sky-300'
      case 'invalid':
        return 'border-destructive/50 text-destructive'
      default:
        return 'text-muted-foreground'
    }
  }
</script>

{#snippet actions()}
  <label class="me-auto flex items-center gap-2 text-xs">
    <input type="checkbox" bind:checked={storeSecrets} />
    <span>{getMessage('mcpStoreAsSecrets')}</span>
  </label>
  <button type="button" class={buttonClass('outline', 'sm')} onclick={() => (open = false)}>
    {getMessage('cancel')}
  </button>
  <button
    type="button"
    class={buttonClass('default', 'sm')}
    disabled={busy || loading || !chosen.length}
    onclick={runImport}
  >
    {#if busy}
      <LoaderCircle class="size-3.5 animate-spin" />
    {/if}
    {getMessage('mcpImportAction', String(chosen.length))}
  </button>
{/snippet}

<Modal
  bind:open
  title={getMessage('mcpImportTitle')}
  description={getMessage('mcpImportDescription')}
  contentClass="sm:max-w-3xl"
  footer={actions}
>
  <div class="grid gap-4">
    {#if loading}
      <div class="grid h-32 place-items-center text-muted-foreground">
        <LoaderCircle class="size-5 animate-spin" />
      </div>
    {:else if scan && !scan.files.length}
      <p class="rounded-md border border-dashed p-6 text-center text-sm text-muted-foreground">
        {getMessage('mcpImportNone')}
      </p>
    {/if}

    {#each groups as group (group.file.path)}
      <section class="grid gap-2">
        <div class="flex min-w-0 items-baseline gap-2">
          <span class="shrink-0 text-sm font-semibold">
            {IMPORT_SOURCE_LABELS[group.file.source] || group.file.source}
          </span>
          <span
            class="truncate font-mono text-[11px] text-muted-foreground"
            title={group.file.path}
          >
            {group.file.path}
          </span>
        </div>
        {#if group.file.error}
          <p class="text-xs text-destructive">
            {getMessage('mcpImportUnreadable', group.file.error)}
          </p>
        {/if}
        {#each group.candidates as candidate (candidate.key)}
          <div
            class={cn(
              'grid gap-1.5 rounded-md border p-3',
              picked[candidate.key] ? 'border-primary/30 bg-background' : 'bg-muted/20'
            )}
          >
            <div class="flex min-w-0 flex-wrap items-center gap-2">
              <input
                type="checkbox"
                aria-label={getMessage('mcpImportPick', candidate.name)}
                disabled={!selectable(candidate) || busy}
                bind:checked={picked[candidate.key]}
              />
              <span class="font-mono text-sm font-semibold">{candidate.name}</span>
              <span class={badgeClass('outline', 'h-4 px-1.5 text-[10px]')}>
                {candidate.transport === 'stdio' ? getMessage('mcpLocal') : getMessage('mcpRemote')}
              </span>
              {#if !candidate.enabled}
                <span class={badgeClass('outline', 'h-4 px-1.5 text-[10px]')}>
                  {getMessage('mcpStatusDisabled')}
                </span>
              {/if}
              <span
                class={badgeClass('outline', cn('h-4 px-1.5 text-[10px]', statusTone(candidate)))}
              >
                {statusLabel(candidate)}
              </span>
              {#if candidate.project}
                <span class="truncate text-[11px] text-muted-foreground" title={candidate.project}>
                  {getMessage('mcpImportProject', candidate.project)}
                </span>
              {/if}
            </div>
            {#if candidate.summary}
              <code
                class="truncate rounded bg-muted/50 px-2 py-1 font-mono text-xs"
                title={candidate.summary}>{candidate.summary}</code
              >
            {/if}
            {#if candidate.error}
              <p class="text-xs break-words text-destructive">{candidate.error}</p>
            {/if}
            {#if picked[candidate.key]}
              <label class="flex items-center gap-2 text-xs">
                <span class="shrink-0 text-muted-foreground">{getMessage('mcpImportAs')}</span>
                <input
                  class={inputClass('h-7 max-w-64 font-mono text-xs')}
                  bind:value={ids[candidate.key]}
                  aria-label={getMessage('mcpImportAs')}
                />
              </label>
              {#if storeSecrets && candidate.plaintext?.length}
                <p class="text-[11px] text-muted-foreground">
                  {getMessage('mcpImportPlaintext', candidate.plaintext.join(', '))}
                </p>
              {/if}
            {/if}
            {#each candidate.warnings || [] as warning}
              <p class="flex items-start gap-1.5 text-[11px] text-amber-700 dark:text-amber-300">
                <AlertTriangle class="mt-0.5 size-3 shrink-0" />
                <span>{warning}</span>
              </p>
            {/each}
          </div>
        {/each}
      </section>
    {/each}

    {#if neededSecrets.length}
      <section class="grid gap-2 rounded-md border bg-muted/20 p-3">
        <div class="text-xs font-semibold">{getMessage('mcpImportSecrets')}</div>
        {#each neededSecrets as secret (secret.name)}
          <label class="grid gap-1 text-xs">
            <span>
              <span class="font-mono font-semibold">{secret.name}</span>
              <span class="text-muted-foreground"> · {secret.description}</span>
            </span>
            <input
              class={inputClass('h-8 text-xs')}
              type="password"
              autocomplete="off"
              placeholder={getMessage('mcpImportSecretLater')}
              aria-label={getMessage('mcpSecretValueFor', secret.name)}
              bind:value={secretValues[secret.name]}
            />
          </label>
        {/each}
      </section>
    {/if}

    {#if scan?.candidates.some((candidate) => picked[candidate.key] && candidate.transport === 'stdio')}
      <p class="text-[11px] text-muted-foreground">{getMessage('mcpImportIsolated')}</p>
    {/if}
    {#if error}
      <p class="text-sm whitespace-pre-wrap text-destructive" role="alert">{error}</p>
    {/if}
  </div>
</Modal>
