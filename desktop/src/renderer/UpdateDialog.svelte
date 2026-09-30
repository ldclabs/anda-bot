<script lang="ts">
  import { RefreshCw } from '@lucide/svelte'
  import type { UpdateStatus } from '../shared/contract'
  import { focusDialog } from './dialog'
  import { label, type Label } from './labels'

  let {
    status,
    language,
    onclose,
    oncheck
  }: {
    status: UpdateStatus | null
    language: string
    onclose: () => void
    oncheck: () => void
  } = $props()
  const t = (key: Label) => label(language, key)
  const running = $derived(!status || status.phase === 'running')
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
    <h2 id="update-title">{t('update')}</h2>
    <p class="update-heading" class:error={status?.phase === 'error'}>
      {#if running}<RefreshCw size={17} class="update-spinner" />{/if}
      {t(running ? 'checkingUpdates' : status?.phase === 'error' ? 'updateFailed' : 'updateResult')}
    </p>
    <p id="update-message" role="status" aria-live="polite">
      {status?.message || t('checkingUpdates')}
    </p>
    {#if running}<p class="update-hint">{t('updateBackground')}</p>{/if}
    <div class="dialog-actions">
      {#if !running}<button onclick={oncheck}>{t('update')}</button>{/if}
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
