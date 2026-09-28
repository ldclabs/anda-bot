<script lang="ts">
	import NexusCanvas from '$lib/components/landing/NexusCanvas.svelte';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import Github from '$lib/components/ui/icons/github.svelte';
	import { infoCopy } from '$lib/content/info';
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
		BookOpen,
		Brain,
		CheckCircle,
		Copy,
		Download,
		ExternalLink,
		FileText,
		Globe,
		Languages,
		LayoutPanelLeft,
		MessageSquare,
		Monitor,
		Network,
		Settings,
		ShieldCheck,
		Sparkles,
		Terminal,
		Workflow,
		Wrench,
		Clock
	} from '@lucide/svelte';
	import { onMount } from 'svelte';

	const installOrder: OsKey[] = ['windows', 'macos', 'linux'];
	const localeStorageKey = 'anda-bot-landing-locale';
	const chromeExtensionStoreUrl =
		'https://chromewebstore.google.com/detail/anda-bot/injpfajmddchcphfkdkiflfddmajglfd';
	const edgeExtensionStoreUrl =
		'https://microsoftedge.microsoft.com/addons/detail/anda-bot/hljillhnmfbobihkehdlpmhbmdgophah';
	const browserDocsUrl = 'https://docs.anda.bot/docs/quick-start/browser-extension';

	let activeLocale = $state<Locale>(fallbackLocale);
	let activeOs = $state<OsKey>('windows');
	let detectedOs = $state<OsKey | null>(null);
	let copyState = $state<'idle' | 'copied' | 'failed'>('idle');
	let copyResetTimer: ReturnType<typeof setTimeout> | undefined;
	let copy = $derived(landingCopy[activeLocale]);
	let info = $derived(infoCopy[activeLocale]);
	let activeDirection = $derived(localeMeta[activeLocale].dir);
	let activeInstall = $derived(copy.install.options[activeOs]);

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

	function detectedLabel(os: OsKey) {
		return formatTemplate(copy.install.detected, { os: copy.install.options[os].label });
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
		if (copyResetTimer) clearTimeout(copyResetTimer);
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
		if (!activeInstall.command) return;

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

	function installIcon(os: OsKey) {
		if (os === 'windows') return Monitor;
		if (os === 'macos') return Sparkles;
		return Terminal;
	}

	const pillarTargets = ['memory', 'action', 'control'];
	const surfaceLinks = [
		browserDocsUrl,
		'https://docs.anda.bot/docs/quick-start/terminal',
		'https://docs.anda.bot/docs/runtime/channels',
		'https://github.com/ldclabs/anda-bot/tree/main/desktop'
	];

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
		if (detected) {
			detectedOs = detected;
			activeOs = detected;
		}
		return () => {
			if (copyResetTimer) clearTimeout(copyResetTimer);
		};
	});
</script>

<svelte:head>
	<title>{copy.meta.title}</title>
	<meta name="description" content={copy.meta.description} />
	<meta property="og:title" content={copy.meta.title} />
	<meta property="og:description" content={copy.meta.description} />
	<meta property="og:type" content="website" />
	<meta property="og:url" content="https://anda.bot" />
	<meta property="og:image" content="https://anda.bot/_assets/logo.png" />
	<meta name="twitter:card" content="summary" />
	<meta name="twitter:creator" content="@ICPandaDAO" />
</svelte:head>

