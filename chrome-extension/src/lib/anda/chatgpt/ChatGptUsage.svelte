<script lang="ts">
  import { getMessage } from '$lib/i18n'
  import { openChatGptUrl, usageUrl } from './api'
  let { model }: { model?: string | null } = $props()
</script>

{#if model?.startsWith('chatgpt:')}
  <span class="flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
    <span>{getMessage('chatgptUsingPlan')}</span>
    <button
      type="button"
      class="underline underline-offset-2"
      onclick={() => void openChatGptUrl(usageUrl)}>{getMessage('chatgptManageUsage')}</button
    >
  </span>
{/if}
