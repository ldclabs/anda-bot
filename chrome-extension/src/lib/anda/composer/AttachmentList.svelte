<script lang="ts">
  import { getMessage } from '$lib/i18n'
  import type { ChatAttachment } from '$lib/anda/client'
  import { fileSizeLabel } from '$lib/anda/composer/attachments'
  import { buttonClass } from '$lib/anda/ui'
  import { FileText, Paperclip, X } from '@lucide/svelte'

  let {
    attachments,
    onRemove
  }: {
    attachments: ChatAttachment[]
    onRemove: (id: string) => void
  } = $props()
</script>

{#if attachments.length}
  <div class="attachment-tray" aria-label={getMessage('attachFiles')}>
    <div class="attachment-tray-count" aria-hidden="true">
      <Paperclip class="size-3" />
      <span>{attachments.length}</span>
    </div>
    <div class="attachment-items">
      {#each attachments as attachment (attachment.id)}
        {#if attachment.type?.startsWith('image/')}
          <div class="attachment-image group">
            <img
              src={`data:${attachment.type};base64,${attachment.resource.blob}`}
              alt={attachment.name}
              class="size-full object-cover"
            />
            <button
              type="button"
              class={buttonClass(
                'destructive',
                'icon-xs',
                'absolute top-0 right-0 size-4 rounded-none rounded-bl-md bg-black/50 p-0 text-white opacity-0 transition-opacity group-hover:opacity-100 hover:bg-red-500'
              )}
              aria-label={getMessage('removeAttachment')}
              onclick={() => onRemove(attachment.id)}
            >
              <X class="size-2" />
            </button>
          </div>
        {:else}
          <span class="attachment-pill" title={attachment.name}>
            <span class="attachment-icon">
              <FileText class="size-3.5" />
            </span>
            <span class="attachment-body">
              <span class="attachment-name">{attachment.name}</span>
              <span class="attachment-meta">{fileSizeLabel(attachment.size || 0)}</span>
            </span>
            <button
              type="button"
              class={buttonClass(
                'ghost',
                'icon-xs',
                'attachment-remove text-muted-foreground/80 hover:text-foreground'
              )}
              aria-label={getMessage('removeAttachment')}
              title={getMessage('removeAttachment')}
              onclick={() => onRemove(attachment.id)}
            >
              <X class="size-3" />
            </button>
          </span>
        {/if}
      {/each}
    </div>
  </div>
{/if}

<style>
  .attachment-tray {
    display: flex;
    min-width: 0;
    align-items: flex-start;
    gap: 0.5rem;
    border: 1px solid color-mix(in srgb, var(--message-border, #e6e6e6) 72%, #059669);
    border-radius: 0.5rem;
    background: color-mix(in srgb, var(--message-bg, #ffffff) 86%, #ecfdf5);
    padding: 0.375rem 0.5rem;
    box-shadow: inset 0 1px 0 color-mix(in srgb, #ffffff 80%, transparent);
  }

  .attachment-tray-count {
    display: inline-flex;
    height: 1.5rem;
    min-width: 1.75rem;
    flex: 0 0 auto;
    align-items: center;
    justify-content: center;
    gap: 0.125rem;
    border-radius: 999px;
    background: color-mix(in srgb, #10b981 14%, var(--message-bg, #ffffff));
    color: #047857;
    font-size: 0.6875rem;
    font-weight: 700;
    line-height: 1;
  }

  .attachment-items {
    display: flex;
    min-width: 0;
    flex: 1;
    flex-wrap: wrap;
    gap: 0.375rem;
  }

  .attachment-image {
    position: relative;
    width: 2rem;
    height: 2rem;
    flex: 0 0 auto;
    overflow: hidden;
    border: 1px solid color-mix(in srgb, var(--message-border, #e6e6e6) 72%, #10b981);
    border-radius: 0.375rem;
    background: color-mix(in srgb, var(--message-bg, #ffffff) 70%, #ecfdf5);
    box-shadow: 0 1px 2px rgba(0, 0, 0, 0.08);
  }

  .attachment-pill {
    display: grid;
    max-width: min(13.5rem, 100%);
    min-width: 8rem;
    grid-template-columns: auto minmax(0, 1fr) auto;
    align-items: center;
    gap: 0.375rem;
    border: 1px solid color-mix(in srgb, var(--message-border, #e6e6e6) 78%, #10b981);
    border-radius: 0.375rem;
    background: color-mix(in srgb, var(--message-bg, #ffffff) 92%, #ecfdf5);
    padding: 0.25rem 0.25rem 0.25rem 0.375rem;
    color: var(--message-text, #171717);
  }

  .attachment-icon {
    display: grid;
    width: 1.375rem;
    height: 1.375rem;
    flex: 0 0 auto;
    place-items: center;
    border-radius: 0.3125rem;
    background: color-mix(in srgb, #10b981 12%, transparent);
    color: #047857;
  }

  .attachment-body {
    display: grid;
    min-width: 0;
    gap: 0.0625rem;
  }

  .attachment-name,
  .attachment-meta {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .attachment-name {
    font-size: 0.75rem;
    font-weight: 650;
    line-height: 0.95rem;
  }

  .attachment-meta {
    color: var(--message-muted, #737373);
    font-size: 0.625rem;
    font-weight: 600;
    line-height: 0.75rem;
  }

  :global(.attachment-remove) {
    width: 1.25rem;
    height: 1.25rem;
    flex: 0 0 auto;
    border-radius: 0.25rem;
  }

  @media (max-width: 420px) {
    .attachment-tray {
      gap: 0.375rem;
      padding-inline: 0.375rem;
    }

    .attachment-pill {
      min-width: 0;
      max-width: 100%;
    }
  }
</style>
