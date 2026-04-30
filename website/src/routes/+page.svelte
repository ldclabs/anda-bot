<script lang="ts">
	import NexusCanvas from '$lib/components/landing/NexusCanvas.svelte';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { Card } from '$lib/components/ui/card';
	import {
		ArrowRight,
		BrainCircuit,
		Clock3,
		Database,
		Download,
		GitBranch,
		KeyRound,
		MessageSquare,
		Mic,
		Network,
		ShieldCheck,
		Sparkles,
		Terminal,
		Workflow
	} from '@lucide/svelte';

	const recallSignals = ['people', 'projects', 'preferences', 'decisions', 'files', 'channels'];
</script>

<svelte:head>
	<title>Anda Bot - A long-term memory agent</title>
	<meta
		name="description"
		content="Anda Bot is an open-source agent with a living long-term memory brain powered by Anda Hippocampus."
	/>
	<meta property="og:title" content="Anda Bot" />
	<meta
		property="og:description"
		content="I remember your context, recall it when it matters, and help you work across chat, voice, files, tools, and channels."
	/>
</svelte:head>

<main class="min-h-screen overflow-hidden bg-[color:var(--anda-ink)] text-[color:var(--anda-parchment)]">
	<section class="relative isolate min-h-[88svh] overflow-hidden border-b border-white/10">
		<NexusCanvas />
		<div class="absolute inset-0 bg-[linear-gradient(90deg,rgba(7,17,15,0.95)_0%,rgba(7,17,15,0.78)_40%,rgba(7,17,15,0.18)_100%)]"></div>
		<div class="anda-grain absolute inset-0 opacity-55"></div>
		<div class="absolute inset-x-0 bottom-0 h-36 bg-gradient-to-t from-[color:var(--anda-ink)] to-transparent"></div>

		<header class="relative z-20 mx-auto flex w-full max-w-7xl items-center justify-between px-5 py-5 sm:px-6 lg:px-8">
			<a href="/" class="group inline-flex items-center gap-3 text-sm font-semibold tracking-wide text-white">
				<span class="grid size-9 place-items-center rounded-md border border-[color:rgba(241,166,78,0.42)] bg-[color:rgba(241,166,78,0.15)] text-[color:var(--anda-amber-soft)] shadow-[0_0_38px_rgba(241,166,78,0.24)] transition group-hover:scale-105">
					<BrainCircuit class="size-5" />
				</span>
				Anda Bot
			</a>

			<nav class="hidden items-center gap-1 rounded-md border border-white/10 bg-white/[0.055] p-1 text-sm text-white/72 backdrop-blur-xl md:flex">
				<a href="#memory" class="rounded px-3 py-2 transition hover:bg-white/8 hover:text-white">Memory</a>
				<a href="#work" class="rounded px-3 py-2 transition hover:bg-white/8 hover:text-white">Workflows</a>
				<a href="#start" class="rounded px-3 py-2 transition hover:bg-white/8 hover:text-white">Start</a>
			</nav>

			<Button href="https://github.com/ldclabs/anda-bot" target="_blank" rel="noreferrer" variant="ghost" size="sm">
				<GitBranch class="size-4" />
				GitHub
			</Button>
		</header>

		<div class="relative z-10 mx-auto grid w-full max-w-7xl gap-10 px-5 pb-10 pt-8 sm:px-6 sm:pt-20 lg:grid-cols-[1.02fr_0.82fr] lg:px-8 lg:pb-16 lg:pt-24">
			<div class="max-w-3xl">
				<Badge class="mb-6 gap-2">
					<Sparkles class="size-3.5" />
					Long-term memory agent
				</Badge>

				<h1 class="anda-display max-w-3xl text-5xl leading-[0.98] tracking-normal text-white sm:text-6xl lg:text-7xl">
					Anda Bot
				</h1>
				<p class="mt-6 max-w-2xl text-lg leading-8 text-white/78 sm:text-2xl sm:leading-9">
					I am the agent that remembers. My Hippocampus brain keeps the thread of your work alive, so every conversation can build on what came before.
				</p>

				<div class="mt-8 flex flex-col gap-3 sm:flex-row">
					<Button href="#start">
						<Download class="size-4" />
						Run me locally
					</Button>
					<Button href="https://github.com/ldclabs/anda-hippocampus" target="_blank" rel="noreferrer" variant="secondary">
						<BrainCircuit class="size-4" />
						Meet my memory
					</Button>
				</div>

				<div class="mt-8 grid max-w-2xl grid-cols-3 gap-3 text-sm text-white/68 sm:mt-10">
					<div class="rounded-lg border border-white/10 bg-white/[0.055] p-3 backdrop-blur-xl sm:p-4">
						<strong class="block text-2xl font-semibold text-white">3</strong>
						memory phases
					</div>
					<div class="rounded-lg border border-white/10 bg-white/[0.055] p-3 backdrop-blur-xl sm:p-4">
						<strong class="block text-2xl font-semibold text-white">local</strong>
						workspace
					</div>
					<div class="rounded-lg border border-white/10 bg-white/[0.055] p-3 backdrop-blur-xl sm:p-4">
						<strong class="block text-2xl font-semibold text-white">open</strong>
						source
					</div>
				</div>
			</div>

			<div class="relative hidden self-end lg:block lg:pl-8">
				<div class="absolute -inset-8 rounded-full bg-[color:rgba(53,178,171,0.13)] blur-3xl"></div>
				<div class="relative overflow-hidden rounded-lg border border-white/12 bg-[rgba(12,23,20,0.72)] shadow-[0_32px_110px_rgba(0,0,0,0.38)] backdrop-blur-2xl">
					<div class="flex items-center justify-between border-b border-white/10 px-5 py-4">
						<div class="flex items-center gap-2 text-sm font-medium text-white/78">
							<Database class="size-4 text-[color:var(--anda-teal)]" />
							hippocampus.stream
						</div>
						<span class="rounded bg-[color:rgba(53,178,171,0.14)] px-2 py-1 text-xs text-[color:var(--anda-teal)]">awake</span>
					</div>

					<div class="space-y-5 p-5">
						<div class="rounded-lg border border-[color:rgba(241,166,78,0.28)] bg-[color:rgba(241,166,78,0.08)] p-4">
							<div class="mb-3 flex items-center justify-between text-xs uppercase tracking-[0.18em] text-[color:var(--anda-amber-soft)]">
								<span>recall pulse</span>
								<span>38ms</span>
							</div>
							<div class="h-2 overflow-hidden rounded-full bg-white/10">
								<div class="h-full w-[74%] rounded-full bg-[linear-gradient(90deg,var(--anda-amber),var(--anda-teal))]"></div>
							</div>
						</div>

						<div class="grid grid-cols-2 gap-2">
							{#each recallSignals as signal}
								<span class="rounded-md border border-white/10 bg-white/[0.06] px-3 py-2 text-sm text-white/72">{signal}</span>
							{/each}
						</div>

						<div class="rounded-lg border border-white/10 bg-black/22 p-4 font-mono text-sm text-white/72">
							<p class="text-[color:var(--anda-teal)]">$ anda chat</p>
							<p class="mt-3">I remember the project brief, your preferred tone, and the decision we made last week.</p>
						</div>
					</div>
				</div>
			</div>
		</div>
	</section>

	<section id="memory" class="relative z-10 border-b border-white/10 bg-[color:var(--anda-ink)] px-5 py-16 sm:px-6 lg:px-8">
		<div class="mx-auto max-w-7xl">
			<div class="grid gap-8 lg:grid-cols-[0.8fr_1.2fr] lg:items-end">
				<div>
					<Badge tone="cool">Anda Hippocampus</Badge>
					<h2 class="anda-display mt-5 max-w-xl text-4xl leading-tight text-white sm:text-5xl">My memory is not a note pile. It is a living loop.</h2>
				</div>
				<p class="max-w-2xl text-lg leading-8 text-white/68 lg:justify-self-end">
					I form memories from useful context, recall them when a new task needs grounding, and maintain them while you sleep so old noise fades and durable knowledge stays reachable.
				</p>
			</div>

			<div class="mt-10 grid gap-4 md:grid-cols-3">
				<Card class="p-6">
					<div class="mb-5 grid size-11 place-items-center rounded-md bg-[color:rgba(241,166,78,0.14)] text-[color:var(--anda-amber-soft)]">
						<Workflow class="size-5" />
					</div>
					<h3 class="text-xl font-semibold text-white">Formation</h3>
					<p class="mt-3 leading-7 text-white/66">I turn conversations, files, channels, and tool results into memories with time, source, and meaning.</p>
				</Card>

				<Card class="p-6">
					<div class="mb-5 grid size-11 place-items-center rounded-md bg-[color:rgba(53,178,171,0.14)] text-[color:var(--anda-teal)]">
						<BrainCircuit class="size-5" />
					</div>
					<h3 class="text-xl font-semibold text-white">Recall</h3>
					<p class="mt-3 leading-7 text-white/66">When you ask, I retrieve the pieces that explain what matters now, not merely the nearest text match.</p>
				</Card>

				<Card class="p-6">
					<div class="mb-5 grid size-11 place-items-center rounded-md bg-[color:rgba(216,75,66,0.14)] text-[color:var(--anda-clay)]">
						<Clock3 class="size-5" />
					</div>
					<h3 class="text-xl font-semibold text-white">Maintenance</h3>
					<p class="mt-3 leading-7 text-white/66">I consolidate, refresh, and prune so my brain can stay useful across months of real work.</p>
				</Card>
			</div>
		</div>
	</section>

	<section id="work" class="relative border-b border-white/10 bg-[#0d1815] px-5 py-16 sm:px-6 lg:px-8">
		<div class="absolute inset-0 bg-[radial-gradient(circle_at_78%_24%,rgba(241,166,78,0.13),transparent_32%),radial-gradient(circle_at_22%_78%,rgba(53,178,171,0.12),transparent_34%)]"></div>
		<div class="relative mx-auto grid max-w-7xl gap-10 lg:grid-cols-[1fr_1fr] lg:items-center">
			<div>
				<Badge tone="ink">Where I work</Badge>
				<h2 class="anda-display mt-5 max-w-xl text-4xl leading-tight text-white sm:text-5xl">Bring me to the places your context is born.</h2>
				<p class="mt-5 max-w-2xl text-lg leading-8 text-white/68">
					I can live in the terminal, join your chat channels, listen and speak by voice, read attachments, run local tools, and keep scheduled rituals moving.
				</p>
				<div class="mt-8 flex flex-wrap gap-3">
					<Badge tone="cool" class="gap-2"><MessageSquare class="size-3.5" /> chat</Badge>
					<Badge tone="warm" class="gap-2"><Mic class="size-3.5" /> voice</Badge>
					<Badge tone="ink" class="gap-2"><Terminal class="size-3.5" /> shell</Badge>
					<Badge tone="cool" class="gap-2"><Network class="size-3.5" /> channels</Badge>
				</div>
			</div>

			<div class="grid gap-4 sm:grid-cols-2">
				<Card class="p-5">
					<Terminal class="mb-5 size-6 text-[color:var(--anda-amber-soft)]" />
					<h3 class="font-semibold text-white">Local-first shell</h3>
					<p class="mt-3 leading-7 text-white/64">A grounded workspace for files, commands, experiments, and daily automation.</p>
				</Card>
				<Card class="p-5">
					<MessageSquare class="mb-5 size-6 text-[color:var(--anda-teal)]" />
					<h3 class="font-semibold text-white">Multi-channel chat</h3>
					<p class="mt-3 leading-7 text-white/64">Discord, Telegram, WeChat, Lark, IRC, and the conversations around your work.</p>
				</Card>
				<Card class="p-5">
					<KeyRound class="mb-5 size-6 text-[color:var(--anda-clay)]" />
					<h3 class="font-semibold text-white">Your keys, your models</h3>
					<p class="mt-3 leading-7 text-white/64">Connect the model provider you trust and keep my runtime close to your machine.</p>
				</Card>
				<Card class="p-5">
					<ShieldCheck class="mb-5 size-6 text-[color:var(--anda-lichen)]" />
					<h3 class="font-semibold text-white">Memory you can inspect</h3>
					<p class="mt-3 leading-7 text-white/64">Open-source components make the shape of my brain visible and auditable.</p>
				</Card>
			</div>
		</div>
	</section>

	<section id="start" class="bg-[color:var(--anda-parchment)] px-5 py-16 text-[color:var(--anda-ink)] sm:px-6 lg:px-8">
		<div class="mx-auto grid max-w-7xl gap-10 lg:grid-cols-[0.9fr_1.1fr] lg:items-center">
			<div>
				<Badge tone="warm" class="border-[color:rgba(7,17,15,0.16)] bg-[color:rgba(7,17,15,0.06)] text-[color:var(--anda-ink)]">Start a persistent thread</Badge>
				<h2 class="anda-display mt-5 max-w-xl text-4xl leading-tight sm:text-5xl">Install me, give me a model, then talk to me like a teammate.</h2>
				<p class="mt-5 max-w-2xl text-lg leading-8 text-black/66">
					The first run creates my home directory and local workspace. From there, every useful session can become part of the memory I bring back next time.
				</p>
				<div class="mt-8 flex flex-col gap-3 sm:flex-row">
					<Button href="https://github.com/ldclabs/anda-bot" target="_blank" rel="noreferrer" class="bg-[color:var(--anda-ink)] text-[color:var(--anda-parchment)] shadow-[0_18px_46px_rgba(7,17,15,0.28)] hover:bg-[color:var(--anda-forest)]">
						<GitBranch class="size-4" />
						Open repository
					</Button>
					<Button href="https://github.com/ldclabs/anda-bot#quick-start" target="_blank" rel="noreferrer" variant="secondary" class="border-black/15 bg-black/[0.06] text-[color:var(--anda-ink)] backdrop-blur-none hover:bg-black/[0.1]">
						Quick start
						<ArrowRight class="size-4" />
					</Button>
				</div>
			</div>

			<div class="overflow-hidden rounded-lg border border-black/12 bg-[#101815] shadow-[0_28px_90px_rgba(7,17,15,0.22)]">
				<div class="flex items-center justify-between border-b border-white/10 px-5 py-4 text-sm text-white/62">
					<span>terminal</span>
					<span>~/.anda/workspace</span>
				</div>
				<pre class="whitespace-pre-wrap break-words p-5 text-sm leading-7 text-white/76"><code><span class="text-[color:var(--anda-teal)]">git clone</span> https://github.com/ldclabs/anda-bot
<span class="text-[color:var(--anda-teal)]">cd</span> anda-bot
<span class="text-[color:var(--anda-teal)]">cargo install</span> --path anda_bot

<span class="text-[color:var(--anda-amber-soft)]">anda</span> chat
<span class="text-white/42"># Ask me what to remember, where to work, and what to do next.</span></code></pre>
			</div>
		</div>
	</section>
</main>
