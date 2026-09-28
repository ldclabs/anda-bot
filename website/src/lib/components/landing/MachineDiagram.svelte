<script lang="ts">
	import type { LandingCopy } from '$lib/content/landing';
	import { ArrowLeftRight, Brain, MessageSquare, Settings } from '@lucide/svelte';

	let { copy }: { copy: LandingCopy['demo']['control'] } = $props();

	// Model families the runtime can be configured with; the memory stays put
	// while the provider behind the socket changes.
	const providers = ['anthropic', 'openai', 'gemini', 'deepseek', 'kimi', 'glm'];
</script>

<div class="machine" aria-hidden="true">
	<i class="ta lead-lane" data-thread="49.5"></i>
	<div class="boundary">
		<i class="ta a-enter" data-thread="50"></i>
		<header>
			<span class="path" dir="ltr">~/.anda</span>
			<span class="label">{copy.machine}</span>
		</header>
		<div class="files">
			<span><Settings class="size-3.5" />{copy.items[0]}</span>
			<span><MessageSquare class="size-3.5" />{copy.items[1]}</span>
		</div>
		<div class="core">
			<i
				class="ta a-core"
				data-thread="51"
				data-thread-loop="-58"
				data-thread-dwell="100"
				data-thread-pose="orb"
				data-thread-side="start"
				data-thread-station="ctl-mem"
			></i>
			<div class="orb" data-station="ctl-mem">
				<Brain class="size-6" />
				<span>{copy.items[2]}</span>
			</div>
			<p class="stays" data-station="ctl-mem">{copy.stays}</p>
		</div>
		<i class="ta a-out wide" data-thread="52" data-thread-dir="h" data-thread-station="ctl-out"></i>
		<i class="ta a-back wide" data-thread="54" data-thread-dir="-h"></i>
		<i class="ta a-exit wide" data-thread="55" data-thread-station="ctl-open"></i>
		<i class="ta a-exit narrow" data-thread="52.5" data-thread-station="ctl-out"></i>
	</div>

	<div class="socket-slot">
		<i
			class="ta a-model wide"
			data-thread="53"
			data-thread-dir="h"
			data-thread-dwell="130"
			data-thread-pose="plug"
			data-thread-side="start"
			data-thread-station="ctl-model"
		></i>
		<i
			class="ta a-model-top narrow"
			data-thread="53.5"
			data-thread-dwell="130"
			data-thread-pose="plug"
			data-thread-station="ctl-model"
		></i>
		<i class="ta a-model-bottom narrow" data-thread="54.5" data-thread-station="ctl-open"></i>
		<div class="model" data-station="ctl-model">
			<span class="model-label"><ArrowLeftRight class="size-3.5" />{copy.model}</span>
			<div class="deck" dir="ltr">
				<ul>
					{#each [...providers, providers[0]] as provider}<li>{provider}</li>{/each}
				</ul>
			</div>
		</div>
	</div>
</div>

<style>
	.machine {
		position: relative;
		display: grid;
		grid-template-columns: minmax(0, 1fr) 11.5rem;
		align-items: center;
		gap: 3.5rem;
	}
	/* single-column layouts: arrive down the margin lane, clear of the copy */
	.lead-lane {
		display: none;
		top: -2.5rem;
		inset-inline-start: var(--lane-in);
	}
	@media (max-width: 1023px) {
		.lead-lane {
			display: block;
		}
	}
	.boundary {
		position: relative;
		padding: 1.3rem 1.4rem 1.6rem;
		border: 1.5px dashed color-mix(in srgb, var(--fg) 35%, transparent);
		border-radius: 1.6rem;
		background: color-mix(in srgb, var(--card) 55%, transparent);
	}
	header {
		display: flex;
		align-items: baseline;
		justify-content: space-between;
		gap: 1rem;
	}
	.path {
		color: var(--accent);
		font-family: var(--mono);
		font-size: 0.95rem;
		font-weight: 600;
	}
	.label {
		color: var(--muted);
		font-size: 0.75rem;
		letter-spacing: 0.06em;
		text-transform: uppercase;
	}
	.files {
		display: flex;
		flex-wrap: wrap;
		gap: 0.5rem;
		margin-top: 1rem;
	}
	.files span {
		display: inline-flex;
		align-items: center;
		gap: 0.4rem;
		padding: 0.35rem 0.7rem;
		border: 1px solid var(--line);
		border-radius: 0.6rem;
		background: var(--card);
		color: var(--fg);
		font-size: 0.78rem;
	}
	.files :global(svg) {
		color: var(--accent-2);
	}
	.core {
		position: relative;
		display: flex;
		flex-direction: column;
		align-items: center;
		gap: 1.3rem;
		margin-top: 2.6rem;
	}
	/* the knot circles the orb: anchor sits one loop radius from its centre */
	.a-core {
		top: 44px;
		inset-inline-start: calc(50% - 58px);
	}
	.orb {
		display: grid;
		width: 88px;
		aspect-ratio: 1;
		place-content: center;
		justify-items: center;
		gap: 0.15rem;
		border-radius: 50%;
		background:
			radial-gradient(circle at 35% 30%, #fff8ea, transparent 60%),
			radial-gradient(circle, #f5d9a8, #e7b56a);
		box-shadow:
			0 0 0 6px color-mix(in srgb, var(--paper) 80%, transparent),
			0 16px 36px -14px rgb(180 100 20 / 0.6);
		color: #5a3308;
		font-size: 0.7rem;
		font-weight: 650;
	}
	:global(.thread-ready) .orb {
		transform: scale(calc(0.9 + var(--arrive, 0) * 0.1));
	}
	.orb:global([data-live]) {
		animation: breathe 3.6s ease-in-out infinite;
	}
	.stays {
		margin-top: 0.8rem;
		color: var(--muted);
		font-family: var(--serif);
		font-size: 0.95rem;
		font-style: italic;
	}
	:global(.thread-ready) .stays {
		opacity: calc(var(--arrive, 0) * 2 - 0.6);
	}
	.a-enter {
		top: 0;
		inset-inline-start: 30%;
	}
	.a-out {
		top: 64%;
		inset-inline-start: 100%;
	}
	.a-back {
		top: 76%;
		inset-inline-start: 100%;
	}
	.a-exit {
		top: 100%;
		inset-inline-start: 72%;
	}
	.socket-slot {
		position: relative;
		align-self: end;
		margin-bottom: 18%;
	}
	.a-model {
		top: 2.6rem;
		inset-inline-start: 0;
	}
	.a-model-top {
		top: 0;
		inset-inline-start: 50%;
	}
	.a-model-bottom {
		top: 100%;
		inset-inline-start: 50%;
	}
	.model {
		padding: 0.9rem 1rem 1rem;
		border: 1px solid var(--line);
		border-radius: 1rem;
		background: #10201c;
		box-shadow: 0 24px 50px -30px rgb(0 0 0 / 0.7);
		color: #f4eee3;
	}
	.model-label {
		display: inline-flex;
		align-items: center;
		gap: 0.35rem;
		color: rgb(244 238 227 / 0.6);
		font-size: 0.7rem;
	}
	.deck {
		height: 1.9rem;
		margin-top: 0.4rem;
		overflow: hidden;
		mask-image: linear-gradient(180deg, transparent, #000 25%, #000 75%, transparent);
	}
	.deck ul {
		display: flex;
		flex-direction: column;
	}
	.deck li {
		height: 1.9rem;
		color: #ffd08a;
		font-family: var(--mono);
		font-size: 1.05rem;
		font-weight: 600;
		line-height: 1.9rem;
	}
	:global([data-live]) .deck ul {
		animation: deck 13.2s cubic-bezier(0.7, 0, 0.3, 1) infinite;
	}
	.narrow {
		display: none;
	}
	@keyframes deck {
		0%,
		13% {
			transform: translateY(0);
		}
		16.67%,
		29.67% {
			transform: translateY(-1.9rem);
		}
		33.33%,
		46.33% {
			transform: translateY(-3.8rem);
		}
		50%,
		63% {
			transform: translateY(-5.7rem);
		}
		66.67%,
		79.67% {
			transform: translateY(-7.6rem);
		}
		83.33%,
		96.33% {
			transform: translateY(-9.5rem);
		}
		100% {
			transform: translateY(-11.4rem);
		}
	}
	@keyframes breathe {
		50% {
			transform: scale(1.04);
		}
	}
	@media (max-width: 767px) {
		.machine {
			grid-template-columns: 1fr;
			gap: 3.2rem;
		}
		.socket-slot {
			justify-self: center;
			width: min(14rem, 100%);
			margin: 0;
		}
		.wide {
			display: none;
		}
		.narrow {
			display: block;
		}
	}
</style>
