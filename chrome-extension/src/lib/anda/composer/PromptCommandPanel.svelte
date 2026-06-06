<script lang="ts">
  import type { PromptCommandSuggestion } from '$lib/anda/composer/prompt-commands'
  import {
    badgeClass,
    buttonClass,
    cardClass,
    cardContentClass,
    cardHeaderClass,
    cardTitleClass
  } from '$lib/anda/ui'
  import { tick } from 'svelte'

  let {
    title,
    suggestions,
    activeIndex,
    onApply
  }: {
    title: string
    suggestions: PromptCommandSuggestion[]
    activeIndex: number
    onApply: (suggestion: PromptCommandSuggestion) => void | Promise<void>
  } = $props()

  let listElement: HTMLElement | null = $state(null)

  $effect(() => {
    activeIndex
    suggestions.length
    void tick().then(scrollActivePromptCommandIntoView)
  })

  function scrollActivePromptCommandIntoView() {
    const activeOption = listElement?.querySelector<HTMLElement>(
      `[data-prompt-command-index="${activeIndex}"]`
    )
    activeOption?.scrollIntoView({ block: 'nearest' })
  }
</script>

<div
  class="absolute right-0 bottom-[calc(100%+8px)] left-0 z-30 max-h-[min(260px,45vh)] overflow-hidden"
  role="listbox"
  aria-label={title}
>
  <div
    class={cardClass(
      'gap-0 rounded-lg bg-white/98 py-0 shadow-[0_18px_48px_rgba(36,45,39,0.16)] backdrop-blur'
    )}
  >
    <div class={cardHeaderClass('border-b px-3 py-2')}>
      <div class={cardTitleClass('text-[10px] font-bold text-muted-foreground uppercase')}>
        {title}
      </div>
    </div>
    <div
      class={cardContentClass('scrollbar-slim max-h-55 overflow-y-auto p-1')}
      bind:this={listElement}
    >
      {#each suggestions as suggestion, index (suggestion.id)}
        {#if suggestion.disabled}
          <div class="px-2 py-2 text-xs text-muted-foreground">{suggestion.description}</div>
        {:else}
          <button
            type="button"
            class={buttonClass(
              'ghost',
              'default',
              `h-auto w-full justify-start px-2 py-2 text-left hover:bg-emerald-50 hover:text-foreground ${
                index === activeIndex
                  ? 'bg-emerald-50 text-foreground shadow-[inset_0_0_0_1px_rgba(16,185,129,0.16)]'
                  : ''
              }`
            )}
            data-prompt-command-index={index}
            role="option"
            aria-selected={index === activeIndex}
            onmousedown={(event) => event.preventDefault()}
            onclick={() => void onApply(suggestion)}
          >
            <span class="grid min-w-0 flex-1 gap-0.5">
              <span class="flex min-w-0 items-center gap-1.5">
                <span
                  class={badgeClass(
                    'secondary',
                    'rounded-md bg-emerald-50 font-mono text-xs font-bold text-emerald-800'
                  )}
                >
                  {suggestion.label}
                </span>
                {#if suggestion.detail}
                  <span class="min-w-0 truncate text-[10px] font-semibold text-amber-700">
                    {suggestion.detail}
                  </span>
                {/if}
              </span>
              <span class="block min-w-0 truncate text-xs font-normal text-muted-foreground">
                {suggestion.description}
              </span>
            </span>
          </button>
        {/if}
      {/each}
    </div>
  </div>
</div>
