<script lang="ts">
	import type { LandingCopy } from '$lib/content/landing';
	import {
		ArrowRight,
		Brain,
		GitBranch,
		Globe,
		LayoutPanelLeft,
		MessageSquare,
		Monitor,
		Terminal
	} from '@lucide/svelte';

	let { items, links }: { items: LandingCopy['surfaces']['items']; links: string[] } = $props();

	const icons = [LayoutPanelLeft, Terminal, MessageSquare, Monitor];
	const platforms = ['Telegram', 'WeChat', 'Discord', 'Lark'];
</script>

<!-- The thread runs along the tops of the frames like a line they hang from. -->
<div class="line">
	<i class="ta lead-lane" data-thread="59.5"></i>
	{#each items as item, index}
		{@const Icon = icons[index]}
		<div class="slot s{index}">
			<i
				class="ta a-hang wide"
				data-thread={60 + index}
				data-thread-dir="h"
				data-thread-dwell="50"
				data-thread-pose="hello"
				data-thread-station={`srf-${index}`}
			></i>
			<!-- two columns: the second row is strung back from right to left -->
			<i
				class="ta a-hang mid"
				data-thread={[60, 61, 63, 62][index]}
				data-thread-dir={index < 2 ? 'h' : '-h'}
				data-thread-dwell="50"
				data-thread-pose="hello"
				data-thread-station={`srf-${index}`}
			></i>
			<i
				class="ta a-side narrow"
				data-thread={60 + index}
				data-thread-dwell="50"
				data-thread-pose="hello"
				data-thread-station={`srf-${index}`}
			></i>
			<article class="surface" data-station={`srf-${index}`}>
				<div class="frame f{index}" aria-hidden="true">
					{#if index === 0}
						<div class="page">
							<b></b><span></span><span></span><span class="short"></span><span></span>
						</div>
						<div class="side">
							<i class="me"></i><i class="bot"></i><i class="me short"></i>
						</div>
					{:else if index === 1}
						<div class="tui">
							<div class="tui-bar" dir="ltr">anda</div>
							<p><em>›</em><span style="--w: 60%"></span></p>
							<p><span style="--w: 82%" class="dim"></span></p>
							<p><span style="--w: 46%" class="dim"></span></p>
							<p><em>›</em><i class="caret"></i></p>
						</div>
					{:else if index === 2}
						<div class="chat">
							<div class="platforms" dir="ltr">
								{#each platforms as platform}<span>{platform}</span>{/each}
							</div>
							<i class="in"></i><i class="out"></i><i class="in short"></i>
						</div>
					{:else}
						<div class="desk">
							<nav>
								<MessageSquare class="size-3" /><Brain class="size-3" /><Globe
									class="size-3"
								/><Terminal class="size-3" /><GitBranch class="size-3" />
							</nav>
							<div class="panes"><span></span><span></span><span></span></div>
						</div>
					{/if}
				</div>
				<div class="copy">
					<h3><Icon class="size-4" />{item.title}</h3>
					{#if item.status}<span class="status">{item.status}</span>{/if}
					<p>{item.detail}</p>
					<a class="link" href={links[index]} target="_blank" rel="noreferrer"
						>{item.linkLabel}<ArrowRight class="direction-arrow size-3.5" /></a
					>
				</div>
			</article>
		</div>
	{/each}
</div>

<style>
	.line {
		display: grid;
		grid-template-columns: repeat(4, minmax(0, 1fr));
		gap: clamp(1.2rem, 2.4vw, 2.2rem);
		margin-top: 5rem;
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
	.line {
		position: relative;
	}
	.slot {
		position: relative;
		padding-top: var(--drop);
	}
	.s0 {
		--drop: 0rem;
		--tilt: -1.6deg;
	}
	.s1 {
		--drop: 1.4rem;
		--tilt: 1.2deg;
	}
	.s2 {
		--drop: 0.4rem;
		--tilt: -0.8deg;
	}
	.s3 {
		--drop: 1.8rem;
		--tilt: 1.8deg;
	}
	.a-hang {
		top: var(--drop);
		inset-inline-start: 50%;
	}
	.a-side {
		top: calc(var(--drop) + 2rem);
		inset-inline-start: 0;
	}
	.mid,
	.narrow {
		display: none;
	}
	.surface {
		transform-origin: 50% 0;
		transform: rotate(var(--tilt));
	}
	:global(.thread-ready) .surface {
		transform: rotate(calc(var(--tilt) + (1 - var(--arrive, 0)) * 5deg))
			translateY(calc((1 - var(--arrive, 0)) * -0.6rem));
		opacity: calc(0.4 + var(--arrive, 0) * 0.6);
	}
	.surface:global([data-live]) {
		animation: sway 5.5s ease-in-out infinite;
	}
	.frame {
		position: relative;
		height: 11.5rem;
		overflow: hidden;
		border: 1px solid var(--line);
		border-radius: 0.9rem;
		background: var(--card);
		box-shadow: 0 26px 50px -34px rgb(60 40 10 / 0.6);
	}
	.f0 {
		display: grid;
		grid-template-columns: 1.4fr 1fr;
	}
	.page {
		display: flex;
		flex-direction: column;
		gap: 0.5rem;
		padding: 0.9rem;
	}
	.page b {
		width: 70%;
		height: 0.55rem;
		border-radius: 3px;
		background: rgb(19 32 27 / 0.5);
	}
	.page span {
		height: 0.32rem;
		border-radius: 3px;
		background: rgb(19 32 27 / 0.12);
	}
	.short {
		width: 60%;
	}
	.side {
		display: flex;
		flex-direction: column;
		justify-content: flex-end;
		gap: 0.45rem;
		padding: 0.7rem;
		border-inline-start: 1px solid var(--line);
		background: rgb(16 32 28 / 0.94);
	}
	.side i,
	.chat i {
		display: block;
		height: 1.1rem;
		border-radius: 0.55rem;
	}
	.side .me {
		align-self: flex-end;
		width: 70%;
		background: #f1a64e;
	}
	.side .bot {
		width: 85%;
		background: rgb(255 255 255 / 0.14);
	}
	.side .me.short {
		width: 45%;
	}
	.f1 {
		background: #0d1a17;
	}
	.tui {
		display: flex;
		height: 100%;
		flex-direction: column;
		gap: 0.6rem;
		padding: 0.6rem 0.75rem;
		font-family: var(--mono);
		font-size: 0.72rem;
	}
	.tui-bar {
		margin: -0.6rem -0.75rem 0.2rem;
		padding: 0.35rem 0.75rem;
		border-bottom: 1px solid rgb(255 255 255 / 0.08);
		color: #35b2ab;
	}
	.tui p {
		display: flex;
		align-items: center;
		gap: 0.5rem;
	}
	.tui em {
		color: #ffd08a;
		font-style: normal;
	}
	.tui span {
		width: var(--w);
		height: 0.34rem;
		border-radius: 3px;
		background: rgb(244 238 227 / 0.6);
	}
	.tui span.dim {
		margin-inline-start: 1rem;
		background: rgb(244 238 227 / 0.22);
	}
	.caret {
		width: 0.45rem;
		height: 0.8rem;
		background: #35b2ab;
		animation: blink 1s steps(2) infinite;
	}
	.chat {
		display: flex;
		height: 100%;
		flex-direction: column;
		gap: 0.5rem;
		padding: 0.75rem;
	}
	.platforms {
		display: flex;
		flex-wrap: wrap;
		gap: 0.3rem;
		margin-bottom: auto;
	}
	.platforms span {
		padding: 0.12rem 0.45rem;
		border-radius: 999px;
		background: rgb(29 123 117 / 0.1);
		color: var(--accent-2);
		font-size: 0.62rem;
		font-weight: 600;
	}
	.chat .in {
		width: 72%;
		background: rgb(19 32 27 / 0.1);
	}
	.chat .out {
		align-self: flex-end;
		width: 58%;
		background: color-mix(in srgb, var(--accent-2) 70%, white);
	}
	.chat .in.short {
		width: 40%;
	}
	.desk {
		display: grid;
		height: 100%;
		grid-template-columns: 2.2rem 1fr;
	}
	.desk nav {
		display: flex;
		flex-direction: column;
		align-items: center;
		gap: 0.7rem;
		padding-top: 0.8rem;
		background: #10201c;
		color: rgb(244 238 227 / 0.7);
	}
	.panes {
		display: grid;
		grid-template-columns: 1fr 1fr;
		grid-template-rows: 1fr 1fr;
		gap: 0.45rem;
		padding: 0.6rem;
	}
	.panes span {
		border-radius: 0.45rem;
		background: rgb(19 32 27 / 0.07);
	}
	.panes span:first-child {
		grid-row: span 2;
		background: rgb(241 166 78 / 0.18);
	}
	.copy {
		padding: 1.1rem 0.2rem 0;
	}
	.copy h3 {
		display: flex;
		align-items: center;
		gap: 0.5rem;
		color: var(--fg);
		font-family: var(--serif);
		font-size: 1.3rem;
	}
	.copy h3 :global(svg) {
		flex-shrink: 0;
		color: var(--accent);
	}
	.status {
		display: inline-block;
		margin-top: 0.5rem;
		padding: 0.1rem 0.5rem;
		border-radius: 999px;
		background: color-mix(in srgb, var(--accent-2) 12%, transparent);
		color: var(--accent-2);
		font-size: 0.7rem;
		font-weight: 600;
	}
	.copy p {
		margin-top: 0.6rem;
		color: var(--muted);
		font-size: 0.92rem;
		line-height: 1.7;
	}
	.link {
		display: inline-flex;
		align-items: center;
		gap: 0.45rem;
		margin-top: 0.9rem;
		color: var(--accent);
		font-size: 0.85rem;
		font-weight: 600;
		text-underline-offset: 4px;
	}
	.link:hover {
		text-decoration: underline;
	}
	@keyframes sway {
		50% {
			transform: rotate(calc(var(--tilt) * -0.4));
		}
	}
	@keyframes blink {
		50% {
			opacity: 0;
		}
	}
	@media (max-width: 1199px) {
		.line {
			grid-template-columns: repeat(2, minmax(0, 1fr));
			row-gap: 4rem;
		}
		.wide {
			display: none;
		}
		.mid {
			display: block;
		}
	}
	@media (max-width: 767px) {
		.line {
			grid-template-columns: 1fr;
			row-gap: 3rem;
			margin-top: 3rem;
		}
		.slot {
			padding-top: 0;
		}
		.mid {
			display: none;
		}
		.narrow {
			display: block;
		}
		.surface {
			--tilt: 0deg;
		}
		:global(.thread-ready) .surface {
			transform: translateY(calc((1 - var(--arrive, 0)) * 0.8rem));
		}
	}
</style>
