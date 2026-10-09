<script lang="ts">
  /**
   * A long chat's table of contents: one mark per prompt along the
   * transcript's edge. The mark for the prompt in view is highlighted;
   * hovering one names it and clicking scrolls to it.
   */
  import { tip } from './tooltip'
  import type { Label } from './labels'

  let {
    prompts,
    active,
    t,
    onJump
  }: {
    prompts: Array<{ id: string; text: string }>
    active: string
    t: (key: Label) => string
    onJump: (id: string) => void
  } = $props()
</script>

<nav class="turn-index" aria-label={t('turnIndex')}>
  {#each prompts as prompt (prompt.id)}
    <button
      class:active={prompt.id === active}
      aria-current={prompt.id === active || undefined}
      aria-label={prompt.text}
      use:tip={prompt.text}
      onclick={() => onJump(prompt.id)}><span></span></button
    >
  {/each}
</nav>
