<script lang="ts">
	import NexusCanvas from '$lib/components/landing/NexusCanvas.svelte';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { Card } from '$lib/components/ui/card';
	import Anda from '$lib/components/ui/icons/anda.svelte';
	import Github from '$lib/components/ui/icons/github.svelte';
	import {
		detectLocale,
		fallbackLocale,
		isLocale,
		landingCopy,
		localeMeta,
		localeOrder,
		type Locale,
		type OsKey
	} from '$lib/content/landing';
	import {
		ArrowRight,
		Brain,
		CheckCircle,
		Clock3,
		Command,
		Copy,
		Database,
		Download,
		Eye,
		Infinity,
		KeyRound,
		Languages,
		MessageSquare,
		Network,
		RotateCcw,
		ShieldCheck,
		Terminal,
		Workflow,
		Zap
	} from '@lucide/svelte';
	import { onMount } from 'svelte';

	const installCommands: Record<OsKey, { command: string; fallback?: string }> = {
		macos: {
			command:
				'curl -fsSL https://raw.githubusercontent.com/ldclabs/anda-bot/main/scripts/install.sh | sh',
			fallback: 'brew install ldclabs/tap/anda'
		},
		windows: {
			command:
				'irm https://raw.githubusercontent.com/ldclabs/anda-bot/main/scripts/install.ps1 | iex'
		},
		linux: {
			command:
				'curl -fsSL https://raw.githubusercontent.com/ldclabs/anda-bot/main/scripts/install.sh | sh'
		}
	};
	const runCommands: Record<OsKey, string[]> = {
		macos: ['DEEPSEEK_API_KEY=**** anda'],
		windows: ['$env:DEEPSEEK_API_KEY="****"; anda'],
		linux: ['DEEPSEEK_API_KEY=**** anda']
	};

	const installOrder: OsKey[] = ['macos', 'windows', 'linux'];
	const localeStorageKey = 'anda-bot-landing-locale';

	let activeLocale = $state<Locale>(fallbackLocale);
	let activeOs = $state<OsKey>('macos');
	let detectedOs = $state<OsKey | null>(null);
	let copyState = $state<'idle' | 'copied' | 'failed'>('idle');
	let copyResetTimer: ReturnType<typeof setTimeout> | undefined;
	let copy = $derived(landingCopy[activeLocale]);
	let activeDirection = $derived(localeMeta[activeLocale].dir);
	let activeInstallText = $derived(copy.install.options[activeOs]);
	let activeInstall = $derived({ ...installCommands[activeOs], ...activeInstallText });
	let activeRunCommands = $derived(runCommands[activeOs]);

	$effect(() => {
		document.documentElement.lang = localeMeta[activeLocale].htmlLang;
		document.documentElement.dir = localeMeta[activeLocale].dir;
	});

	function detectOs(value: string): OsKey | null {
		const normalized = value.toLowerCase();
		if (normalized.includes('win')) return 'windows';
		if (normalized.includes('mac')) return 'macos';
		if (
			normalized.includes('linux') ||
			normalized.includes('x11') ||
			normalized.includes('ubuntu') ||
			normalized.includes('debian') ||
			normalized.includes('fedora')
		) {
			return 'linux';
		}

		return null;
	}

	function formatTemplate(template: string, values: Record<string, string>) {
		return template.replace(/\{(\w+)\}/g, (_, key: string) => values[key] ?? '');
	}

	function installFor(osLabel: string) {
		return formatTemplate(copy.hero.installFor, { os: osLabel });
	}

	function detectedLabel(os: OsKey) {
		return formatTemplate(copy.install.detected, { os: copy.install.options[os].label });
	}

	function alternativeLabel(method: string) {
		return formatTemplate(copy.install.alternative, { method });
	}

	function selectLocale(locale: Locale) {
		activeLocale = locale;
		copyState = 'idle';
		try {
			localStorage.setItem(localeStorageKey, locale);
		} catch {
			// Ignore private browsing storage failures.
		}
	}

	function selectOs(os: OsKey) {
		activeOs = os;
		copyState = 'idle';
	}

	function resetCopyFeedback(state: typeof copyState) {
		copyState = state;
		if (copyResetTimer) clearTimeout(copyResetTimer);
		copyResetTimer = setTimeout(() => {
			copyState = 'idle';
		}, 1800);
	}

	function copyWithFallback(text: string) {
		const textarea = document.createElement('textarea');
		textarea.value = text;
		textarea.setAttribute('readonly', '');
		textarea.style.position = 'fixed';
		textarea.style.opacity = '0';
		document.body.appendChild(textarea);
		textarea.select();
		const copied = document.execCommand('copy');
		document.body.removeChild(textarea);
		if (!copied) throw new Error('copy failed');
	}

	async function copyInstallCommand() {
		try {
			if (navigator.clipboard?.writeText) {
				await navigator.clipboard.writeText(activeInstall.command);
			} else {
				copyWithFallback(activeInstall.command);
			}
			resetCopyFeedback('copied');
		} catch {
			resetCopyFeedback('failed');
		}
	}

	onMount(() => {
		let storedLocale: string | null = null;
		try {
			storedLocale = localStorage.getItem(localeStorageKey);
		} catch {
			storedLocale = null;
		}

		if (isLocale(storedLocale)) {
			activeLocale = storedLocale;
		} else {
			activeLocale = detectLocale(
				navigator.languages?.length ? navigator.languages : [navigator.language]
			);
		}

		const nav = navigator as Navigator & { userAgentData?: { platform?: string } };
		const platform = nav.userAgentData?.platform ?? navigator.platform ?? '';
		const detected = detectOs(`${platform} ${navigator.userAgent}`);
		if (!detected) return;

		detectedOs = detected;
		activeOs = detected;
	});
