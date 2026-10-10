<script lang="ts">
  import AndaMark from './AndaMark.svelte'
  import { prefersReducedMotion } from '$lib/anda/chat/entrance'
  import { fly } from 'svelte/transition'

  // The transcript's last line while a turn runs: Anda reads along, the label
  // shimmers, and each new step slides in over the one before.
  let {
    label,
    elapsed = '',
    step = ''
  }: {
    label: string
    elapsed?: string
    step?: string
  } = $props()

  const stepIn = { y: 8, duration: prefersReducedMotion() ? 0 : 280 }
</script>

<div class="chat-working" role="status">
  <span class="chat-working-mark" aria-hidden="true">
    <AndaMark working class="chat-working-panda" />
  </span>
  <span class="chat-working-label anda-shimmer">{label}</span>
  {#if elapsed}
    <span class="chat-working-elapsed">{elapsed}</span>
  {/if}
  {#if step}
    <span class="chat-working-step">
      {#key step}
        <code title={step} in:fly={stepIn}>{step}</code>
      {/key}
    </span>
  {/if}
</div>

<style>
  .chat-working {
    display: flex;
    min-width: 0;
    align-items: center;
    gap: 0.5rem;
    padding: 0.75rem 0;
    color: var(--message-muted, #737373);
    font-size: 0.75rem;
    line-height: 1rem;
    animation: chat-working-in 420ms var(--anda-ease-out, ease-out) both;
  }

  .chat-working-mark {
    position: relative;
    display: grid;
    flex-shrink: 0;
    place-items: center;
    width: 1.5rem;
    height: 1.25rem;
  }

  /* A soft aura that breathes behind the mark. */
  .chat-working-mark::before {
    position: absolute;
    inset: -0.375rem -0.25rem;
    border-radius: 999px;
    background: radial-gradient(
      closest-side,
      color-mix(in srgb, var(--chat-accent, #10b981) 32%, transparent),
      transparent
    );
    content: '';
    animation: chat-working-aura 1.9s ease-in-out infinite;
  }

  .chat-working-mark :global(.chat-working-panda) {
    position: relative;
    width: 1.375rem;
    height: auto;
  }

  .chat-working-label {
    flex-shrink: 0;
    font-weight: 500;
  }

  .chat-working-elapsed {
    flex-shrink: 0;
    font-variant-numeric: tabular-nums;
  }

  .chat-working-step {
    display: grid;
    min-width: 0;
    overflow: hidden;
  }

  .chat-working-step code {
    grid-area: 1 / 1;
    min-width: 0;
    overflow: hidden;
    font-size: 0.6875rem;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  @keyframes chat-working-in {
    from {
      opacity: 0;
      transform: translateY(6px);
    }
  }

  @keyframes chat-working-aura {
    0%,
    100% {
      opacity: 0.45;
      transform: scale(0.82);
    }
    50% {
      opacity: 1;
      transform: scale(1.12);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .chat-working,
    .chat-working-mark::before {
      animation: none;
    }
  }
</style>
