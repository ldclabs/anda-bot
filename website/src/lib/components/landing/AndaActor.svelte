<script lang="ts">
	import {
		AlarmClock,
		ArrowLeftRight,
		BookmarkPlus,
		Brain,
		FileText,
		Hand,
		Heart,
		Keyboard,
		Music,
		RefreshCw,
		Search,
		Sparkles,
		Star,
		Users
	} from '@lucide/svelte';
	import { panda } from '$lib/components/ui/icons/panda';

	let { el = $bindable() }: { el?: HTMLElement } = $props();

	// Work poses the journey asks for (data-thread-pose), then tap emotes.
	const badges = [
		['remember', BookmarkPlus],
		['revise', RefreshCw],
		['read', Search],
		['file', FileText],
		['agents', Users],
		['clock', AlarmClock],
		['orb', Brain],
		['plug', ArrowLeftRight],
		['hello', Hand],
		['type', Keyboard]
	] as const;
	const emotes = [
		['heart', Heart],
		['spark', Sparkles],
		['note', Music],
		['star', Star]
	] as const;
</script>

{#snippet face(ears = true)}
	{#if ears}
		<path class="dark ear ear-l" d={panda.ears[0]} />
		<path class="dark ear ear-r" d={panda.ears[1]} />
	{/if}
	<path class="face" d={panda.head} />
	<path class="dark" d={panda.eyes.join('')} />
	<g class="pupils"><path class="pupil" d={panda.pupils.join('')} /></g>
	<path class="dark" d={panda.muzzle} fill-rule="evenodd" />
{/snippet}

<!-- Anda as a little sprite flying the page. Driven by thread/actor.ts. -->
<div class="actor" bind:this={el} data-pose="hover" data-themed aria-hidden="true">
	<span class="shadow"></span>
	<div class="float">
		<span class="aura"></span>
		<div class="body">
			<svg viewBox="0 0 500 500">
				<g class="leg leg-l"><path class="dark" d={panda.legs[1]} /></g>
				<g class="leg leg-r"><path class="dark" d={panda.legs[0]} /></g>
				<g class="head">
					{@render face()}
					<ellipse class="blush" cx="140" cy="262" rx="30" ry="16" />
					<ellipse class="blush" cx="360" cy="262" rx="30" ry="16" />
				</g>
			</svg>
		</div>
	</div>
	<span class="badge">
		{#each badges as [pose, Icon]}<span class="icon" data-for={pose}><Icon /></span>{/each}
		{#each emotes as [emote, Icon]}<span class="icon" data-emote-for={emote}><Icon /></span>{/each}
	</span>
	{#each Array.from({ length: 8 }) as _, index}
		<span class="spark tone-{index % 3}"></span>
	{/each}
	<svg class="agent agent-a" viewBox="0 0 500 400">{@render face()}</svg>
	<svg class="agent agent-b" viewBox="0 0 500 400">{@render face()}</svg>
</div>

<style>
	.actor {
		--size: 72px;
		--step: 0;
		--grow: 1;
		--finale: 0;
		--line: color-mix(in oklab, #f1a64e, #14211c calc(var(--theme, 0) * 100%));
		position: absolute;
		top: 0;
		left: 0;
		z-index: 3;
		width: var(--size);
		height: var(--size);
		pointer-events: none;
		opacity: 0;
		transform-origin: 0 0;
		will-change: transform;
	}
	:global(.landing.thread-ready) .actor {
		opacity: 1;
	}
	:global(.landing.thread-reduced) .actor {
		display: none;
	}

	/* hovering: the body floats, the shadow below breathes with it */
	.float {
		position: absolute;
		inset: 0;
		animation: float 2.6s ease-in-out infinite;
	}
	.shadow {
		position: absolute;
		top: 104%;
		left: 18%;
		width: 64%;
		height: 12%;
		border-radius: 50%;
		background: radial-gradient(closest-side, rgb(30 18 6 / 0.35), transparent);
		opacity: calc((0.25 + var(--theme, 0) * 0.55) * (1 - var(--finale)));
		animation: shadow 2.6s ease-in-out infinite;
	}
	.aura {
		position: absolute;
		inset: -28%;
		border-radius: 50%;
		background: radial-gradient(closest-side, rgb(255 208 138 / 0.34), transparent 72%);
		opacity: calc((1 - var(--theme, 0)) * (1 - var(--finale)));
		animation: aura 3.2s ease-in-out infinite;
	}
	.body {
		position: absolute;
		inset: 0;
		pointer-events: auto;
		cursor: pointer;
		/* tapped: rocks on its round bottom like a roly-poly toy */
		rotate: calc(var(--wobble, 0) * 1rad);
		transform-origin: 50% 96%;
	}
	.body svg {
		display: block;
		width: 100%;
		height: 100%;
		overflow: visible;
	}
	.dark,
	.face {
		stroke: var(--line);
		/* stays fine when Anda grows into the portrait (the sprite is scaled) */
		stroke-width: calc(1.6px * (1 + (var(--grow) - 1) * 0.12) / var(--grow));
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
		animation: blink 4.4s ease-in-out infinite;
	}
	.pupils {
		translate: calc(var(--look-x, 0) * 16px) calc(var(--look-y, 0) * 12px);
		transform-box: fill-box;
		transform-origin: center;
		transition: scale 160ms ease;
	}
	/* happy squint while an emote is showing */
	.actor:global([data-emote]:not([data-emote=''])) .pupils {
		scale: 1 0.2;
	}

	/* flying: legs paddle, head bobs; ears have a mind of their own */
	.leg-l {
		transform-origin: 130px 340px;
		translate: 0 calc(min(0, var(--step)) * 34px);
		rotate: calc(var(--step) * 9deg);
	}
	.leg-r {
		transform-origin: 380px 340px;
		translate: 0 calc(min(0, var(--step) * -1) * 34px);
		rotate: calc(var(--step) * 9deg);
	}
	.head {
		transform-origin: 250px 380px;
		translate: 0 calc(max(var(--step), var(--step) * -1) * -12px);
	}
	.ear-l {
		transform-origin: 120px 120px;
		animation: ear 3.8s ease-in-out infinite;
	}
	.ear-r {
		transform-origin: 380px 120px;
		animation: ear 3.8s 1.3s ease-in-out infinite reverse;
	}
	.blush {
		fill: #f08a6c;
		opacity: 0;
		transition: opacity 400ms ease;
	}
	.actor:global([data-pose='curious']) .head {
		rotate: -9deg;
		transition: rotate 300ms ease;
	}
	.actor:global([data-pose='curious']) .ear,
	.actor:global([data-emote]:not([data-emote=''])) .ear {
		animation-duration: 0.5s;
	}
	.actor:global([data-pose='hello']) .blush,
	.actor:global([data-pose='home']) .blush,
	.actor:global([data-emote]:not([data-emote=''])) .blush {
		opacity: 0.5;
	}
	.actor:global([data-pose='home']) .head {
		animation: breathe 3.8s ease-in-out infinite;
	}
	.actor:global([data-pose='home']) .float {
		animation: none;
	}
	.actor:global([data-pose='type']) .head {
		animation: nod 340ms ease-in-out infinite;
	}
	.actor:global([data-pose='remember']) .head,
	.actor:global([data-pose='revise']) .head,
	.actor:global([data-pose='read']) .head {
		animation: tug 760ms ease-in-out infinite;
	}

	/* what Anda is doing, or how it feels */
	.badge {
		position: absolute;
		top: -54%;
		left: 27%;
		display: grid;
		width: 46%;
		aspect-ratio: 1;
		place-items: center;
		border: 1.5px solid var(--line);
		border-radius: 50%;
		background: color-mix(in oklab, #f4eee3 94%, #ffd08a);
		color: #9c3a1e;
		box-shadow: 0 6px 16px -6px rgb(0 0 0 / 0.45);
		opacity: 0;
		scale: 0.3;
		transition:
			opacity 220ms ease,
			scale 380ms cubic-bezier(0.3, 1.7, 0.5, 1);
	}
	.icon {
		display: none;
		width: 58%;
		height: 58%;
	}
	.icon :global(svg) {
		width: 100%;
		height: 100%;
	}
	.actor:global([data-pose='remember']) .badge,
	.actor:global([data-pose='revise']) .badge,
	.actor:global([data-pose='read']) .badge,
	.actor:global([data-pose='file']) .badge,
	.actor:global([data-pose='agents']) .badge,
	.actor:global([data-pose='clock']) .badge,
	.actor:global([data-pose='orb']) .badge,
	.actor:global([data-pose='plug']) .badge,
	.actor:global([data-pose='hello']) .badge,
	.actor:global([data-pose='type']) .badge,
	.actor:global([data-emote]:not([data-emote=''])) .badge {
		opacity: 1;
		scale: 1;
	}
	.actor:global([data-pose='home']) .badge {
		top: -4%;
		left: 76%;
		width: 16%;
	}
	.actor:global([data-emote]:not([data-emote=''])) .badge {
		color: #d84b42;
	}
	.actor:global([data-pose='remember']) [data-for='remember'],
	.actor:global([data-pose='revise']) [data-for='revise'],
	.actor:global([data-pose='read']) [data-for='read'],
	.actor:global([data-pose='file']) [data-for='file'],
	.actor:global([data-pose='agents']) [data-for='agents'],
	.actor:global([data-pose='clock']) [data-for='clock'],
	.actor:global([data-pose='orb']) [data-for='orb'],
	.actor:global([data-pose='plug']) [data-for='plug'],
	.actor:global([data-pose='hello']) [data-for='hello'],
	.actor:global([data-pose='type']) [data-for='type'] {
		display: block;
	}
	.actor:global([data-emote]:not([data-emote=''])) [data-for] {
		display: none;
	}
	.actor:global([data-emote='heart']) [data-emote-for='heart'],
	.actor:global([data-emote='spark']) [data-emote-for='spark'],
	.actor:global([data-emote='note']) [data-emote-for='note'],
	.actor:global([data-emote='star']) [data-emote-for='star'] {
		display: block;
		animation: pop 500ms cubic-bezier(0.3, 1.8, 0.5, 1);
	}
	.actor:global([data-pose='hello']) [data-for='hello'] {
		transform-origin: 70% 90%;
		animation: wave 900ms ease-in-out infinite;
	}

	/* a burst of sparks when Anda lands or is tapped (animated from actor.ts) */
	.spark {
		position: absolute;
		top: 16%;
		left: 50%;
		width: 12%;
		aspect-ratio: 1;
		background: var(--c);
		clip-path: polygon(50% 0, 62% 38%, 100% 50%, 62% 62%, 50% 100%, 38% 62%, 0 50%, 38% 38%);
		opacity: 0;
	}
	.tone-0 {
		--c: #ffd08a;
	}
	.tone-1 {
		--c: #35b2ab;
	}
	.tone-2 {
		--c: #f08a6c;
	}

	/* subagents: two small Andas pop out and scurry off to help */
	.agent {
		position: absolute;
		top: 34%;
		left: 22%;
		width: 56%;
		height: auto;
		overflow: visible;
		opacity: 0;
		scale: 0.2;
		transition:
			opacity 200ms ease,
			scale 300ms ease,
			translate 500ms cubic-bezier(0.3, 1.5, 0.5, 1);
	}
	.actor:global([data-pose='agents']) .agent {
		opacity: 1;
		scale: 1;
	}
	.actor:global([data-pose='agents']) .agent-a {
		translate: -120% 50%;
		animation: hop 520ms ease-in-out infinite;
	}
	.actor:global([data-pose='agents']) .agent-b {
		translate: 120% 50%;
		animation: hop 520ms 260ms ease-in-out infinite;
	}

	@keyframes float {
		50% {
			translate: 0 -7%;
		}
	}
	@keyframes shadow {
		50% {
			scale: 0.78;
			opacity: 0.6;
		}
	}
	@keyframes aura {
		50% {
			scale: 1.1;
		}
	}
	@keyframes blink {
		0%,
		92%,
		100% {
			transform: scaleY(1);
		}
		95% {
			transform: scaleY(0.1);
		}
	}
	@keyframes ear {
		0%,
		70%,
		100% {
			rotate: 0deg;
		}
		78% {
			rotate: -10deg;
		}
		86% {
			rotate: 4deg;
		}
	}
	@keyframes breathe {
		50% {
			scale: 1.012;
		}
	}
	@keyframes wave {
		50% {
			rotate: -18deg;
		}
	}
	@keyframes nod {
		50% {
			translate: 0 10px;
		}
	}
	@keyframes tug {
		50% {
			rotate: -6deg;
		}
	}
	@keyframes pop {
		from {
			scale: 0.2;
		}
	}
	@keyframes hop {
		50% {
			transform: translateY(-22%);
		}
	}
	@media (max-width: 767px) {
		.actor {
			--size: 46px;
		}
	}
</style>
