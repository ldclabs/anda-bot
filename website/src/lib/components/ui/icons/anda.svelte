<script lang="ts">
	import { panda } from './panda';

	// The Anda mark: the sprite's head. It blinks, and its ears twitch when the
	// mark (or a link around it) is hovered. `--mark-line` sets the outline.
	let { class: className = '' }: { class?: string } = $props();
</script>

<svg class="anda-mark {className}" viewBox="-6 -62 512 512" aria-hidden="true">
	<path class="dark ear ear-l" d={panda.ears[0]} />
	<path class="dark ear ear-r" d={panda.ears[1]} />
	<path class="face" d={panda.head} />
	<path class="dark" d={panda.eyes.join('')} />
	<path class="pupil" d={panda.pupils.join('')} />
	<path class="dark" d={panda.muzzle} fill-rule="evenodd" />
</svg>

<style>
	.anda-mark {
		display: block;
		flex-shrink: 0;
		overflow: visible;
	}
	.dark,
	.face {
		stroke: var(--mark-line, #f1a64e);
		stroke-width: var(--mark-stroke, 1.4px);
		stroke-linejoin: round;
		vector-effect: non-scaling-stroke;
	}
	.dark {
		fill: #10201c;
	}
	.face {
		fill: #f6f0e4;
	}
	.pupil {
		fill: #ffd08a;
		transform-box: fill-box;
		transform-origin: center;
		animation: blink 5.4s ease-in-out infinite;
	}
	.ear {
		transform-box: fill-box;
		transition: rotate 240ms cubic-bezier(0.3, 1.8, 0.5, 1);
	}
	.ear-l {
		transform-origin: 70% 90%;
	}
	.ear-r {
		transform-origin: 30% 90%;
	}
	.anda-mark:hover .ear-l,
	:global(a:hover) .anda-mark .ear-l {
		rotate: -14deg;
	}
	.anda-mark:hover .ear-r,
	:global(a:hover) .anda-mark .ear-r {
		rotate: 14deg;
	}
	@keyframes blink {
		0%,
		93%,
		100% {
			transform: scaleY(1);
		}
		96% {
			transform: scaleY(0.1);
		}
	}
</style>
