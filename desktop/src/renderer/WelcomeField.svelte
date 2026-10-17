<script lang="ts">
  import { prefersReducedMotion } from '$lib/anda/chat/entrance'
  import { createWelcomeField, type WelcomeField } from './welcome-field'

  // The New chat backdrop (see welcome-field.ts). It reacts to the pointer
  // anywhere in its parent, the chat column, and stays behind its content.
  let { dark, origin }: { dark: boolean; origin: HTMLElement | null } = $props()

  let canvas: HTMLCanvasElement
  let field: WelcomeField | null = null

  $effect(() => {
    const host = canvas.parentElement
    if (!host) return
    const instance = createWelcomeField(canvas, host, () => origin, {
      reducedMotion: prefersReducedMotion()
    })
    field = instance
    return () => {
      instance.destroy()
      field = null
    }
  })

  $effect(() => {
    void dark
    field?.restyle()
  })
</script>

<canvas bind:this={canvas} class="welcome-field" aria-hidden="true"></canvas>
