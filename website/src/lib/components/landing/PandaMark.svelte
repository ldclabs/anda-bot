<script lang="ts">
	import { panda } from '$lib/components/ui/icons/panda';
</script>

<!-- Where Anda's journey ends: the travelling Anda grows to fill this box. The
     drawing below is the still portrait for reduced motion and no-JS visitors. -->
<div class="panda" data-actor-home>
	<svg viewBox="0 0 500 500" aria-hidden="true">
		<path class="dark" d={panda.ears.join('')} />
		<path class="dark" d={panda.legs.join('')} />
		<path class="face" d={panda.head} />
		<path class="dark" d={panda.eyes.join('')} />
		<path class="pupil" d={panda.pupils.join('')} />
		<path class="dark" d={panda.muzzle} fill-rule="evenodd" />
	</svg>
</div>

<style>
	.panda {
		width: min(30rem, 100%);
		aspect-ratio: 1;
		margin-inline: auto;
	}
	:global(.landing.thread-ready:not(.thread-reduced)) svg {
		visibility: hidden;
	}
	svg {
		display: block;
		width: 100%;
		height: 100%;
		overflow: visible;
	}
	.dark,
	.face {
		stroke: #f1a64e;
		stroke-width: 1.6px;
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
	}
</style>
