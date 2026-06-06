<script lang="ts">
  import type { ChatAttachment } from '$lib/anda/client'
  import { fileSizeLabel } from '$lib/anda/composer/attachments'
  import { badgeClass, buttonClass, cardClass } from '$lib/anda/ui'
  import { FileText, X } from '@lucide/svelte'

  let {
    attachments,
    onRemove
  }: {
    attachments: ChatAttachment[]
    onRemove: (id: string) => void
  } = $props()
</script>

{#if attachments.length}
  <div class="mb-2 flex flex-wrap gap-1.5 px-1">
    {#each attachments as attachment (attachment.id)}
      {#if attachment.type?.startsWith('image/')}
        <div
          class={cardClass(
            'group relative size-8 shrink-0 gap-0 rounded-md bg-muted/50 p-0 shadow-sm transition-all hover:border-emerald-500/50'
          )}
        >
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
            aria-label={chrome.i18n.getMessage('removeAttachment')}
            onclick={() => onRemove(attachment.id)}
          >
            <X class="size-2" />
          </button>
        </div>
      {:else}
        <span
          class={badgeClass(
            'outline',
            'max-w-full rounded-md bg-muted/50 py-1 text-xs font-normal text-muted-foreground'
          )}
          title={attachment.name}
        >
          <FileText class="size-3 shrink-0 text-emerald-700" />
          <span class="max-w-30 truncate">{attachment.name}</span>
          <span class="shrink-0 text-muted-foreground/70"
            >{fileSizeLabel(attachment.size || 0)}</span
          >
          <button
            type="button"
            class={buttonClass(
              'ghost',
              'icon-xs',
              'size-4 rounded-sm text-muted-foreground/80 hover:text-foreground'
            )}
            aria-label={chrome.i18n.getMessage('removeAttachment')}
            title={chrome.i18n.getMessage('removeAttachment')}
            onclick={() => onRemove(attachment.id)}
          >
            <X class="size-3" />
          </button>
        </span>
      {/if}
    {/each}
  </div>
{/if}
