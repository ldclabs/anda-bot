<script lang="ts">
  import { RefreshCw } from '@lucide/svelte'
  import type { UpdateOffer, UpdateStatus } from '../shared/contract'
  import { focusDialog } from './dialog'
  import { label, updateOperationLabels, type Label } from './labels'

  let {
    status,
    offer,
    language,
    onclose,
    oncheck,
    oncontinue
  }: {
    status: UpdateStatus | null
    offer: UpdateOffer | null
    language: string
    onclose: () => void
    oncheck: () => void
    oncontinue: () => void
  } = $props()
  const t = (key: Label) => label(language, key)
  const running = $derived(!status || status.phase === 'running')
  const labels = $derived(updateOperationLabels[status?.operation ?? 'check'])
  // A download or restart that did not go through repeats the status bar's step.
  const retry = $derived(status?.operation !== 'check' ? offer : null)
</script>

<div class="modal-backdrop">
  <div
    class="rename-dialog update-dialog"
    use:focusDialog={onclose}
    role="dialog"
    aria-modal="true"
    aria-labelledby="update-title"
    aria-describedby="update-message"
    tabindex="-1"
  >
    <h2 id="update-title">{t(labels.title)}</h2>
    <p class="update-heading" class:error={status?.phase === 'error'}>
      {#if running}<RefreshCw size={17} class="update-spinner" />{/if}
      {t(running ? labels.progress : status?.phase === 'error' ? 'updateFailed' : 'updateResult')}
    </p>
    <p id="update-message" role="status" aria-live="polite">
      {status?.message || t(labels.progress)}
    </p>
    {#if running}<p class="update-hint">{t('updateBackground')}</p>{/if}
    <div class="dialog-actions">
      {#if !running}
        {#if retry}<button onclick={oncontinue}
            >{t(retry.ready ? 'restartToUpdate' : 'downloadUpdate')}</button
          >{:else}<button onclick={oncheck}>{t('update')}</button>{/if}
      {/if}
      <button class="primary" onclick={onclose}>{t('close')}</button>
    </div>
  </div>
</div>

<style>
  .modal-backdrop {
    align-items: center;
    padding: 16px;
  }
  .update-dialog {
    width: min(480px, calc(100vw - 32px));
    max-height: 80vh;
    overflow: auto;
  }
  .update-heading {
    display: flex;
    align-items: center;
    gap: 9px;
    font-weight: 550;
  }
  .update-heading.error {
    color: #bb6250;
  }
  .dialog-actions .primary {
    background: var(--desk-text);
    color: var(--desk-bg);
    border-color: var(--desk-text);
  }
  #update-message {
    margin-top: 12px;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  .update-hint {
    margin-top: 12px;
    color: var(--desk-muted);
    font-size: 12px;
  }
  .update-dialog :global(.update-spinner) {
    animation: update-spin 1s linear infinite;
  }
  @keyframes update-spin {
    to {
      transform: rotate(360deg);
    }
  }
</style>
