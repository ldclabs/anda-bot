<script lang="ts">
  import type { ChatAttachment } from '$lib/anda/client'
  import { fileSizeLabel } from '$lib/anda/composer/attachments'
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
          class="group relative size-8 shrink-0 overflow-hidden rounded-md border border-stone-200 bg-stone-50 shadow-sm transition-all hover:border-emerald-500/50"
        >
          <img
            src={`data:${attachment.type};base64,${attachment.resource.blob}`}
            alt={attachment.name}
            class="size-full object-cover"
          />
          <button
            type="button"
            class="absolute top-0 right-0 grid size-3.5 place-items-center rounded-bl-md bg-black/50 text-white opacity-0 transition-opacity group-hover:opacity-100 hover:bg-red-500"
            aria-label={chrome.i18n.getMessage('removeAttachment')}
            onclick={() => onRemove(attachment.id)}
          >
            <X class="size-2" />
          </button>
        </div>
      {:else}
        <span
          class="inline-flex max-w-full items-center gap-1.5 rounded-md border border-stone-200 bg-stone-50 px-2 py-1 text-[11px] text-stone-600"
          title={attachment.name}
        >
          <FileText class="size-3 shrink-0 text-emerald-700" />
          <span class="max-w-30 truncate">{attachment.name}</span>
          <span class="shrink-0 text-stone-400">{fileSizeLabel(attachment.size || 0)}</span>
          <button
            type="button"
            class="grid size-4 shrink-0 place-items-center rounded-sm text-stone-400 hover:bg-stone-200 hover:text-stone-700"
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
