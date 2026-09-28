<script lang="ts">
	import type { LandingCopy } from '$lib/content/landing';
	import { Check, Clock, FileText, Globe, Target, Workflow } from '@lucide/svelte';

	let {
		features,
		copy
	}: {
		features: LandingCopy['action']['features'];
		copy: LandingCopy['demo']['action'];
	} = $props();

	const icons = [Globe, FileText, Workflow, Clock];
	/** What Anda does at each card (see AndaActor). */
	const poses = ['read', 'file', 'agents', 'clock'];
	const pageLines = [0.92, 0.74, 0.86, 0.6, 0.8];
	const toolCalls = [
		{ tool: 'read_file', arg: 'docs/launch.md', ok: '' },
		{ tool: 'shell', arg: 'cargo test', ok: '128 passed' },
		{ tool: 'write_file', arg: 'report.md', ok: '' }
	];
</script>

<div class="grid">
	<!-- The thread runs down the gutter and knots onto each card's inner edge. -->
	<i class="ta a-lead" data-thread="39"></i>
	{#each features as feature, index}
		{@const Icon = icons[index]}
		<div class="slot s{index}">
			<i
				class="ta a-knot"
				data-thread={40 + index}
				data-thread-loop={index % 2 ? 8 : -8}
				data-thread-dwell="150"
				data-thread-pose={poses[index]}
				data-thread-side={index % 2 ? 'start' : 'end'}
				data-thread-station={`act-${index}`}
			></i>
			<article class="card" data-station={`act-${index}`}>
				<div class="mini" aria-hidden="true">
					{#if index === 0}
						<div class="browser">
							<div class="chrome">
								<i></i><i></i><i></i>
								<span class="url">atlas.example/launch</span>
							</div>
							<div class="view">
								<div class="page">
									<b></b>
									{#each pageLines as width, line}
										<span class="ln" style="--w: {width}; --k: {line}"><em></em></span>
									{/each}
								</div>
								<div class="panel">
									<strong>{copy.summary}</strong>
									<span class="sum" style="--k: 0"></span>
									<span class="sum" style="--k: 1; --w: 0.7"></span>
									<span class="sum" style="--k: 2; --w: 0.85"></span>
									<small><Check class="size-3" />{copy.browser}</small>
								</div>
							</div>
						</div>
					{:else if index === 1}
						<div class="term" dir="ltr">
							{#each toolCalls as call, line}
								<div class="row" style="--k: {line}">
									<span class="tool">{call.tool}</span>
									<span class="arg">{call.arg}</span>
									<span class="ok"><Check class="size-3" />{call.ok}</span>
								</div>
							{/each}
							<div class="row prompt" style="--k: 3">›<span class="caret"></span></div>
						</div>
					{:else if index === 2}
						<div class="goal">
							<div class="goal-head"><Target class="size-4" /><span>{copy.goal}</span></div>
							<div class="bar"><span></span></div>
							<ul>
								{#each copy.agents as agent, line}
									<li style="--k: {line}"><i></i><span>{agent}</span><b></b></li>
								{/each}
							</ul>
						</div>
					{:else}
						<div class="routine">
							<svg class="clock" viewBox="0 0 48 48">
								<circle cx="24" cy="24" r="21" />
								{#each Array.from({ length: 12 }) as _, tick}
									<line
										x1="24"
										y1="5"
										x2="24"
										y2={tick % 3 ? 7 : 9}
										transform={`rotate(${tick * 30} 24 24)`}
									/>
								{/each}
								<g class="hand-h"><line x1="24" y1="24" x2="24" y2="14" /></g>
								<g class="hand-m"><line x1="24" y1="24" x2="24" y2="9" /></g>
								<circle class="hub" cx="24" cy="24" r="2" />
							</svg>
							<ul>
								{#each copy.routines as routine, line}
									<li style="--k: {line}">
										<span class="when">{routine.when}</span>
										<span class="what">{routine.what}</span>
									</li>
								{/each}
								<li class="next" style="--k: 2">
									<span>{copy.next}</span><span class="when">{copy.routines[0].when}</span>
								</li>
							</ul>
						</div>
					{/if}
				</div>
				<div class="copy">
					<Icon class="size-5" />
					<h3>{feature.title}</h3>
					<p>{feature.detail}</p>
				</div>
			</article>
		</div>
	{/each}
	<i class="ta a-tail" data-thread="45"></i>
</div>

<style>
	.grid {
		display: grid;
		grid-template-columns: repeat(2, minmax(0, 1fr));
		column-gap: clamp(2rem, 6vw, 5.5rem);
		row-gap: 3.5rem;
		margin-top: 4.5rem;
	}
	.grid {
		position: relative;
	}
	.a-lead {
		top: -2.4rem;
		inset-inline-start: 50%;
	}
	.a-tail {
		bottom: -3rem;
		inset-inline-start: 50%;
	}
	.slot {
		position: relative;
	}
	.s1 {
		margin-top: 9rem;
	}
	.s2 {
		margin-top: -5.5rem;
	}
	.s3 {
		margin-top: 3.5rem;
	}
	.a-knot {
		top: 3.2rem;
		inset-inline-start: 100%;
	}
	.s1 .a-knot,
	.s3 .a-knot {
		inset-inline-start: 0;
	}
	.card {
		overflow: hidden;
		border: 1px solid var(--line);
		border-radius: 1.15rem;
		background: var(--card);
		box-shadow:
			0 1px 0 rgb(255 255 255 / 0.7) inset,
			0 30px 60px -44px rgb(60 40 10 / 0.6);
	}
	:global(.thread-ready) .card {
		transform: translateY(calc((1 - min(1, var(--arrive, 0) * 2.5)) * 1.2rem));
		transition: transform 120ms linear;
	}
	.mini {
		--p: var(--arrive, 1);
		height: 12.5rem;
		padding: 1rem;
		border-bottom: 1px solid var(--line);
		background:
			radial-gradient(90% 90% at 0% 0%, rgb(255 255 255 / 0.55), transparent 70%),
			color-mix(in srgb, var(--paper) 70%, #e9dfcc);
		font-size: 0.78rem;
	}
	:global(.landing:not(.thread-ready)) .mini {
		--p: 1;
	}
	.copy {
		padding: 1.3rem 1.4rem 1.5rem;
	}
	.copy :global(svg) {
		color: var(--accent);
	}
	.copy h3 {
		margin-top: 0.8rem;
		color: var(--fg);
		font-family: var(--serif);
		font-size: 1.45rem;
		line-height: 1.25;
	}
	.copy p {
		margin-top: 0.55rem;
		color: var(--muted);
		line-height: 1.7;
	}
	/* staged reveal: item k lights up k/4 of the way through Anda's work here */
	.ln em,
	.row,
	.goal li,
	.routine li {
		--on: clamp(0, calc((var(--p) - var(--k) * 0.16 - 0.05) * 5), 1);
	}

	.browser {
		display: flex;
		height: 100%;
		flex-direction: column;
		overflow: hidden;
		border: 1px solid var(--line);
		border-radius: 0.7rem;
		background: #fffdf8;
	}
	.chrome {
		display: flex;
		align-items: center;
		gap: 0.3rem;
		padding: 0.45rem 0.6rem;
		border-bottom: 1px solid var(--line);
	}
	.chrome i {
		width: 0.45rem;
		aspect-ratio: 1;
		border-radius: 50%;
		background: rgb(19 32 27 / 0.16);
	}
	.url {
		flex: 1;
		margin-inline-start: 0.4rem;
		padding: 0.12rem 0.5rem;
		overflow: hidden;
		border-radius: 999px;
		background: rgb(19 32 27 / 0.06);
		color: var(--muted);
		font-family: var(--mono);
		font-size: 0.66rem;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.view {
		display: grid;
		flex: 1;
		grid-template-columns: 1.5fr 1fr;
	}
	.page {
		position: relative;
		display: flex;
		flex-direction: column;
		gap: 0.5rem;
		padding: 0.8rem;
		overflow: hidden;
	}
	.page b {
		width: 60%;
		height: 0.55rem;
		border-radius: 3px;
		background: rgb(19 32 27 / 0.55);
	}
	.ln {
		position: relative;
		width: calc(var(--w) * 100%);
		height: 0.34rem;
		border-radius: 3px;
		background: rgb(19 32 27 / 0.12);
	}
	.ln em {
		position: absolute;
		inset: -0.18rem -0.1rem;
		border-radius: 2px;
		background: rgb(241 166 78 / 0.45);
		transform: scaleX(var(--on));
		transform-origin: 0 50%;
	}
	:global([data-live]) .page::after {
		position: absolute;
		inset-inline: 0;
		top: 0;
		height: 1.6rem;
		content: '';
		background: linear-gradient(180deg, transparent, rgb(53 178 171 / 0.14), transparent);
		animation: scan 3.2s ease-in-out infinite;
	}
	.panel {
		display: flex;
		flex-direction: column;
		gap: 0.45rem;
		padding: 0.7rem;
		border-inline-start: 1px solid var(--line);
		background: rgb(29 123 117 / 0.06);
	}
	.panel strong {
		color: var(--accent-2);
		font-size: 0.68rem;
	}
	.sum {
		width: calc(var(--w, 0.95) * 100%);
		height: 0.32rem;
		border-radius: 3px;
		background: rgb(29 123 117 / 0.35);
		transform: scaleX(clamp(0, calc((var(--p) - 0.35 - var(--k) * 0.12) * 4), 1));
		transform-origin: 0 50%;
	}
	.panel small {
		display: inline-flex;
		align-items: center;
		gap: 0.25rem;
		margin-top: auto;
		color: var(--accent-2);
		font-size: 0.64rem;
		font-weight: 600;
		opacity: clamp(0, calc((var(--p) - 0.75) * 5), 1);
	}

	.term {
		display: flex;
		height: 100%;
		flex-direction: column;
		gap: 0.55rem;
		padding: 0.85rem 0.9rem;
		border-radius: 0.7rem;
		background: #10201c;
		color: #d9e3d6;
		font-family: var(--mono);
		font-size: 0.72rem;
	}
	.row {
		display: flex;
		align-items: center;
		gap: 0.6rem;
		opacity: calc(0.16 + var(--on) * 0.84);
		transform: translateX(calc((1 - var(--on)) * -0.6rem));
		white-space: nowrap;
	}
	.tool {
		min-width: 5.6rem;
		color: #ffd08a;
	}
	.arg {
		overflow: hidden;
		color: rgb(217 227 214 / 0.7);
		text-overflow: ellipsis;
	}
	.ok {
		display: inline-flex;
		align-items: center;
		gap: 0.25rem;
		margin-inline-start: auto;
		color: #6fd3a6;
	}
	.prompt {
		color: #35b2ab;
	}
	.caret {
		width: 0.45rem;
		height: 0.85rem;
		background: #35b2ab;
		animation: blink 1s steps(2) infinite;
	}

	.goal {
		display: flex;
		height: 100%;
		flex-direction: column;
		gap: 0.75rem;
		padding: 0.2rem 0.1rem;
	}
	.goal-head {
		display: flex;
		align-items: center;
		gap: 0.5rem;
		color: var(--fg);
		font-family: var(--serif);
		font-size: 1rem;
	}
	.goal-head :global(svg) {
		flex-shrink: 0;
		color: var(--accent);
	}
	.bar {
		position: relative;
		height: 0.45rem;
		overflow: hidden;
		border-radius: 999px;
		background: rgb(19 32 27 / 0.1);
	}
	.bar span {
		position: absolute;
		inset: 0;
		border-radius: inherit;
		background: linear-gradient(90deg, var(--accent), #e39a3d);
		transform: scaleX(calc(0.1 + var(--p) * 0.62));
		transform-origin: 0 50%;
	}
	:global(.landing[dir='rtl']) .bar span,
	:global(.landing[dir='rtl']) .ln em,
	:global(.landing[dir='rtl']) .sum {
		transform-origin: 100% 50%;
	}
	:global([data-live]) .bar span::after {
		position: absolute;
		inset: 0;
		content: '';
		background: linear-gradient(90deg, transparent, rgb(255 255 255 / 0.55), transparent);
		animation: shimmer 2.2s ease-in-out infinite;
	}
	.goal ul {
		display: flex;
		flex-direction: column;
		gap: 0.45rem;
		padding-inline-start: 0.55rem;
		border-inline-start: 1.5px solid rgb(19 32 27 / 0.15);
	}
	.goal li {
		position: relative;
		display: flex;
		align-items: center;
		gap: 0.5rem;
		padding: 0.3rem 0.55rem;
		border-radius: 0.5rem;
		background: rgb(255 255 255 / 0.6);
		color: var(--fg);
		font-size: 0.74rem;
		opacity: calc(0.25 + var(--on) * 0.75);
		transform: translateX(calc((1 - var(--on)) * -0.8rem));
	}
	.goal li i {
		position: absolute;
		inset-inline-start: -0.6rem;
		width: 0.55rem;
		height: 1.5px;
		background: rgb(19 32 27 / 0.15);
	}
	.goal li b {
		width: 0.5rem;
		aspect-ratio: 1;
		margin-inline-start: auto;
		border-radius: 50%;
		background: var(--accent-2);
	}
	:global([data-live]) .goal li b {
		animation: pulse 1.6s ease-in-out infinite;
		animation-delay: calc(var(--k) * 0.35s);
	}

	.routine {
		display: flex;
		height: 100%;
		align-items: center;
		gap: 1.2rem;
	}
	.clock {
		width: 6.2rem;
		flex-shrink: 0;
		fill: none;
		stroke: var(--fg);
		stroke-linecap: round;
	}
	.clock circle {
		stroke-width: 1.4;
		fill: #fffdf8;
	}
	.clock line {
		stroke-width: 1.2;
		opacity: 0.55;
	}
	.clock .hand-h line,
	.clock .hand-m line {
		opacity: 1;
		stroke-width: 2;
	}
	.clock .hand-m line {
		stroke: var(--accent);
		stroke-width: 1.6;
	}
	.hand-h,
	.hand-m {
		transform-origin: 24px 24px;
	}
	.hand-h {
		transform: rotate(calc(var(--p) * 60deg));
	}
	.hand-m {
		transform: rotate(calc(var(--p) * 540deg));
	}
	:global([data-live]) .hand-m line {
		transform-origin: 24px 24px;
		animation: spin 8s linear infinite;
	}
	.clock .hub {
		fill: var(--accent);
		stroke: none;
	}
	.routine ul {
		display: flex;
		flex: 1;
		min-width: 0;
		flex-direction: column;
		gap: 0.5rem;
	}
	.routine li {
		display: flex;
		flex-direction: column;
		gap: 0.1rem;
		padding: 0.4rem 0.6rem;
		border-radius: 0.5rem;
		background: rgb(255 255 255 / 0.65);
		opacity: calc(0.3 + var(--on) * 0.7);
		transform: translateY(calc((1 - var(--on)) * 0.5rem));
	}
	.when {
		color: var(--accent);
		font-family: var(--mono);
		font-size: 0.68rem;
	}
	.what {
		color: var(--fg);
		font-size: 0.78rem;
	}
	.routine .next {
		flex-direction: row;
		justify-content: space-between;
		background: transparent;
		color: var(--muted);
		font-size: 0.68rem;
	}

	@keyframes scan {
		0% {
			transform: translateY(-1.6rem);
		}
		100% {
			transform: translateY(9rem);
		}
	}
	@keyframes blink {
		50% {
			opacity: 0;
		}
	}
	@keyframes shimmer {
		from {
			transform: translateX(-100%);
		}
		to {
			transform: translateX(100%);
		}
	}
	@keyframes pulse {
		50% {
			transform: scale(1.6);
			opacity: 0.4;
		}
	}
	@keyframes spin {
		to {
			transform: rotate(360deg);
		}
	}

	@media (max-width: 767px) {
		.grid {
			grid-template-columns: 1fr;
			row-gap: 3rem;
			margin-top: 3rem;
		}
		.s1,
		.s2,
		.s3 {
			margin-top: 0;
		}
		.a-lead,
		.a-tail {
			inset-inline-start: var(--lane-in);
		}
		.a-knot,
		.s1 .a-knot,
		.s3 .a-knot {
			inset-inline-start: 0;
		}
	}
</style>
