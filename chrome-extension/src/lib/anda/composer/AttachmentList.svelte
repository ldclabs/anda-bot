<script lang="ts">
  import type { ChatAttachment } from '$lib/anda/client'
  import { fileSizeLabel } from '$lib/anda/composer/attachments'
  import { Badge } from '$lib/components/ui/badge/index.js'
  import { Button } from '$lib/components/ui/button/index.js'
  import { Card } from '$lib/components/ui/card/index.js'
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
        <Card
          class="group relative size-8 shrink-0 gap-0 rounded-md bg-muted/50 p-0 shadow-sm transition-all hover:border-emerald-500/50"
        >
          <img
            src={`data:${attachment.type};base64,${attachment.resource.blob}`}
            alt={attachment.name}
            class="size-full object-cover"
          />
          <Button
            variant="destructive"
            size="icon-xs"
            class="absolute top-0 right-0 size-4 rounded-none rounded-bl-md bg-black/50 p-0 text-white opacity-0 transition-opacity group-hover:opacity-100 hover:bg-red-500"
            aria-label={chrome.i18n.getMessage('removeAttachment')}
            onclick={() => onRemove(attachment.id)}
          >
            <X class="size-2" />
          </Button>
        </Card>
      {:else}
        <Badge
          variant="outline"
          class="max-w-full rounded-md bg-muted/50 py-1 text-[11px] font-normal text-muted-foreground"
          title={attachment.name}
        >
          <FileText class="size-3 shrink-0 text-emerald-700" />
          <span class="max-w-30 truncate">{attachment.name}</span>
          <span class="shrink-0 text-muted-foreground/70"
            >{fileSizeLabel(attachment.size || 0)}</span
          >
          <Button
            variant="ghost"
            size="icon-xs"
            class="size-4 rounded-sm text-muted-foreground/80 hover:text-foreground"
            aria-label={chrome.i18n.getMessage('removeAttachment')}
            title={chrome.i18n.getMessage('removeAttachment')}
            onclick={() => onRemove(attachment.id)}
          >
            <X class="size-3" />
          </Button>
        </Badge>
      {/if}
    {/each}
  </div>
{/if}
