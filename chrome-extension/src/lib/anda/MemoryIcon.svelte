<script lang="ts">
  import type { SVGAttributes } from 'svelte/elements'

  let { size = 24, ...props }: SVGAttributes<SVGSVGElement> & { size?: number | string } = $props()
</script>

<!-- A returning line gathers around a memory seed. Wide gaps keep the
     open loops legible at toolbar sizes. -->
<svg
  width={size}
  height={size}
  viewBox="0 0 24 24"
  fill="none"
  stroke="currentColor"
  stroke-width="1.8"
  stroke-linecap="round"
  stroke-linejoin="round"
  aria-hidden="true"
  focusable="false"
  {...props}
>
  <path
    d="M18.8 6.6a8.6 8.6 0 1 0 1 9.1c1.6-4.4-1.7-8.9-6-8.9a5.1 5.1 0 0 0-5.1 5.1c0 2.5 1.7 4 3.7 4"
  />
  <circle cx="12.4" cy="11.6" r="1.45" fill="currentColor" stroke="none" />
</svg>
