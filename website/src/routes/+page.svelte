<script lang="ts">
	import NexusCanvas from '$lib/components/landing/NexusCanvas.svelte';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { Card } from '$lib/components/ui/card';
	import Anda from '$lib/components/ui/icons/anda.svelte';
	import {
		ArrowRight,
		Brain,
		CheckCircle,
		Clock3,
		Command,
		Database,
		Download,
		Eye,
		GitBranch,
		KeyRound,
		MessageSquare,
		Network,
		Radio,
		RotateCcw,
		ShieldCheck,
		Terminal,
		Workflow,
		Zap
	} from '@lucide/svelte';

	const recallSignals = [
		{ label: 'identity', value: 'stable', level: 86 },
		{ label: 'projects', value: 'active', level: 72 },
		{ label: 'decisions', value: 'linked', level: 64 },
		{ label: 'channels', value: 'listening', level: 58 }
	];

	const liveEvents = [
		{ time: '00:01', phase: 'formation', detail: 'project brief anchored to workspace' },
		{ time: '00:18', phase: 'recall', detail: 'prior tone and decisions surfaced' },
		{ time: '03:40', phase: 'maintenance', detail: 'stale context faded, durable facts kept' }
	];

	const workSurfaces = [
		{ label: 'chat', detail: 'Daily threads, decisions, and questions stay connected.' },
		{ label: 'voice', detail: 'Spoken context can become part of the same working memory.' },
		{ label: 'shell', detail: 'Local commands and files give memory something real to stand on.' },
		{ label: 'channels', detail: 'Context born in teams can follow the work instead of vanishing.' }
	];
</script>

<svelte:head>
	<title>Anda Bot - The agent that remembers</title>
	<meta
		name="description"
		content="Anda Bot is an open-source long-term memory agent powered by Anda Hippocampus, built to keep the thread of your work alive across conversations, tools, files, and channels."
	/>
	<meta property="og:title" content="Anda Bot - The agent that remembers" />
	<meta
		property="og:description"
		content="Meet Anda Bot, an open-source agent with a living long-term memory brain powered by Anda Hippocampus."
	/>
</svelte:head>

<main
	class="observatory-shell min-h-screen overflow-x-clip bg-(--anda-ink) text-(--anda-parchment)"