</script>

<svelte:head>
	<title>{copy.meta.title}</title>
	<meta name="description" content={copy.meta.description} />
	<meta property="og:title" content={copy.meta.ogTitle} />
	<meta property="og:description" content={copy.meta.ogDescription} />
	<meta content="website" property="og:type" />
	<meta content="https://anda.bot" property="og:url" />
	<meta content="https://anda.bot/_assets/images/anda_bot.webp" property="og:image" />
	<meta content="summary_large_image" name="twitter:card" />
	<meta content="@ICPandaDAO" name="twitter:creator" />
</svelte:head>

<main
	dir={activeDirection}
	class="observatory-shell min-h-screen overflow-x-clip bg-(--anda-ink) text-(--anda-parchment)"
>
	<section class="relative isolate min-h-[92svh] overflow-hidden border-b border-white/10">
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
				<a href="#install" class="rounded-md px-3 py-2 transition hover:bg-white/8 hover:text-white"
					>{copy.nav.install}</a
				>
				<a
					href="#reasoning"
					class="rounded-md px-3 py-2 transition hover:bg-white/8 hover:text-white"
					>{copy.nav.reasoning}</a
				>
				<a href="#memory" class="rounded-md px-3 py-2 transition hover:bg-white/8 hover:text-white"
					>{copy.nav.memory}</a
				>
				<a href="#work" class="rounded-md px-3 py-2 transition hover:bg-white/8 hover:text-white"
					>{copy.nav.surfaces}</a
				>
			</nav>

			<div class="header-actions">
				<label class="language-switcher">
					<Languages class="size-4" />
					<span class="sr-only">{copy.language.label}</span>
					<select
						aria-label={copy.language.label}
						value={activeLocale}
						onchange={(event) =>
							selectLocale((event.currentTarget as HTMLSelectElement).value as Locale)}
					>
						{#each localeOrder as locale}
							<option value={locale}>{localeMeta[locale].nativeName}</option>
						{/each}
					</select>
				</label>

				<Button
					href="https://github.com/ldclabs/anda-bot"
					target="_blank"
					rel="noreferrer"
					variant="ghost"
					size="sm"
				>
					<Github class="size-5" />
					GitHub
				</Button>
			</div>
		</header>

		<div
			class="relative z-10 mx-auto grid w-full max-w-7xl gap-8 px-5 pt-8 pb-10 sm:px-6 sm:pt-14 lg:grid-cols-[minmax(0,0.8fr)_minmax(430px,0.92fr)] lg:gap-10 lg:px-8 lg:pt-16 lg:pb-14"
		>
			<div class="max-w-3xl self-center">
				<Badge tone="warm" class="mb-6 gap-2">
					<Download class="size-3.5" />
					{copy.hero.badge}
				</Badge>

				<p class="mb-3 flex items-center gap-2 text-sm text-(--anda-muted)">
					<span class="h-px w-8 bg-(--anda-amber)"></span>
					{copy.hero.eyebrow}
				</p>
				<h1
					class="anda-display max-w-3xl text-5xl leading-[0.94] text-white sm:text-6xl lg:text-7xl"
				>
					{copy.hero.title}
				</h1>
				<p class="mt-6 max-w-2xl text-lg leading-8 text-white/78 sm:text-2xl sm:leading-9">
					{copy.hero.body}
				</p>

				<div class="mt-8 flex flex-col gap-3 sm:flex-row">
					<Button href="#install" size="lg">
						<Download class="size-4" />
						{installFor(activeInstall.label)}
					</Button>
					<Button href="#reasoning" variant="secondary" size="lg">
						<Infinity class="size-4" />
						{copy.hero.seeReasoning}
					</Button>
				</div>

				<div class="mt-8 grid max-w-2xl grid-cols-3 gap-2 text-sm text-white/68 sm:gap-3">
					<div class="proof-tile">
						<strong class="block text-xl font-semibold text-white sm:text-2xl"
							>{copy.hero.proofOs}</strong
						>
						{copy.hero.proofOsText}
					</div>
					<div class="proof-tile">
						<strong class="block text-xl font-semibold text-white sm:text-2xl"
							>{copy.hero.proofReasoning}</strong
						>
						{copy.hero.proofReasoningText}
					</div>
					<div class="proof-tile">
						<strong class="block text-xl font-semibold text-white sm:text-2xl"
							>{copy.hero.proofMemory}</strong
						>
						{copy.hero.proofMemoryText}
					</div>
				</div>
			</div>

			<div id="install" class="install-console self-center">
				<div
					class="relative z-10 flex items-center justify-between gap-4 border-b border-white/10 pb-4"
				>
					<div>
						<p class="text-xs font-medium tracking-[0.16em] text-(--anda-amber-soft) uppercase">
							{copy.install.eyebrow}
						</p>
						<h2 class="mt-2 text-2xl font-semibold text-white sm:text-3xl">
							{copy.install.title}
						</h2>
					</div>
					<span
						class="memory-pulse inline-flex shrink-0 items-center gap-2 rounded-full border border-[rgba(53,178,171,0.35)] bg-[rgba(53,178,171,0.1)] px-3 py-1 text-xs text-(--anda-teal)"
					>
						<span class="size-1.5 rounded-full bg-(--anda-teal)"></span>
						{detectedOs ? detectedLabel(detectedOs) : copy.install.chooseOs}
					</span>
				</div>

				<div
					class="relative z-10 mt-5 grid gap-3 sm:grid-cols-3"
					role="tablist"
					aria-label={copy.install.tabAria}
				>
					{#each installOrder as os}
						<button
							type="button"
							role="tab"
							aria-selected={activeOs === os}
							class={`install-tab ${activeOs === os ? 'install-tab-active' : ''}`}
							onclick={() => selectOs(os)}
						>
							<span>{copy.install.options[os].label}</span>
							{#if detectedOs === os}
								<small>{detectedLabel(os)}</small>
							{/if}
						</button>
					{/each}
				</div>

				<div
					class="relative z-10 mt-5 overflow-hidden rounded-lg border border-white/10 bg-[#08110f]"
				>
					<div
						class="flex items-center justify-between border-b border-white/10 px-4 py-3 text-sm text-white/58"
					>
						<span class="inline-flex items-center gap-2">
							<Terminal class="size-4 text-(--anda-teal)" />
							{activeInstall.commandLabel}
						</span>
						<div class="flex items-center gap-2">
							<span>{activeInstall.label}</span>
							<button
								type="button"
								class="copy-command-button"
								aria-label={copy.install.copyAria}
								onclick={() => void copyInstallCommand()}
							>
								{#if copyState === 'copied'}
									<CheckCircle class="size-3.5" />
									{copy.install.copied}
								{:else if copyState === 'failed'}
									<Copy class="size-3.5" />
									{copy.install.copyFailed}
								{:else}
									<Copy class="size-3.5" />
									{copy.install.copy}
								{/if}
							</button>
						</div>
					</div>
					<button
						type="button"
						class="install-command"
						aria-label={copy.install.commandAria}
						onclick={() => void copyInstallCommand()}><code>{activeInstall.command}</code></button
					>
				</div>

				{#if activeInstall.fallback && activeInstall.fallbackLabel}
					<div class="relative z-10 mt-3 rounded-lg border border-white/10 bg-white/4.5 p-4">
						<p class="text-xs font-medium text-white/52">
							{alternativeLabel(activeInstall.fallbackLabel)}
						</p>
						<code class="mt-2 block font-mono text-sm leading-6 wrap-break-word text-white/76">
							{activeInstall.fallback}
						</code>
					</div>
				{/if}

				<div class="relative z-10 mt-5 grid gap-3 text-sm text-white/68 sm:grid-cols-3">
					<div class="install-step">
						<CheckCircle class="size-4 text-(--anda-teal)" />
						<span>{copy.install.steps[0]}</span>
					</div>
					<div class="install-step">
						<KeyRound class="size-4 text-(--anda-amber-soft)" />
						<span>{copy.install.steps[1]}</span>
					</div>
					<div class="install-step">
						<Command class="size-4 text-(--anda-lichen)" />
						<span>{copy.install.steps[2]} <code>anda</code></span>
					</div>
				</div>

				<p class="relative z-10 mt-4 text-sm leading-6 text-white/58">{activeInstall.note}</p>
				<p class="relative z-10 mt-2 text-sm leading-6 text-white/58">
					{copy.install.requiresPrefix} <code>~/.anda/config.yaml</code>
					{copy.install.requiresSuffix}
				</p>
			</div>
		</div>
	</section>

	<section
		id="reasoning"
		class="relative z-10 border-b border-white/10 bg-(--anda-ink) px-5 py-16 sm:px-6 lg:px-8 lg:py-24"
	>
		<div class="mx-auto max-w-7xl">
			<div class="grid gap-8 lg:grid-cols-[0.74fr_1fr] lg:items-end">
				<div>
					<Badge tone="warm" class="gap-2">
						<Infinity class="size-3.5" />
						{copy.reasoning.badge}
					</Badge>
					<h2 class="anda-display mt-5 max-w-2xl text-4xl leading-tight text-white sm:text-5xl">
						{copy.reasoning.title}
					</h2>
				</div>
				<p class="max-w-2xl text-lg leading-8 text-white/68 lg:justify-self-end">
					{copy.reasoning.body}
				</p>
			</div>

			<div class="reasoning-grid mt-10 grid gap-4 lg:grid-cols-[0.95fr_1.05fr] lg:gap-5">
				<div
					class="signal-panel relative overflow-hidden rounded-lg border border-white/12 bg-[rgba(7,17,15,0.54)] p-5 shadow-[0_32px_120px_rgba(0,0,0,0.36)] backdrop-blur-2xl"
				>
					<div
						class="relative z-10 flex items-center justify-between border-b border-white/10 pb-4"
					>
						<div class="flex items-center gap-2 text-sm font-medium text-white/78">
							<Database class="size-4 text-(--anda-teal)" />
							{copy.reasoning.panelTitle}
						</div>
						<span class="font-mono text-xs text-(--anda-amber-soft)"
							>{copy.reasoning.panelStatus}</span
						>
					</div>

					<div class="relative z-10 grid gap-6 py-6 lg:grid-cols-[0.82fr_1fr]">
						<div class="phase-wheel mx-auto">
							<span class="phase-chip phase-chip-formation">{copy.reasoning.phases[0]}</span>
							<span class="phase-chip phase-chip-recall">{copy.reasoning.phases[1]}</span>
							<span class="phase-chip phase-chip-maintenance">{copy.reasoning.phases[2]}</span>
							<div class="phase-core">
								<Infinity class="size-20" />
							</div>
						</div>

						<div class="space-y-3">
							{#each copy.reasoning.signals as signal}
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
						{#each copy.reasoning.events as event}
							<div
								class="hero-log-line grid grid-cols-[58px_92px_1fr] gap-3 rounded-md border border-white/8 bg-white/4.5 px-3 py-2 text-xs text-white/62"
							>
								<span class="font-mono text-(--anda-amber-soft)">{event.time}</span>
								<span class="text-(--anda-teal)">{event.phase}</span>
								<span>{event.detail}</span>
							</div>
						{/each}
					</div>
				</div>

				<div class="grid gap-4">
					{#each copy.reasoning.cards as card}
						<Card class="reasoning-card p-6">
							<p class="font-mono text-xs text-(--anda-amber-soft) uppercase">{card.label}</p>
							<h3 class="mt-3 text-2xl font-semibold text-white">{card.title}</h3>
							<p class="mt-3 max-w-2xl leading-7 text-white/66">{card.detail}</p>
						</Card>
					{/each}
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
						{copy.memory.badge}
					</Badge>
					<h2 class="anda-display mt-5 max-w-xl text-4xl leading-tight text-white sm:text-5xl">
						{copy.memory.title}
					</h2>
				</div>
				<p class="max-w-2xl text-lg leading-8 text-white/68 lg:justify-self-end">
					{copy.memory.body}
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
					<h3 class="text-xl font-semibold text-white">{copy.memory.formationTitle}</h3>
					<p class="mt-3 leading-7 text-white/66">
						{copy.memory.formationBody}
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
					<h3 class="text-xl font-semibold text-white">{copy.memory.recallTitle}</h3>
					<p class="mt-3 leading-7 text-white/66">
						{copy.memory.recallBody}
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
					<h3 class="text-xl font-semibold text-white">{copy.memory.maintenanceTitle}</h3>
					<p class="mt-3 leading-7 text-white/66">
						{copy.memory.maintenanceBody}
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
						><Network class="size-4 text-(--anda-teal)" /> {copy.work.contextRoutes}</span
					>
					<span class="font-mono text-(--anda-amber-soft)">{copy.work.memoryRoute}</span>
				</div>

				<div class="grid gap-3 sm:grid-cols-2">
					{#each copy.work.surfaces as surface}
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
					{copy.work.badge}
				</Badge>
				<h2 class="anda-display mt-5 max-w-xl text-4xl leading-tight text-white sm:text-5xl">
					{copy.work.title}
				</h2>
				<p class="mt-5 max-w-2xl text-lg leading-8 text-white/68">
					{copy.work.body}
				</p>

				<div class="mt-8 grid gap-3 sm:grid-cols-2">
					<Card class="work-card p-5" size="sm">
						<Terminal class="mb-4 size-6 text-(--anda-amber-soft)" />
						<h3 class="font-semibold text-white">{copy.work.cards[0].title}</h3>
						<p class="mt-2 leading-7 text-white/62">
							{copy.work.cards[0].detail}
						</p>
					</Card>
					<Card class="work-card p-5" size="sm">
						<MessageSquare class="mb-4 size-6 text-(--anda-teal)" />
						<h3 class="font-semibold text-white">{copy.work.cards[1].title}</h3>
						<p class="mt-2 leading-7 text-white/62">
							{copy.work.cards[1].detail}
						</p>
					</Card>
					<Card class="work-card p-5" size="sm">
						<KeyRound class="mb-4 size-6 text-(--anda-clay)" />
						<h3 class="font-semibold text-white">{copy.work.cards[2].title}</h3>
						<p class="mt-2 leading-7 text-white/62">
							{copy.work.cards[2].detail}
						</p>
					</Card>
					<Card class="work-card p-5" size="sm">
						<ShieldCheck class="mb-4 size-6 text-(--anda-lichen)" />
						<h3 class="font-semibold text-white">{copy.work.cards[3].title}</h3>
						<p class="mt-2 leading-7 text-white/62">
							{copy.work.cards[3].detail}
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
					{copy.start.badge}
				</Badge>
				<h2 class="anda-display mt-5 max-w-xl text-4xl leading-tight sm:text-5xl">
					{copy.start.title}
				</h2>
				<p class="mt-5 max-w-2xl text-lg leading-8 text-black/66">
					{copy.start.bodyPrefix} <code>~/.anda</code>
					{copy.start.bodySuffix}
				</p>

				<div class="mt-8 flex flex-col gap-3 sm:flex-row">
					<Button
						href="https://github.com/ldclabs/anda-bot#quick-start"
						target="_blank"
						rel="noreferrer"
						class="bg-(--anda-ink) text-(--anda-parchment) shadow-[0_18px_46px_rgba(7,17,15,0.28)] hover:bg-(--anda-forest)"
						size="lg"
					>
						<ArrowRight class="size-4" />
						{copy.start.quickStart}
					</Button>
					<Button
						href="https://github.com/ldclabs/anda-hippocampus"
						target="_blank"
						rel="noreferrer"
						variant="secondary"
						class="border-black/15 bg-black/6 text-(--anda-ink) backdrop-blur-none hover:bg-black/10"
						size="lg"
					>
						<Brain class="size-4" />
						{copy.start.meetHippocampus}
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
						><Terminal class="size-4 text-(--anda-teal)" /> {copy.start.terminalLabel}</span
					>
					<span>~/.anda/config.yaml</span>
				</div>
				<div dir="ltr" class="p-5 font-mono text-sm leading-7 wrap-break-word text-white/76">
					<code class="block">
						<span class="block text-white/42"># {copy.start.sourceComment}</span>
						{#each activeRunCommands as command}
							<span class="block text-(--anda-teal)">{command}</span>
						{/each}
						<span class="mt-3 block text-white/42"># {copy.start.goalComment}</span>
						<span class="block text-(--anda-amber-soft)">anda</span>
						<span class="block"><span class="text-(--anda-amber-soft)">anda</span> --help</span>
					</code>
				</div>
				<div class="grid gap-2 border-t border-white/10 p-5 text-sm text-white/62 sm:grid-cols-3">
					<span class="inline-flex items-center gap-2"
						><CheckCircle class="size-4 text-(--anda-teal)" /> {copy.start.localRuntime}</span
					>
					<span class="inline-flex items-center gap-2"
						><Clock3 class="size-4 text-(--anda-amber-soft)" /> {copy.start.durableThread}</span
					>
					<span class="inline-flex items-center gap-2"
						><Eye class="size-4 text-(--anda-lichen)" /> {copy.start.inspectableBrain}</span
					>
				</div>
			</div>
		</div>
	</section>
</main>
