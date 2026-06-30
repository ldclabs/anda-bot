<script lang="ts">
	import BrowserAppPreview from '$lib/components/landing/BrowserAppPreview.svelte';
	import NexusCanvas from '$lib/components/landing/NexusCanvas.svelte';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { Card } from '$lib/components/ui/card';
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
		Eye,
		KeyRound,
		Languages,
		LayoutPanelLeft,
		MessageSquare,
		Monitor,
		Network,
		RefreshCcw,
		Settings,
		ShieldCheck,
		Sparkles,
		Terminal,
		Workflow,
		Wrench,
		Zap
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

	function whyIcon(index: number) {
		return [Terminal, Workflow, Brain][index] ?? Brain;
	}

	function launcherIcon(index: number) {
		return [Settings, Monitor, KeyRound, RefreshCcw][index] ?? Settings;
	}

	function memoryIcon(index: number) {
		return [Brain, Workflow, Network, Wrench][index] ?? Brain;
	}

	function surfaceIcon(index: number) {
		return [LayoutPanelLeft, Settings, Terminal, MessageSquare][index] ?? LayoutPanelLeft;
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
	<meta content="https://anda.bot/_assets/images/anda-extension-marquee.png" property="og:image" />
	<meta content="summary_large_image" name="twitter:card" />
	<meta content="@ICPandaDAO" name="twitter:creator" />
</svelte:head>

<main
	dir={activeDirection}
	class="landing-shell min-h-[100dvh] overflow-x-clip text-(--anda-parchment)"
>
	<section
		class="hero-stage relative isolate min-h-[88dvh] overflow-hidden border-b border-white/10"
	>
		<NexusCanvas />
		<BrowserAppPreview variant="hero" />
		<div class="hero-vignette absolute inset-0"></div>
		<div class="memory-strata absolute inset-0 opacity-55"></div>

		<header
			class="relative z-20 mx-auto flex h-[72px] w-full max-w-7xl items-center justify-between gap-4 px-5 sm:px-6 lg:px-8"
		>
			<a
				href="/"
				class="group inline-flex min-w-0 items-center gap-3 text-sm font-semibold text-white"
			>
				<img src="/_assets/logo.hdr.png" alt="Anda Bot" class="hdr-img size-11 rounded-lg" />
				<span class="truncate text-xl sm:text-2xl">Anda Bot</span>
			</a>

			<nav class="hidden items-center gap-1 text-sm font-semibold text-white/70 lg:flex">
				<a href="#install" class="nav-link">{copy.nav.install}</a>
				<a href="#why" class="nav-link">{copy.nav.why}</a>
				<a href="#browser" class="nav-link">{copy.nav.browser}</a>
				<a href="#launcher" class="nav-link">{copy.nav.launcher}</a>
				<a href="#memory" class="nav-link">{copy.nav.memory}</a>
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

		<div
			class="relative z-10 mx-auto grid w-full max-w-7xl px-5 pt-8 pb-14 sm:px-6 sm:pt-12 lg:min-h-[calc(88dvh-72px)] lg:grid-cols-[minmax(0,0.76fr)_minmax(280px,0.24fr)] lg:items-center lg:px-8 lg:pt-0 lg:pb-20"
		>
			<div class="hero-copy max-w-4xl">
				<Badge tone="warm" class="mb-5 gap-2">
					<Sparkles class="size-3.5" />
					{copy.hero.badge}
				</Badge>
				<h1
					class="anda-display max-w-4xl text-4xl leading-[1.04] font-semibold text-white sm:text-5xl lg:text-7xl"
				>
					{copy.hero.title}
				</h1>
				<p
					class="mt-6 max-w-[32ch] text-base leading-7 text-white/76 sm:max-w-2xl sm:text-xl sm:leading-8"
				>
					{copy.hero.body}
				</p>

				<div
					class="mt-8 flex max-w-[32ch] flex-col gap-3 sm:max-w-none sm:flex-row text-shadow-none"
				>
					<Button href="#install" size="lg" class="min-w-36">
						<Download class="size-4" />
						{copy.hero.primary}
					</Button>
					<Button href="#browser" variant="secondary" size="lg" class="min-w-36">
						<LayoutPanelLeft class="size-4" />
						{copy.hero.secondary}
					</Button>
				</div>
			</div>
		</div>
	</section>

	<section class="proof-band border-b border-white/10 px-5 py-6 sm:px-6 lg:px-8">
		<div class="mx-auto grid max-w-7xl gap-3 md:grid-cols-3">
			{#each copy.proof as item}
				<div class="proof-item">
					<strong>{item.value}</strong>
					<span>{item.label}</span>
				</div>
			{/each}
		</div>
	</section>

	<section
		id="why"
		class="why-section relative z-10 border-b border-white/10 px-5 py-16 sm:px-6 lg:px-8 lg:py-24"
	>
		<div class="mx-auto max-w-7xl">
			<div class="why-shell">
				<div class="why-manifesto">
					<Badge tone="cool" class="gap-2">
						<Brain class="size-3.5" />
						{copy.why.badge}
					</Badge>
					<h2 class="anda-display mt-5 text-4xl leading-tight font-semibold text-white sm:text-5xl">
						{copy.why.title}
					</h2>
					<p class="mt-5 max-w-2xl text-lg leading-8 text-white/68">{copy.why.body}</p>
				</div>

				<div class="why-routes">
					{#each copy.why.routes as route, index}
						{@const Icon = whyIcon(index)}
						<article class={`why-route ${route.primary ? 'why-route-primary' : ''}`}>
							<div class="why-route-heading">
								<Icon class="size-5" />
								<div>
									<h3>{route.name}</h3>
									<span>{route.role}</span>
								</div>
							</div>
							<p>{route.fit}</p>
						</article>
					{/each}
				</div>
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
				<p class="mt-5 max-w-2xl text-lg leading-8 text-white/68">
					{copy.install.body}
				</p>
			</div>

			<div class="install-panel">
				<div
					class="flex flex-col gap-3 sm:flex-row"
					role="tablist"
					aria-label={copy.install.tabAria}
				>
					{#each installOrder as os}
						{@const Icon = installIcon(os)}
						<button
							type="button"
							role="tab"
							aria-selected={activeOs === os}
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
					<div class="flex items-start justify-between gap-4">
						<div>
							<h3>{activeInstall.title}</h3>
							<p>{activeInstall.body}</p>
						</div>
						<ShieldCheck class="mt-1 size-6 shrink-0 text-(--anda-teal)" />
					</div>

					<div class="mt-6 grid gap-3 sm:grid-cols-3">
						{#each activeInstall.steps as step}
							<div class="setup-step">
								<CheckCircle class="size-4" />
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
							<button
								type="button"
								class="install-command"
								aria-label={copy.install.commandAria}
								onclick={() => void copyInstallCommand()}
							>
								<code>{activeInstall.command}</code>
							</button>
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
								{activeInstall.primaryLabel}
							</Button>
						{/if}
						<p class="text-sm leading-6 text-white/56">{activeInstall.note}</p>
					</div>
				</div>
			</div>
		</div>
	</section>

	<section
		id="browser"
		class="browser-section relative z-10 border-b border-white/10 px-5 py-16 sm:px-6 lg:px-8 lg:py-24"
	>
		<div class="memory-strata absolute inset-0 opacity-20"></div>
		<div
			class="relative mx-auto grid max-w-7xl gap-10 lg:grid-cols-[1.08fr_0.92fr] lg:items-center"
		>
			<div class="browser-visual">
				<div class="browser-marquee-card">
					<div class="browser-marquee-toolbar">
						<span>Chrome Web Store / Edge Add-ons</span>
						<ExternalLink class="size-4" />
					</div>
					<img
						src="/_assets/images/anda-extension-marquee.png"
						alt="Anda Bot graph-memory agent browser extension promo"
						loading="lazy"
					/>
				</div>
			</div>

			<div class="section-copy">
				<Badge tone="cool" class="gap-2">
					<LayoutPanelLeft class="size-3.5" />
					{copy.browser.badge}
				</Badge>
				<h2 class="anda-display mt-5 text-4xl leading-tight font-semibold text-white sm:text-5xl">
					{copy.browser.title}
				</h2>
				<p class="mt-5 max-w-2xl text-lg leading-8 text-white/68">{copy.browser.body}</p>
				<div class="mt-8 flex flex-col gap-3 sm:flex-row sm:flex-wrap">
					<Button href={chromeExtensionStoreUrl} target="_blank" rel="noreferrer" size="lg">
						<ExternalLink class="size-4" />
						{copy.browser.chromeStore}
					</Button>
					<Button href={edgeExtensionStoreUrl} target="_blank" rel="noreferrer" size="lg">
						<ExternalLink class="size-4" />
						{copy.browser.edgeStore}
					</Button>
					<Button
						href={browserDocsUrl}
						target="_blank"
						rel="noreferrer"
						variant="secondary"
						size="lg"
					>
						<BookOpen class="size-4" />
						{copy.browser.docs}
					</Button>
				</div>

				<div class="mt-8 grid gap-3">
					{#each copy.browser.features as feature, index}
						{@const Icon = [Eye, Zap, ShieldCheck][index] ?? Eye}
						<article class="browser-feature">
							<Icon class="size-5 text-(--anda-amber-soft)" />
							<div>
								<h3>{feature.title}</h3>
								<p>{feature.detail}</p>
							</div>
						</article>
					{/each}
				</div>
			</div>
		</div>
	</section>

	<section
		id="launcher"
		class="relative z-10 border-b border-white/10 px-5 py-16 sm:px-6 lg:px-8 lg:py-24"
	>
		<div class="mx-auto max-w-7xl">
			<div class="section-copy max-w-3xl">
				<Badge tone="warm" class="gap-2">
					<Settings class="size-3.5" />
					{copy.launcher.badge}
				</Badge>
				<h2 class="anda-display mt-5 text-4xl leading-tight font-semibold text-white sm:text-5xl">
					{copy.launcher.title}
				</h2>
				<p class="mt-5 max-w-2xl text-lg leading-8 text-white/68">{copy.launcher.body}</p>
			</div>

			<div class="launcher-grid mt-10">
				<div class="launcher-device">
					<img src="/_assets/logo.hdr.png" alt="Anda Bot" class="hdr-img size-16 rounded-2xl" />
					<div>
						<strong>Anda Bot</strong>
						<span>local daemon ready</span>
					</div>
					<div class="launcher-menu">
						<span>Open Anda</span>
						<span>Status</span>
						<span>Browser token</span>
						<span>Check updates</span>
						<span>Logs</span>
					</div>
				</div>

				<div class="grid gap-4 sm:grid-cols-2">
					{#each copy.launcher.features as feature, index}
						{@const Icon = launcherIcon(index)}
						<Card class="feature-card p-6">
							<h3 class="flex items-center gap-4">
								<Icon class="size-6 text-(--anda-amber-soft)" />
								<span>{feature.title}</span>
							</h3>
							<p>{feature.detail}</p>
						</Card>
					{/each}
				</div>
			</div>
		</div>
	</section>

	<section
		id="memory"
		class="memory-section relative z-10 border-b border-white/10 px-5 py-16 sm:px-6 lg:px-8 lg:py-24"
	>
		<div class="mx-auto max-w-7xl">
			<div class="section-copy max-w-3xl">
				<Badge tone="cool" class="gap-2">
					<Brain class="size-3.5" />
					{copy.memory.badge}
				</Badge>
				<h2 class="anda-display mt-5 text-4xl leading-tight font-semibold text-white sm:text-5xl">
					{copy.memory.title}
				</h2>
				<p class="mt-5 max-w-2xl text-lg leading-8 text-white/68">{copy.memory.body}</p>
			</div>

			<div class="memory-bento mt-10">
				{#each copy.memory.features as feature, index}
					{@const Icon = memoryIcon(index)}
					<Card class={`memory-cell memory-cell-${index} p-6`}>
						<Icon class="size-6 text-(--anda-teal)" />
						<h3>{feature.title}</h3>
						<p>{feature.detail}</p>
					</Card>
				{/each}
			</div>
		</div>
	</section>

	<section
		id="work"
		class="relative z-10 border-b border-white/10 px-5 py-16 sm:px-6 lg:px-8 lg:py-24"
	>
		<div class="mx-auto grid max-w-7xl gap-10 lg:grid-cols-[0.78fr_1.22fr] lg:items-center">
			<div class="section-copy">
				<Badge tone="ink" class="gap-2">
					<Workflow class="size-3.5" />
					{copy.work.badge}
				</Badge>
				<h2 class="anda-display mt-5 text-4xl leading-tight font-semibold text-white sm:text-5xl">
					{copy.work.title}
				</h2>
				<p class="mt-5 max-w-2xl text-lg leading-8 text-white/68">{copy.work.body}</p>
			</div>

			<div class="surface-map">
				{#each copy.work.surfaces as surface, index}
					{@const Icon = surfaceIcon(index)}
					<article class="surface-item">
						<h3 class="flex items-center gap-4">
							<Icon class="size-5 text-(--anda-amber-soft)" />
							<span>{surface.label}</span>
						</h3>
						<p>{surface.detail}</p>
					</article>
				{/each}
			</div>
		</div>
	</section>

	<section id="start" class="final-cta px-5 py-16 sm:px-6 lg:px-8 lg:py-24">
		<div class="mx-auto grid max-w-7xl gap-8 lg:grid-cols-[1fr_auto] lg:items-end">
			<div>
				<h2
					class="anda-display max-w-3xl text-4xl leading-tight font-semibold text-white sm:text-5xl"
				>
					{copy.final.title}
				</h2>
				<p class="mt-5 max-w-2xl text-lg leading-8 text-white/68">{copy.final.body}</p>
			</div>
			<div class="flex flex-col gap-3 sm:flex-row lg:justify-end">
				<Button href="#install" size="lg">
					<Download class="size-4" />
					{copy.final.install}
				</Button>
				<Button
					href="https://docs.anda.bot"
					target="_blank"
					rel="noreferrer"
					variant="secondary"
					size="lg"
				>
					<BookOpen class="size-4" />
					{copy.final.docs}
				</Button>
				<Button
					href="https://github.com/ldclabs/anda-bot"
					target="_blank"
					rel="noreferrer"
					variant="ghost"
					size="lg"
				>
					<Github class="size-5" />
					{copy.final.github}
				</Button>
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