>
	<section class="relative isolate min-h-[88svh] overflow-hidden border-b border-white/10">
		<NexusCanvas />
		<div class="hero-vignette absolute inset-0"></div>
		<div class="memory-strata absolute inset-0 opacity-80"></div>
		<div class="hero-scan absolute inset-0 opacity-30"></div>

		<header
			class="relative z-20 mx-auto flex w-full max-w-7xl items-center justify-between px-5 py-5 sm:px-6 lg:px-8"
		>
			<a href="/" class="group inline-flex items-center gap-3 text-sm font-semibold text-white">
				<span
					class="brand-mark grid size-12 place-items-center rounded-lg border border-[rgba(241,166,78,0.44)] bg-[rgba(53,178,171,0.35)] text-(--anda-amber-soft) shadow-[0_0_38px_rgba(241,166,78,0.2)] transition group-hover:scale-105"
				>
					<Anda class="size-10" />
				</span>
				<span class="text-2xl">Anda Bot</span>
			</a>

			<nav
				class="hidden items-center gap-1 rounded-lg border border-white/10 bg-black/20 p-1 text-sm text-white/74 backdrop-blur-xl md:flex"
			>
				<a href="#memory" class="rounded-md px-3 py-2 transition hover:bg-white/8 hover:text-white"
					>Memory</a
				>
				<a href="#work" class="rounded-md px-3 py-2 transition hover:bg-white/8 hover:text-white"
					>Surfaces</a
				>
				<a href="#start" class="rounded-md px-3 py-2 transition hover:bg-white/8 hover:text-white"
					>Start</a
				>
			</nav>

			<Button
				href="https://github.com/ldclabs/anda-bot"
				target="_blank"
				rel="noreferrer"
				variant="ghost"
				size="sm"
			>
				<GitBranch class="size-4" />
				GitHub
			</Button>
		</header>

		<div
			class="relative z-10 mx-auto grid w-full max-w-7xl gap-8 px-5 pt-8 pb-8 sm:px-6 sm:pt-14 lg:grid-cols-[minmax(0,0.82fr)_minmax(420px,0.9fr)] lg:gap-10 lg:px-8 lg:pt-20 lg:pb-12"
		>
			<div class="max-w-3xl self-center">
				<Badge tone="warm" class="mb-6 gap-2">
					<Radio class="size-3.5" />
					Long-term memory agent
				</Badge>

				<p class="mb-3 flex items-center gap-2 text-sm text-(--anda-muted)">
					<span class="h-px w-8 bg-(--anda-amber)"></span>
					hippocampus awake
				</p>
				<h1
					class="anda-display max-w-3xl text-5xl leading-[0.94] text-white sm:text-6xl lg:text-7xl"
				>
					Anda Bot
				</h1>
				<p class="mt-6 max-w-2xl text-lg leading-8 text-white/78 sm:text-2xl sm:leading-9">
					I am the agent that remembers. My Hippocampus brain keeps the thread of your work alive
					across conversations, tools, files, and channels.
				</p>

				<div class="mt-8 flex flex-col gap-3 sm:flex-row">
					<Button href="#start" size="lg">
						<Download class="size-4" />
						Run me locally
					</Button>
					<Button
						href="https://github.com/ldclabs/anda-hippocampus"
						target="_blank"
						rel="noreferrer"
						variant="secondary"
						size="lg"
					>
						<Brain class="size-4" />
						Meet my memory
					</Button>
				</div>

				<div class="mt-8 grid max-w-2xl grid-cols-3 gap-2 text-sm text-white/68 sm:gap-3">
					<div class="proof-tile">
						<strong class="block text-xl font-semibold text-white sm:text-2xl">3</strong>
						phases
					</div>
					<div class="proof-tile">
						<strong class="block text-xl font-semibold text-white sm:text-2xl">local</strong>
						workspace
					</div>
					<div class="proof-tile">
						<strong class="block text-xl font-semibold text-white sm:text-2xl">open</strong>
						source
					</div>
				</div>
			</div>

			<div class="hidden self-center lg:block">
				<div
					class="signal-panel relative overflow-hidden rounded-lg border border-white/12 bg-[rgba(7,17,15,0.54)] p-5 shadow-[0_32px_120px_rgba(0,0,0,0.44)] backdrop-blur-2xl"
				>
					<div
						class="relative z-10 flex items-center justify-between border-b border-white/10 pb-4"
					>
						<div class="flex items-center gap-2 text-sm font-medium text-white/78">
							<Database class="size-4 text-(--anda-teal)" />
							Anda Hippocampus
						</div>
						<span
							class="memory-pulse inline-flex items-center gap-2 rounded-full border border-[rgba(53,178,171,0.35)] bg-[rgba(53,178,171,0.1)] px-3 py-1 text-xs text-(--anda-teal)"
						>
							<span class="size-1.5 rounded-full bg-(--anda-teal)"></span>
							recalling
						</span>
					</div>

					<div class="relative z-10 grid gap-6 py-6 lg:grid-cols-[0.82fr_1fr]">
						<div class="phase-wheel mx-auto">
							<span class="phase-chip phase-chip-formation">formation</span>
							<span class="phase-chip phase-chip-recall">recall</span>
							<span class="phase-chip phase-chip-maintenance">maintenance</span>
							<div class="phase-core">
								<Brain class="size-20" />
							</div>
						</div>

						<div class="space-y-3">
							{#each recallSignals as signal}
								<div class="signal-row">
									<div class="flex items-center justify-between gap-3 text-xs text-white/58">
										<span>{signal.label}</span>
										<span>{signal.value}</span>
									</div>
									<div class="signal-track mt-2">
										<span class="signal-meter" style={`--level: ${signal.level}%`}></span>
									</div>
								</div>
							{/each}
						</div>
					</div>

					<div class="relative z-10 space-y-2 border-t border-white/10 pt-4">
						{#each liveEvents as event}
							<div
								class="hero-log-line grid grid-cols-[52px_92px_1fr] gap-3 rounded-md border border-white/8 bg-white/4.5 px-3 py-2 text-xs text-white/62"
							>
								<span class="font-mono text-(--anda-amber-soft)">{event.time}</span>
								<span class="text-(--anda-teal)">{event.phase}</span>
								<span>{event.detail}</span>
							</div>
						{/each}
					</div>
				</div>
			</div>
		</div>
	</section>

	<section
		id="memory"
		class="relative z-10 border-b border-white/10 bg-(--anda-ink) px-5 py-16 sm:px-6 lg:px-8 lg:py-24"
	>
		<div class="mx-auto max-w-7xl">
			<div class="grid gap-8 lg:grid-cols-[0.72fr_1fr] lg:items-end">
				<div>
					<Badge tone="cool" class="gap-2">
						<Brain class="size-3.5" />
						Anda Hippocampus
					</Badge>
					<h2 class="anda-display mt-5 max-w-xl text-4xl leading-tight text-white sm:text-5xl">
						My memory is not a note pile. It is a living loop.
					</h2>
				</div>
				<p class="max-w-2xl text-lg leading-8 text-white/68 lg:justify-self-end">
					I form memories from useful context, recall the pieces that matter now, and maintain them
					over time so knowledge can survive beyond a single context window.
				</p>
			</div>

			<div class="memory-loop mt-10 grid gap-4 lg:grid-cols-3 lg:gap-5">
				<Card class="phase-card p-6">
					<div class="mb-6 flex items-center justify-between">
						<span class="phase-number">01</span>
						<div
							class="grid size-11 place-items-center rounded-lg bg-[rgba(241,166,78,0.14)] text-(--anda-amber-soft)"
						>
							<Workflow class="size-5" />
						</div>
					</div>
					<h3 class="text-xl font-semibold text-white">Formation</h3>
					<p class="mt-3 leading-7 text-white/66">
						I turn conversations, files, channels, and tool results into memories with source, time,
						and meaning.
					</p>
				</Card>

				<Card class="phase-card phase-card-raised p-6">
					<div class="mb-6 flex items-center justify-between">
						<span class="phase-number">02</span>
						<div
							class="grid size-11 place-items-center rounded-lg bg-[rgba(53,178,171,0.14)] text-(--anda-teal)"
						>
							<Zap class="size-5" />
						</div>
					</div>
					<h3 class="text-xl font-semibold text-white">Recall</h3>
					<p class="mt-3 leading-7 text-white/66">
						When you ask, I retrieve the context that explains the present task instead of matching
						nearest text alone.
					</p>
				</Card>

				<Card class="phase-card p-6">
					<div class="mb-6 flex items-center justify-between">
						<span class="phase-number">03</span>
						<div
							class="grid size-11 place-items-center rounded-lg bg-[rgba(216,75,66,0.14)] text-(--anda-clay)"
						>
							<RotateCcw class="size-5" />
						</div>
					</div>
					<h3 class="text-xl font-semibold text-white">Maintenance</h3>
					<p class="mt-3 leading-7 text-white/66">
						I consolidate, refresh, and prune so durable knowledge remains reachable while old noise
						fades.
					</p>
				</Card>
			</div>
		</div>
	</section>

	<section
		id="work"
		class="work-band relative border-b border-white/10 px-5 py-16 sm:px-6 lg:px-8 lg:py-24"
	>
		<div class="memory-strata absolute inset-0 opacity-35"></div>
		<div
			class="relative mx-auto grid max-w-7xl gap-10 lg:grid-cols-[1.05fr_0.95fr] lg:items-center"
		>
			<div
				class="context-board overflow-hidden rounded-lg border border-white/10 bg-black/24 p-4 shadow-[0_28px_100px_rgba(0,0,0,0.28)] backdrop-blur-xl sm:p-5"
			>
				<div
					class="mb-4 flex items-center justify-between border-b border-white/10 pb-4 text-sm text-white/60"
				>
					<span class="inline-flex items-center gap-2"
						><Network class="size-4 text-(--anda-teal)" /> context routes</span
					>
					<span class="font-mono text-(--anda-amber-soft)">anda://memory</span>
				</div>

				<div class="grid gap-3 sm:grid-cols-2">
					{#each workSurfaces as surface}
						<article class="context-lane">
							<span class="context-lane-rule"></span>
							<h3 class="text-lg font-semibold text-white">{surface.label}</h3>
							<p class="mt-2 leading-7 text-white/62">{surface.detail}</p>
						</article>
					{/each}
				</div>
			</div>

			<div>
				<Badge tone="ink" class="gap-2">
					<Command class="size-3.5" />
					Where I work
				</Badge>
				<h2 class="anda-display mt-5 max-w-xl text-4xl leading-tight text-white sm:text-5xl">
					Bring me to the places your context is born.
				</h2>
				<p class="mt-5 max-w-2xl text-lg leading-8 text-white/68">
					I can live in the terminal, join your chat channels, listen and speak by voice, read
					files, run tools, and keep scheduled rituals moving.
				</p>

				<div class="mt-8 grid gap-3 sm:grid-cols-2">
					<Card class="work-card p-5" size="sm">
						<Terminal class="mb-4 size-6 text-(--anda-amber-soft)" />
						<h3 class="font-semibold text-white">Local-first shell</h3>
						<p class="mt-2 leading-7 text-white/62">
							A grounded workspace for commands, files, experiments, and automation.
						</p>
					</Card>
					<Card class="work-card p-5" size="sm">
						<MessageSquare class="mb-4 size-6 text-(--anda-teal)" />
						<h3 class="font-semibold text-white">Multi-channel chat</h3>
						<p class="mt-2 leading-7 text-white/62">
							Conversations around the work can become part of the working thread.
						</p>
					</Card>
					<Card class="work-card p-5" size="sm">
						<KeyRound class="mb-4 size-6 text-(--anda-clay)" />
						<h3 class="font-semibold text-white">Your keys and models</h3>
						<p class="mt-2 leading-7 text-white/62">
							Connect the provider you trust and keep the runtime close to your machine.
						</p>
					</Card>
					<Card class="work-card p-5" size="sm">
						<ShieldCheck class="mb-4 size-6 text-(--anda-lichen)" />
						<h3 class="font-semibold text-white">Inspectable memory</h3>
						<p class="mt-2 leading-7 text-white/62">
							Open-source components keep the shape of my brain visible and auditable.
						</p>
					</Card>
				</div>
			</div>
		</div>
	</section>

	<section id="start" class="start-band px-5 py-16 text-(--anda-ink) sm:px-6 lg:px-8 lg:py-24">
		<div class="mx-auto grid max-w-7xl gap-10 lg:grid-cols-[0.86fr_1.14fr] lg:items-center">
			<div>
				<Badge
					tone="warm"
					class="border-[rgba(7,17,15,0.16)] bg-[rgba(7,17,15,0.06)] text-(--anda-ink)"
				>
					Start a persistent thread
				</Badge>
				<h2 class="anda-display mt-5 max-w-xl text-4xl leading-tight sm:text-5xl">
					Install me, give me a model, then talk to me like a teammate.
				</h2>
				<p class="mt-5 max-w-2xl text-lg leading-8 text-black/66">
					The first run creates my home directory and local workspace. From there, every useful
					session can become part of the memory I bring back next time.
				</p>

				<div class="mt-8 flex flex-col gap-3 sm:flex-row">
					<Button
						href="https://github.com/ldclabs/anda-bot"
						target="_blank"
						rel="noreferrer"
						class="bg-(--anda-ink) text-(--anda-parchment) shadow-[0_18px_46px_rgba(7,17,15,0.28)] hover:bg-(--anda-forest)"
						size="lg"
					>
						<GitBranch class="size-4" />
						Open repository
					</Button>
					<Button
						href="https://github.com/ldclabs/anda-bot#quick-start"
						target="_blank"
						rel="noreferrer"
						variant="secondary"
						class="border-black/15 bg-black/6 text-(--anda-ink) backdrop-blur-none hover:bg-black/10"
						size="lg"
					>
						Quick start
						<ArrowRight class="size-4" />
					</Button>
				</div>
			</div>

			<div
				class="terminal-panel overflow-hidden rounded-lg border border-black/12 bg-[#101815] shadow-[0_28px_90px_rgba(7,17,15,0.22)]"
			>
				<div
					class="flex items-center justify-between border-b border-white/10 px-5 py-4 text-sm text-white/62"
				>
					<span class="inline-flex items-center gap-2"
						><Terminal class="size-4 text-(--anda-teal)" /> terminal</span
					>
					<span>~/.anda</span>
				</div>
				<pre class="p-5 text-sm leading-7 wrap-break-word whitespace-pre-wrap text-white/76"><code
						><span class="text-(--anda-teal)">git clone</span> https://github.com/ldclabs/anda-bot
<span class="text-(--anda-teal)">cd</span> anda-bot
<span class="text-(--anda-teal)">cargo install</span>

<span class="text-(--anda-amber-soft)">anda</span> chat
<span class="text-white/42"># Ask me what to remember, where to work, and what to do next.</span
						></code
					></pre>
				<div class="grid gap-2 border-t border-white/10 p-5 text-sm text-white/62 sm:grid-cols-3">
					<span class="inline-flex items-center gap-2"
						><CheckCircle class="size-4 text-(--anda-teal)" /> local runtime</span
					>
					<span class="inline-flex items-center gap-2"
						><Clock3 class="size-4 text-(--anda-amber-soft)" /> durable thread</span
					>
					<span class="inline-flex items-center gap-2"
						><Eye class="size-4 text-(--anda-lichen)" /> inspectable brain</span
					>
				</div>
			</div>
		</div>
	</section>
</main>
