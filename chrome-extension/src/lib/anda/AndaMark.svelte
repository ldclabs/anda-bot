<script lang="ts" module>
  // The head of the Anda panda (website/src/lib/assets/panda.svg, rounded).
  const mark = {
    head: 'M249.3 7C289.3 7.1 326.3 17.5 360.4 38.1C390.9 56.6 416.4 81.5 436.8 112.8C455.9 142.2 467.9 172.8 472.7 204.7C477.7 237.5 474 265.7 461.8 289.3C429.7 351.2 359.1 383.5 249.9 386.3C249.7 386.3 249.4 386.3 249.2 386.3C249.1 386.3 249.1 386.3 249 386.3C248.9 386.3 248.9 386.3 248.8 386.3C248.6 386.3 248.3 386.3 248.1 386.3C138.9 383.5 68.3 351.2 36.2 289.3C24 265.7 20.4 237.5 25.3 204.7C30.1 172.8 42.1 142.2 61.2 112.8C81.6 81.5 107.1 56.6 137.6 38.1C171.7 17.5 208.7 7.1 248.7 7C248.7 7 248.8 7 248.8 7C248.9 7 248.9 7 249 7L249 7L249.1 7C249.1 7 249.1 7 249.2 7C249.2 7 249.3 7 249.3 7Z',
    earLeft:
      'M140.9 45.5C144.7 43.2 148.5 41 152.4 38.9C138.2 21.7 116.4 11.7 93.4 11.7C51.9 11.7 18.2 43.8 18.2 83.3C18.2 107.2 30.7 129.5 51.6 142.7C55.6 134.9 60.2 127.1 65.2 119.4C85.4 88.4 110.6 63.8 140.9 45.5Z',
    earRight:
      'M437.8 119.4C442.8 127.1 447.3 134.8 451.3 142.7C472.1 129.4 484.5 107.1 484.5 83.3C484.5 43.8 450.9 11.7 409.3 11.7C386.4 11.7 364.6 21.7 350.4 38.9C354.3 40.9 358.2 43.1 362.1 45.5C392.3 63.8 417.5 88.4 437.8 119.4Z',
    eyes: 'M105.3 204.9C68.6 153.1 149.2 99.6 187.7 125.5C203.8 137.8 204.9 149.6 199.9 163.2C194.9 176.4 180.3 181.5 166.1 191.3C153.2 200.2 160 214.7 146.5 221.1C136.7 225.8 114.4 217.8 105.3 204.9ZM298.6 163.2C293.6 149.6 294.7 137.8 310.8 125.5C349.2 99.6 429.8 153.1 393.2 204.9C384 217.8 361.8 225.8 352 221.1C338.4 214.7 345.3 200.2 332.4 191.3C318.2 181.5 303.6 176.4 298.6 163.2Z',
    pupilLeft:
      'M174.8 148.8C165.9 142.9 147.5 155.1 155.9 166.8C164.3 178.5 185.7 156.6 174.8 148.8Z',
    pupilRight:
      'M342.5 166.8C334.1 178.5 312.7 156.6 323.6 148.8C332.4 142.9 350.9 155.1 342.5 166.8Z',
    muzzle:
      'M194.5 259.8C193.6 258.6 193.1 257.2 193.1 255.7C193.1 251.9 196.3 248.9 200.2 248.9C202.5 248.9 204.7 250 206 251.7C208.1 254.4 210.7 256.5 213.8 258.1C217 259.7 220.5 260.5 224.2 260.5C227.9 260.5 231.3 259.7 234.6 258.1C237.6 256.6 240.2 254.5 242.3 251.9V240.8C239.7 240.1 237.3 239.1 235 237.7L232.1 235.8C225.4 231.7 219.9 226 216.1 219.3C212.5 212.9 215.6 205 222.6 202.3L226.9 200.7C241.2 195.2 257.2 195.2 271.5 200.7L275.8 202.3C282.9 205 285.9 212.9 282.3 219.3C278.5 226 273 231.7 266.4 235.8L263.4 237.7C261.3 239 258.9 240 256.5 240.7V253.3C256.5 253.4 256.5 253.6 256.5 253.7C258.3 255.5 260.3 256.9 262.6 258.1C265.9 259.7 269.3 260.5 273 260.5C276.7 260.5 280.1 259.7 283.4 258.1C286.5 256.5 289.1 254.4 291.2 251.7C292.5 250 294.7 248.9 297 248.9C300.9 248.9 304.1 251.9 304.1 255.7C304.1 257.2 303.6 258.6 302.6 259.8C299.2 264.2 295 267.6 289.9 270.1C284.6 272.7 279 274 273 274C267 274 261.4 272.7 256.1 270.1C253.3 268.8 250.8 267.1 248.6 265.2C246.3 267.1 243.8 268.8 241.1 270.1C235.8 272.7 230.2 274 224.2 274C218.2 274 212.6 272.7 207.3 270.1C202.2 267.6 197.9 264.2 194.5 259.8Z'
  }