<main dir={activeDirection} class="landing-shell min-h-screen text-(--anda-parchment)">
	<section class="hero-stage relative isolate overflow-hidden border-b border-white/10">
		<NexusCanvas />
		<div class="hero-vignette pointer-events-none absolute inset-0"></div>
		<header
			class="relative z-20 mx-auto flex min-h-20 py-4 w-full max-w-7xl items-center justify-between gap-4 px-5 sm:px-6 lg:px-8"
		>
			<a
				href="/"
				class="group inline-flex min-w-0 items-center gap-3 text-sm font-semibold text-white"
			>
				<img src="/_assets/logo.hdr.png" alt="Anda Bot" class="hdr-img size-11 rounded-lg" />
				<span class="truncate text-xl sm:text-2xl">Anda Bot</span>
			</a>

			<nav
				aria-label={info.common.navigationLabel}
				class="hidden items-center gap-1 text-sm font-semibold text-white/80 xl:flex"
			>
				<a href="#memory" class="nav-link">{copy.nav.memory}</a>
				<a href="#action" class="nav-link">{copy.nav.action}</a>
				<a href="#control" class="nav-link">{copy.nav.control}</a>
				<a href="#surfaces" class="nav-link">{copy.nav.surfaces}</a>
				<a href="#install" class="nav-link">{copy.nav.install}</a>
				<a href="https://docs.anda.bot" target="_blank" rel="noreferrer" class="nav-link"
					>{copy.nav.docs}</a
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
					class="hidden sm:inline-flex"
				>
					<Github class="size-5" />
					GitHub
				</Button>
			</div>
		</header>
		<div class="hero-intro page-width">
			<div class="relative">
				<Badge tone="warm" class="mb-7 gap-2"><Sparkles class="size-3.5" />{copy.hero.badge}</Badge>
				<h1 class="anda-display hero-title">
					{#each copy.hero.title as line, index}<span class:accent={index === 2}>{line}</span
						>{/each}
				</h1>
			</div>
			<div class="hero-description">
				<p>{copy.hero.body}</p>
				<div class="mt-8 flex flex-col gap-3 sm:flex-row sm:flex-wrap">
					<Button href="#install" size="lg"><Download class="size-4" />{copy.hero.primary}</Button>
					<Button href="#overview" variant="secondary" size="lg"
						>{copy.hero.secondary}<ArrowRight class="direction-arrow size-4" /></Button
					>
				</div>
				<ul class="hero-facts">
					{#each copy.hero.facts as fact}<li>
							<CheckCircle aria-hidden="true" class="size-3.5" />{fact}
						</li>{/each}
				</ul>
			</div>
		</div>
		<nav id="overview" class="pillar-links page-width" aria-label={copy.hero.secondary}>
			{#each copy.pillars as pillar, index}
				{@const Icon = [Brain, Workflow, ShieldCheck][index]}
				<a href={`#${pillarTargets[index]}`} class="pillar-link">
					<div class="flex items-center justify-between gap-3">
						<Icon class="size-6" /><ArrowRight class="direction-arrow size-4" />
					</div>
					<h2>{pillar.title}</h2>
					<p>{pillar.detail}</p>
				</a>
			{/each}
		</nav>
	</section>

	<section id="memory" class="narrative-section memory-section">
		<div class="page-width narrative-split">
			<div class="section-copy">
				<p class="section-eyebrow">{copy.memory.badge}</p>
				<h2>{copy.memory.title}</h2>
				<p class="section-body">{copy.memory.body}</p>
				<a
					class="text-link mt-7"
					href="https://docs.anda.bot/docs/memory/brain"
					target="_blank"
					rel="noreferrer">{copy.memoryLink}<ArrowRight class="direction-arrow size-4" /></a
				>
			</div>
			<div>
				<div class="feature-list">
					{#each copy.memory.features as feature, index}
						{@const Icon = [Settings, Network, Clock][index]}
						<article>
							<Icon class="size-5" />
							<div>
								<h3>{feature.title}</h3>
								<p>{feature.detail}</p>
							</div>
						</article>
					{/each}
				</div>
				<aside class="release-note">
					<span>{copy.previewLabel}</span>
					<p>{copy.memoryPreview}</p>
				</aside>
			</div>
		</div>
	</section>

	<section id="action" class="narrative-section">
		<div class="page-width">
			<div class="section-copy max-w-3xl">
				<p class="section-eyebrow">{copy.action.badge}</p>
				<h2>{copy.action.title}</h2>
				<p class="section-body">{copy.action.body}</p>
			</div>
			<div class="action-grid">
				{#each copy.action.features as feature, index}
					{@const Icon = [Globe, FileText, Workflow, Clock][index]}
					<article>
						<Icon class="size-7" />
						<h3>{feature.title}</h3>
						<p>{feature.detail}</p>
					</article>
				{/each}
			</div>
		</div>
	</section>

	<section id="control" class="narrative-section control-section">
		<div class="page-width">
			<div class="section-copy max-w-3xl">
				<p class="section-eyebrow">{copy.control.badge}</p>
				<h2>{copy.control.title}</h2>
				<p class="section-body">{copy.control.body}</p>
			</div>
			<div class="control-grid">
				{#each copy.control.features as feature, index}
					{@const Icon = [Monitor, Settings, Wrench][index]}
					<article>
						<Icon class="size-6" />
						<h3>{feature.title}</h3>
						<p>{feature.detail}</p>
					</article>
				{/each}
			</div>
			<div class="data-note">
				<ShieldCheck class="size-5 shrink-0" />
				<p>{copy.dataNote}</p>
			</div>
			<div class="mt-6 flex flex-wrap gap-x-8 gap-y-4">
				<a class="text-link" href="/privacy"
					>{copy.privacyLink}<ArrowRight class="direction-arrow size-4" /></a
				>
				<a
					class="text-link"
					href="https://github.com/ldclabs/anda-bot"
					target="_blank"
					rel="noreferrer">{copy.sourceLink}<Github class="size-4" /></a
				>
			</div>
		</div>
	</section>

	<section id="surfaces" class="narrative-section">
		<div class="page-width narrative-split">
			<div class="section-copy">
				<p class="section-eyebrow">{copy.surfaces.badge}</p>
				<h2>{copy.surfaces.title}</h2>
				<p class="section-body">{copy.surfaces.body}</p>
				<div class="mt-7 flex flex-wrap gap-x-6 gap-y-4">
					<a class="text-link" href={chromeExtensionStoreUrl} target="_blank" rel="noreferrer"
						>Chrome<ExternalLink class="size-3.5" /></a
					>
					<a class="text-link" href={edgeExtensionStoreUrl} target="_blank" rel="noreferrer"
						>Edge<ExternalLink class="size-3.5" /></a
					>
				</div>
			</div>
			<div class="surface-list">
				{#each copy.surfaces.items as surface, index}
					{@const Icon = [LayoutPanelLeft, Terminal, MessageSquare, Monitor][index]}
					<article>
						<div class="flex items-center gap-3">
							<Icon class="size-5" />
							<h3>{surface.title}</h3>
						</div>
						{#if surface.status}<span class="surface-status">{surface.status}</span>{/if}
						<p>{surface.detail}</p>
						<a class="text-link" href={surfaceLinks[index]} target="_blank" rel="noreferrer"
							>{surface.linkLabel}<ArrowRight class="direction-arrow size-4" /></a
						>
					</article>
				{/each}
			</div>
		</div>
	</section>
	<section
		id="install"
		class="relative z-10 border-b border-white/10 px-5 py-16 sm:px-6 lg:px-8 lg:py-24"
	>
		<div class="mx-auto grid max-w-7xl gap-8 lg:grid-cols-[0.92fr_1.08fr] lg:items-start">
			<div class="section-copy">
				<Badge tone="warm" class="gap-2">
					<Download class="size-3.5" />
					{copy.install.badge}
				</Badge>
				<h2 class="anda-display mt-5 text-4xl leading-tight font-semibold text-white sm:text-5xl">
					{copy.install.title}
				</h2>
				<p class="mt-5 max-w-2xl text-lg leading-8 text-white/75">
					{copy.install.body}
				</p>
				<p class="install-requirements">{copy.install.requirements}</p>
			</div>

			<div class="install-panel">
				<div class="flex flex-col gap-3 sm:flex-row" role="group" aria-label={copy.install.osLabel}>
					{#each installOrder as os}
						{@const Icon = installIcon(os)}
						<button
							type="button"
							aria-pressed={activeOs === os}
							class={`os-tab ${activeOs === os ? 'os-tab-active' : ''}`}
							onclick={() => selectOs(os)}
						>
							<Icon class="size-4" />
							<span>{copy.install.options[os].label}</span>
							{#if detectedOs === os}
								<small>{detectedLabel(os)}</small>
							{/if}
						</button>
					{/each}
				</div>

				<div class="install-route mt-5">
					<span class="sr-only" role="status"
						>{copyState === 'copied'
							? copy.install.copied
							: copyState === 'failed'
								? copy.install.copyFailed
								: ''}</span
					>
					<div class="flex items-start justify-between gap-4">
						<div>
							<h3>{activeInstall.title}</h3>
							<p>{activeInstall.body}</p>
						</div>
						<ShieldCheck class="mt-1 size-6 shrink-0 text-(--anda-teal)" />
					</div>

					<div class="mt-6 grid gap-3 sm:grid-cols-3">
						{#each activeInstall.steps as step, index}
							<div class="setup-step">
								<span class="step-number" aria-hidden="true">{index + 1}</span>
								<span>{step}</span>
							</div>
						{/each}
					</div>

					{#if activeInstall.command}
						<div class="command-card mt-5">
							<div
								class="flex items-center justify-between gap-3 border-b border-white/10 px-4 py-3"
							>
								<span class="inline-flex min-w-0 items-center gap-2 truncate text-sm text-white/60">
									<Terminal class="size-4 shrink-0 text-(--anda-teal)" />
									{activeInstall.commandLabel}
								</span>
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
							<pre class="install-command" dir="ltr"><code>{activeInstall.command}</code></pre>
						</div>
					{/if}

					<div class="mt-5 flex flex-col gap-3 sm:flex-row sm:items-center">
						{#if activeInstall.href}
							<Button
								href={activeInstall.href}
								target={activeInstall.download ? undefined : '_blank'}
								rel={activeInstall.download ? undefined : 'noreferrer'}
								download={activeInstall.download}
								size="lg"
							>
								<ArrowRight class="size-4" />
								{activeInstall.primaryLabel}
							</Button>
						{:else}
							<Button type="button" size="lg" onclick={() => void copyInstallCommand()}>
								<Copy class="size-4" />
								{copyState === 'copied'
									? copy.install.copied
									: copyState === 'failed'
										? copy.install.copyFailed
										: activeInstall.primaryLabel}
							</Button>
						{/if}
						<p class="text-sm leading-6 text-white/70">{activeInstall.note}</p>
					</div>
				</div>
			</div>
		</div>
	</section>

	<section class="narrative-section" aria-labelledby="faq-title">
		<div class="page-width narrative-split">
			<h2 id="faq-title" class="faq-title">{copy.faqTitle}</h2>
			<div class="faq-list">
				{#each copy.faq as item}<details>
						<summary>{item.title}</summary>
						<p>{item.detail}</p>
					</details>{/each}
			</div>
		</div>
	</section>
	<section class="final-cta narrative-section">
		<div class="page-width flex flex-col gap-8 lg:flex-row lg:items-end lg:justify-between">
			<div class="section-copy max-w-3xl">
				<h2>{copy.final.title}</h2>
				<p class="section-body">{copy.final.body}</p>
			</div>
			<div class="flex shrink-0 flex-col gap-3 sm:flex-row">
				<Button href="#install" size="lg"><Download class="size-4" />{copy.final.install}</Button>
				<Button
					href="https://docs.anda.bot"
					target="_blank"
					rel="noreferrer"
					variant="secondary"
					size="lg"><BookOpen class="size-4" />{copy.final.docs}</Button
				>
			</div>
		</div>
	</section>
	<footer class="border-t border-white/10 px-5 py-8 sm:px-6 lg:px-8">
		<div
			class="mx-auto flex max-w-7xl flex-col gap-5 sm:flex-row sm:items-center sm:justify-between"
		>
			<a href="/" class="inline-flex items-center gap-3 font-semibold text-white">
				<img src="/_assets/logo.hdr.png" alt="Anda Bot" class="hdr-img size-10 rounded-lg" />
				<span>Anda Bot</span>
			</a>

			<nav class="flex flex-wrap gap-x-5 gap-y-2 text-sm font-medium text-white/56">
				<a class="hover:text-white" href="/privacy">{info.common.privacy}</a>
				<a class="hover:text-white" href="/terms">{info.common.terms}</a>
				<a class="hover:text-white" href="/support">{info.common.support}</a>
				<a class="hover:text-white" href="https://docs.anda.bot" target="_blank" rel="noreferrer">
					{info.common.docs}
				</a>
			</nav>
		</div>
	</footer>
</main>

<style>
	.page-width {
		width: 100%;
		max-width: 80rem;
		margin-inline: auto;
		padding-inline: 2rem;
	}
	.hero-intro {
		position: relative;
		display: grid;
		grid-template-columns: 1.08fr 0.92fr;
		gap: 4rem;
		align-items: end;
		padding-block: 6rem 5rem;
	}
	.hero-title {
		font-size: clamp(3rem, 5.6vw, 5.4rem);
		font-weight: 650;
		line-height: 1.12;
		letter-spacing: -0.04em;
		color: white;
	}
	.hero-title span {
		display: block;
	}
	.hero-title .accent {
		color: var(--anda-amber-soft);
	}
	.hero-description {
		padding-bottom: 0.5rem;
		max-width: 34rem;
	}
	.hero-description > p {
		color: rgba(255, 255, 255, 0.83);
		font-size: 1.15rem;
		line-height: 1.85;
	}
	.hero-facts {
		display: flex;
		flex-wrap: wrap;
		gap: 0.75rem 1.1rem;
		margin-top: 2rem;
		font-size: 0.8rem;
		color: var(--anda-muted);
	}
	.hero-facts li {
		display: flex;
		align-items: center;
		gap: 0.4rem;
	}
	.hero-facts :global(svg) {
		color: var(--anda-lichen);
		flex-shrink: 0;
	}
	.pillar-links {
		position: relative;
		display: grid;
		grid-template-columns: repeat(3, minmax(0, 1fr));
		padding-bottom: 2rem;
	}
	.pillar-link {
		padding: 1.5rem 2rem;
		border-top: 1px solid #ffffff30;
		background: #07110f70;
		transition: background 150ms;
	}
	.pillar-link + .pillar-link {
		border-inline-start: 1px solid #ffffff20;
	}
	.pillar-link:hover {
		background: #ffffff0b;
	}
	.pillar-link :global(svg) {
		color: var(--anda-amber-soft);
	}
	.pillar-link h2 {
		margin-top: 1rem;
		font-size: 1.2rem;
		font-weight: 650;
		color: white;
	}
	.pillar-link p {
		margin-top: 0.6rem;
		font-size: 0.95rem;
		line-height: 1.7;
		color: var(--anda-muted);
	}
	.narrative-section {
		padding-block: 6rem;
		border-bottom: 1px solid #ffffff18;
		scroll-margin-top: 1rem;
	}
	.narrative-split {
		display: grid;
		grid-template-columns: minmax(0, 1fr) minmax(0, 1.15fr);
		gap: 5rem;
		align-items: start;
	}
	.section-eyebrow {
		color: var(--anda-amber-soft);
		font-size: 0.85rem;
		font-weight: 600;
		margin-bottom: 1.25rem;
	}
	.section-copy h2,
	.faq-title {
		white-space: pre-line;
		font-size: clamp(2rem, 3.25vw, 3rem);
		line-height: 1.25;
		font-weight: 600;
		letter-spacing: -0.025em;
		color: white;
		text-wrap: balance;
	}
	.section-body {
		margin-top: 1.5rem;
		font-size: 1.06rem;
		line-height: 1.85;
		color: var(--anda-muted);
	}
	.text-link {
		display: inline-flex;
		align-items: center;
		gap: 0.65rem;
		font-size: 0.9rem;
		font-weight: 600;
		color: var(--anda-amber-soft);
		text-underline-offset: 5px;
	}
	.text-link:hover {
		text-decoration: underline;
	}
	:global(.landing-shell a:focus-visible),
	:global(.landing-shell button:focus-visible),
	:global(.landing-shell select:focus-visible),
	summary:focus-visible {
		outline: 2px solid var(--anda-amber-soft);
		outline-offset: 5px;
	}
	:global(.landing-shell[dir='rtl'] .direction-arrow) {
		transform: scaleX(-1);
	}
	.feature-list article {
		display: grid;
		grid-template-columns: auto 1fr;
		gap: 1.25rem;
		padding-block: 1.5rem;
		border-bottom: 1px solid #ffffff20;
	}
	.feature-list article:first-child {
		padding-top: 0;
	}
	.feature-list :global(svg) {
		color: var(--anda-teal);
		margin-top: 0.2rem;
	}
	article h3 {
		font-size: 1.15rem;
		line-height: 1.45;
		font-weight: 650;
		color: white;
	}
	article p {
		margin-top: 0.7rem;
		line-height: 1.8;
		color: var(--anda-muted);
	}
	.release-note {
		margin-top: 1.5rem;
		padding-inline-start: 1rem;
		border-inline-start: 2px solid var(--anda-teal);
	}
	.release-note span,
	.surface-status {
		display: inline-block;
		color: var(--anda-lichen);
		font-size: 0.75rem;
		font-weight: 600;
	}
	.release-note p {
		color: var(--anda-muted);
		font-size: 0.85rem;
		line-height: 1.7;
		margin-top: 0.4rem;
	}
	.action-grid {
		display: grid;
		grid-template-columns: repeat(2, minmax(0, 1fr));
		margin-top: 3rem;
		border-top: 1px solid #ffffff20;
	}
	.action-grid article {
		padding-block: 2rem;
		padding-inline: 0 2.5rem;
		border-bottom: 1px solid #ffffff20;
	}
	.action-grid article:nth-child(even) {
		padding-inline: 2.5rem 0;
		border-inline-start: 1px solid #ffffff20;
	}
	.action-grid :global(svg),
	.control-grid :global(svg) {
		color: var(--anda-amber-soft);
		margin-bottom: 1.2rem;
	}
	.control-section {
		background: #ffffff04;
	}
	.control-grid {
		display: grid;
		grid-template-columns: repeat(3, minmax(0, 1fr));
		gap: 2.5rem;
		margin-top: 3rem;
	}
	.data-note {
		display: flex;
		align-items: start;
		gap: 1rem;
		padding-top: 2rem;
		margin-top: 2.5rem;
		border-top: 1px solid #ffffff20;
		color: var(--anda-muted);
		font-size: 0.9rem;
		line-height: 1.8;
	}
	.data-note :global(svg) {
		margin-top: 0.2rem;
		color: var(--anda-lichen);
	}
	.surface-list {
		display: grid;
		grid-template-columns: repeat(2, minmax(0, 1fr));
		gap: 2rem;
	}
	.surface-list article {
		display: flex;
		flex-direction: column;
		align-items: start;
		padding-top: 1.25rem;
		border-top: 1px solid #ffffff25;
	}
	.surface-list :global(svg) {
		color: var(--anda-amber-soft);
		flex-shrink: 0;
	}
	.surface-list .text-link {
		padding-top: 1.25rem;
		margin-top: auto;
	}
	.surface-status {
		margin-top: 0.7rem;
	}
	.install-requirements {
		margin-top: 1.5rem;
		padding-inline-start: 1rem;
		border-inline-start: 2px solid var(--anda-amber);
		font-size: 0.95rem;
		line-height: 1.8;
		color: var(--anda-muted);
	}
	.install-panel,
	.install-route {
		min-width: 0;
	}
	.install-route {
		overflow-wrap: anywhere;
	}
	.install-panel :global([data-slot='button']) {
		height: auto;
		min-height: 2.75rem;
		white-space: normal;
	}
	.step-number {
		color: var(--anda-amber-soft);
		font-variant-numeric: tabular-nums;
	}
	pre.install-command {
		cursor: text;
		white-space: pre-wrap;
		overflow-wrap: anywhere;
		text-align: start;
	}
	.faq-list details {
		border-bottom: 1px solid #ffffff25;
		padding-block: 1.25rem;
	}
	.faq-list details:first-child {
		padding-top: 0;
	}
	summary {
		cursor: pointer;
		color: white;
		font-weight: 600;
		line-height: 1.6;
		padding-inline-end: 1rem;
	}
	.faq-list p {
		margin-top: 1rem;
		line-height: 1.85;
		color: var(--anda-muted);
	}
	@media (max-width: 1023px) {
		.hero-intro {
			gap: 2rem;
			padding-block: 4rem;
		}
		.hero-title {
			font-size: clamp(2.7rem, 6.8vw, 4.8rem);
		}
		.narrative-split {
			gap: 2.5rem;
			grid-template-columns: 1fr 1.2fr;
		}
		.surface-list {
			grid-template-columns: 1fr;
		}
		.pillar-link {
			padding: 1.5rem 1rem;
		}
	}
	@media (max-width: 767px) {
		.page-width {
			padding-inline: 1.25rem;
		}
		.hero-intro,
		.narrative-split {
			grid-template-columns: 1fr;
		}
		.hero-intro {
			padding-block: 3.5rem 2.5rem;
			gap: 2rem;
		}
		.hero-title {
			font-size: clamp(2.6rem, 11vw, 4.5rem);
		}
		.hero-description > p {
			font-size: 1rem;
		}
		.pillar-links {
			grid-template-columns: 1fr;
		}
		.pillar-link {
			padding: 1.25rem 0;
		}
		.pillar-link + .pillar-link {
			border-inline-start: 0;
		}
		.pillar-link h2 {
			margin-top: 0.75rem;
		}
		.narrative-section {
			padding-block: 3.5rem;
		}
		.action-grid,
		.control-grid {
			grid-template-columns: 1fr;
		}
		.action-grid article,
		.action-grid article:nth-child(even) {
			padding: 1.5rem 0;
			border-inline-start: 0;
		}
		.control-grid {
			gap: 2rem;
		}
	}
</style>