</script>

<script lang="ts">
  import { prefersReducedMotion } from '$lib/anda/chat/entrance'

  // The Anda mark, alive: it blinks, reads along while Anda works, follows the
  // pointer with its eyes when `track` is on, and wobbles when tapped (pandas
  // roll; they never spin).
  let {
    class: className = '',
    working = false,
    track = false
  }: {
    class?: string
    working?: boolean
    track?: boolean
  } = $props()

  let svg: SVGSVGElement | null = $state(null)
  let happy = $state(false)
  let happyTimer: number | undefined

  $effect(() => {
    const element = svg
    if (!track || !element) return
    let frame = 0
    let x = 0
    let y = 0
    const look = () => {
      frame = 0
      const rect = element.getBoundingClientRect()
      const dx = x - (rect.left + rect.width / 2)
      const dy = y - (rect.top + rect.height * 0.45)
      const distance = Math.hypot(dx, dy) || 1
      const reach = Math.min(1, distance / 220)
      element.style.setProperty('--gaze-x', ((dx / distance) * reach).toFixed(3))
      element.style.setProperty('--gaze-y', ((dy / distance) * reach).toFixed(3))
    }
    const move = (event: PointerEvent) => {
      x = event.clientX
      y = event.clientY
      frame ||= requestAnimationFrame(look)
    }
    const rest = () => {
      element.style.setProperty('--gaze-x', '0')
      element.style.setProperty('--gaze-y', '0')
    }
    window.addEventListener('pointermove', move, { passive: true })
    document.documentElement.addEventListener('pointerleave', rest)
    return () => {
      cancelAnimationFrame(frame)
      window.removeEventListener('pointermove', move)
      document.documentElement.removeEventListener('pointerleave', rest)
    }
  })

  $effect(() => () => clearTimeout(happyTimer))

  function wobble() {
    if (!track || !svg || prefersReducedMotion()) return
    svg.animate(
      [
        { rotate: '0deg' },
        { rotate: '-13deg', offset: 0.18 },
        { rotate: '9deg', offset: 0.42 },
        { rotate: '-5deg', offset: 0.64 },
        { rotate: '2deg', offset: 0.84 },
        { rotate: '0deg' }
      ],
      { duration: 900, easing: 'ease-out' }
    )
    happy = true
    clearTimeout(happyTimer)
    happyTimer = window.setTimeout(() => (happy = false), 900)
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<svg
  bind:this={svg}
  class="anda-mark {className}"
  class:anda-mark-working={working}
  class:anda-mark-tracking={track}
  class:anda-mark-happy={happy}
  viewBox="10 0 482 396"
  aria-hidden="true"
  onpointerdown={wobble}
>
  <g class="anda-mark-head">
    <path class="anda-mark-dark anda-mark-ear anda-mark-ear-l" d={mark.earLeft} />
    <path class="anda-mark-dark anda-mark-ear anda-mark-ear-r" d={mark.earRight} />
    <path class="anda-mark-face" d={mark.head} />
    <path class="anda-mark-dark" d={mark.eyes} />
    <g class="anda-mark-gaze">
      <path class="anda-mark-pupil" d={mark.pupilLeft} />
      <path class="anda-mark-pupil" d={mark.pupilRight} />
    </g>
    <path class="anda-mark-dark" d={mark.muzzle} fill-rule="evenodd" />
  </g>
</svg>

<style>
  .anda-mark {
    --mark-face: #fbfaf6;
    --mark-dark: #1d211f;
    --mark-pupil: #ffffff;
    display: block;
    flex-shrink: 0;
    overflow: visible;
    transform-origin: 50% 92%;
  }

  :global(.dark) .anda-mark {
    --mark-face: #ecebe5;
    --mark-dark: #121413;
  }

  .anda-mark-dark,
  .anda-mark-face {
    stroke: var(--mark-dark);
    stroke-width: var(--mark-stroke, 1px);
    stroke-linejoin: round;
    vector-effect: non-scaling-stroke;
  }

  .anda-mark-dark {
    fill: var(--mark-dark);
  }

  .anda-mark-face {
    fill: var(--mark-face);
  }

  .anda-mark-pupil {
    fill: var(--mark-pupil);
    transform-box: fill-box;
    transform-origin: center;
    animation: anda-mark-blink 5.2s ease-in-out infinite;
  }

  .anda-mark-head,
  .anda-mark-gaze,
  .anda-mark-ear {
    transform-box: fill-box;
  }

  .anda-mark-head {
    transform-origin: 50% 90%;
  }

  .anda-mark-ear {
    transition: rotate 260ms var(--anda-spring, ease-out);
  }

  .anda-mark-ear-l {
    transform-origin: 70% 90%;
  }

  .anda-mark-ear-r {
    transform-origin: 30% 90%;
  }

  /* Following the pointer: the eyes lead, the head leans after them. */
  .anda-mark-tracking {
    cursor: grab;
  }

  .anda-mark-tracking .anda-mark-gaze {
    translate: calc(var(--gaze-x, 0) * 15px) calc(var(--gaze-y, 0) * 10px);
    transition: translate 260ms var(--anda-ease-out, ease-out);
  }

  .anda-mark-tracking .anda-mark-head {
    translate: calc(var(--gaze-x, 0) * 7px) calc(var(--gaze-y, 0) * 5px);
    rotate: calc(var(--gaze-x, 0) * 5deg);
    transition:
      translate 520ms var(--anda-spring-soft, ease-out),
      rotate 520ms var(--anda-spring-soft, ease-out);
  }

  .anda-mark-tracking:hover .anda-mark-ear-l {
    rotate: -14deg;
  }

  .anda-mark-tracking:hover .anda-mark-ear-r {
    rotate: 14deg;
  }

  .anda-mark-happy .anda-mark-pupil {
    animation: none;
    scale: 1.15 0.3;
  }

  /* Working: a small nod, ears that twitch in turn, eyes that read along. */
  .anda-mark-working .anda-mark-head {
    animation: anda-mark-nod 1.9s ease-in-out infinite;
  }

  .anda-mark-working .anda-mark-ear-l {
    animation: anda-mark-ear-l 3.8s ease-in-out infinite;
  }

  .anda-mark-working .anda-mark-ear-r {
    animation: anda-mark-ear-r 3.8s ease-in-out 1.9s infinite;
  }

  .anda-mark-working .anda-mark-gaze {
    animation: anda-mark-read 2.6s ease-in-out infinite;
  }

  @keyframes anda-mark-blink {
    0%,
    92%,
    100% {
      scale: 1 1;
    }
    95% {
      scale: 1 0.1;
    }
  }

  @keyframes anda-mark-nod {
    0%,
    100% {
      translate: 0 0;
      rotate: -2deg;
    }
    50% {
      translate: 0 7px;
      rotate: 2deg;
    }
  }

  @keyframes anda-mark-ear-l {
    0%,
    80%,
    100% {
      rotate: 0deg;
    }
    86% {
      rotate: -16deg;
    }
    92% {
      rotate: 4deg;
    }
  }

  @keyframes anda-mark-ear-r {
    0%,
    80%,
    100% {
      rotate: 0deg;
    }
    86% {
      rotate: 16deg;
    }
    92% {
      rotate: -4deg;
    }
  }

  @keyframes anda-mark-read {
    0%,
    100% {
      translate: -12px 5px;
    }
    45% {
      translate: 12px 5px;
    }
    55% {
      translate: 12px 8px;
    }
    68% {
      translate: -12px 8px;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .anda-mark-pupil,
    .anda-mark-working .anda-mark-head,
    .anda-mark-working .anda-mark-ear-l,
    .anda-mark-working .anda-mark-ear-r,
    .anda-mark-working .anda-mark-gaze {
      animation: none;
    }

    .anda-mark-tracking .anda-mark-gaze,
    .anda-mark-tracking .anda-mark-head {
      transition: none;
    }
  }
</style>
