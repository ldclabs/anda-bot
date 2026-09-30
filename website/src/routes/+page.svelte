<script lang="ts">
	import ActionCards from '$lib/components/landing/ActionCards.svelte';
	import AndaActor from '$lib/components/landing/AndaActor.svelte';
	import Composer from '$lib/components/landing/Composer.svelte';
	import InstallConsole from '$lib/components/landing/InstallConsole.svelte';
	import MachineDiagram from '$lib/components/landing/MachineDiagram.svelte';
	import MemoryBoard from '$lib/components/landing/MemoryBoard.svelte';
	import PandaMark from '$lib/components/landing/PandaMark.svelte';
	import SurfaceFrames from '$lib/components/landing/SurfaceFrames.svelte';
	import { createThread, type ThreadEngine } from '$lib/components/landing/thread/engine';
	import { Button } from '$lib/components/ui/button';
	import Anda from '$lib/components/ui/icons/anda.svelte';
	import Github from '$lib/components/ui/icons/github.svelte';
	import { LanguageMenu } from '$lib/components/ui/language-menu';
	import { infoCopy } from '$lib/content/info';
	import {
		detectLocale,
		fallbackLocale,
		isLocale,
		landingCopy,
		localeMeta,
		type Locale,
		type OsKey
	} from '$lib/content/landing';
	import {
		ArrowRight,
		BookOpen,
		Brain,
		CheckCircle,
		Download,
		ExternalLink,
		ShieldCheck,
		Workflow
	} from '@lucide/svelte';
	import { onMount, tick } from 'svelte';

	const localeStorageKey = 'anda-bot-landing-locale';
	const chromeExtensionStoreUrl =
		'https://chromewebstore.google.com/detail/anda-bot/injpfajmddchcphfkdkiflfddmajglfd';
	const edgeExtensionStoreUrl =
		'https://microsoftedge.microsoft.com/addons/detail/anda-bot/hljillhnmfbobihkehdlpmhbmdgophah';
	const surfaceLinks = [
		'https://docs.anda.bot/docs/quick-start/browser-extension',
		'https://docs.anda.bot/docs/quick-start/terminal',
		'https://docs.anda.bot/docs/runtime/channels',
		'https://github.com/ldclabs/anda-bot/tree/main/desktop'
	];
	const pillarTargets = ['memory', 'action', 'control'];
	const memoryStations = ['mem-0', 'mem-link', 'mem-revise'];
	const controlStations = ['ctl-mem', 'ctl-model', 'ctl-open'];

	let root: HTMLElement;
	let threadSvg: SVGSVGElement;
	let actorEl = $state<HTMLElement>();
	let engine = $state<ThreadEngine | null>(null);
	let activeLocale = $state<Locale>(fallbackLocale);
	let detectedOs = $state<OsKey | null>(null);
	let activeSection = $state('');
	let copy = $derived(landingCopy[activeLocale]);
	let info = $derived(infoCopy[activeLocale]);
	let activeDirection = $derived(localeMeta[activeLocale].dir);
	let navItems = $derived([
		{ id: 'memory', label: copy.nav.memory },
		{ id: 'action', label: copy.nav.action },
		{ id: 'control', label: copy.nav.control },
		{ id: 'surfaces', label: copy.nav.surfaces },
		{ id: 'install', label: copy.nav.install }
	]);

	$effect(() => {
		document.documentElement.lang = localeMeta[activeLocale].htmlLang;
		document.documentElement.dir = localeMeta[activeLocale].dir;
	});

	// Copy length and direction move the anchors; re-thread after the DOM settles.
	$effect(() => {
		void activeLocale;
		const current = engine;
		if (current) void tick().then(() => current.refresh());
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

	function selectLocale(locale: Locale) {
		activeLocale = locale;
		try {
			localStorage.setItem(localeStorageKey, locale);
		} catch {
			// Ignore private browsing storage failures.
		}
	}

	function marquee(words: readonly string[], separator: string) {
		return Array.from({ length: 3 }, () => words.join(separator)).join(separator);
	}

	onMount(() => {
		let storedLocale: string | null = null;
		try {
			storedLocale = localStorage.getItem(localeStorageKey);
		} catch {
			storedLocale = null;
		}
		activeLocale = isLocale(storedLocale)
			? storedLocale
			: detectLocale(navigator.languages?.length ? navigator.languages : [navigator.language]);

		const nav = navigator as Navigator & { userAgentData?: { platform?: string } };
		const platform = nav.userAgentData?.platform ?? navigator.platform ?? '';
		detectedOs = detectOs(`${platform} ${navigator.userAgent}`);

		let created: ThreadEngine | null = null;
		void tick().then(() => {
			if (!actorEl) return;
			created = createThread(root, threadSvg, actorEl, {
				onSection: (id) => {
					activeSection = id;
				}
			});
			engine = created;
		});
		return () => created?.destroy();
	});
</script>

<svelte:head>
	<title>{copy.meta.title}</title>
	<meta name="description" content={copy.meta.description} />
	<meta property="og:title" content={copy.meta.title} />
	<meta property="og:description" content={copy.meta.description} />
	<meta property="og:type" content="website" />
	<meta property="og:url" content="https://anda.bot" />
	<meta property="og:image" content="https://anda.bot/_assets/images/og-anda-bot.jpg" />
	<meta property="og:image:width" content="1200" />
	<meta property="og:image:height" content="630" />
	<meta property="og:image:alt" content={copy.meta.title} />
	<meta name="twitter:card" content="summary_large_image" />
	<meta name="twitter:creator" content="@AndaBotHQ" />
</svelte:head>

<main bind:this={root} dir={activeDirection} class="landing">
	<div class="stage" data-themed aria-hidden="true">
		<div class="stage-dawn"></div>
		<div class="stage-paper"></div>
		<div class="stage-glow"></div>
		<div class="stage-grain"></div>
	</div>
	<svg bind:this={threadSvg} class="thread-svg" aria-hidden="true"></svg>
	<AndaActor bind:el={actorEl} />

	<header class="site-header" data-scrolled data-themed>
		<a href="/" class="brand">
			<Anda />
			<span>Anda Bot</span>
		</a>
		<nav aria-label={info.common.navigationLabel}>
			{#each navItems as item}
				<a href={`#${item.id}`} aria-current={activeSection === item.id ? 'location' : undefined}
					>{item.label}</a
				>
			{/each}
			<a href="https://docs.anda.bot" target="_blank" rel="noreferrer">{copy.nav.docs}</a>
		</nav>
		<div class="header-actions">
			<LanguageMenu value={activeLocale} label={copy.language.label} onselect={selectLocale} />
			<Button
				href="https://github.com/ldclabs/anda-bot"
				target="_blank"
				rel="noreferrer"
				variant="ghost"
				size="sm"
				class="github"
			>
				<Github class="size-5" />
				<span>GitHub</span>
			</Button>
		</div>
		<span class="progress" data-progress aria-hidden="true"></span>
	</header>

	<section id="top" class="hero is-night" data-hero data-nav>
		<div class="wrap hero-grid">
			<div class="hero-copy">
				<p class="eyebrow"><span class="beacon"></span>{copy.hero.badge}</p>
				<h1 class="hero-title">
					{#each copy.hero.title as line, index}
						<span class="line" style="--i: {index}"><span>{line}</span></span>
					{/each}
				</h1>
				<p class="lede">{copy.hero.body}</p>
				<div class="ctas">
					<Button href="#install" size="lg"><Download class="size-4" />{copy.hero.primary}</Button>
					<Button href="#overview" variant="secondary" size="lg"
						>{copy.hero.secondary}<ArrowRight class="direction-arrow size-4" /></Button
					>
				</div>
				<ul class="facts">
					{#each copy.hero.facts as fact}
						<li><CheckCircle aria-hidden="true" class="size-3.5" />{fact}</li>
					{/each}
				</ul>
			</div>
			<div class="hero-demo">
				<div class="demo-fade">
					<Composer copy={copy.demo.composer} onmemory={() => engine?.cheer()} />
				</div>
				<i class="ta h-start" data-thread="10"></i>
			</div>
		</div>
		<div class="pillars-slot">
			<i class="ta h-exit" data-thread="12"></i>
			<nav id="overview" class="wrap pillars" aria-label={copy.hero.secondary}>
				{#each copy.pillars as pillar, index}
					{@const Icon = [Brain, Workflow, ShieldCheck][index]}
					<a href={`#${pillarTargets[index]}`}>
						<span class="pillar-index">0{index + 1}</span>
						<h2><Icon class="size-4" />{pillar.title}</h2>
						<p>{pillar.detail}</p>
					</a>
				{/each}
			</nav>
		</div>
	</section>

	<div class="interlude" data-interlude="paper" aria-hidden="true">
		<i class="ta i-a" data-thread="20"></i>
		<i class="ta i-b" data-thread="21" data-thread-loop="92"></i>
		<i class="ta i-c" data-thread="22" data-thread-loop="-30"></i>
		<div class="interlude-stick">
			<p class="marquee hollow"><span>{marquee(copy.hero.title, '  ')}</span></p>
			<p class="marquee filled reverse">
				<span
					>{marquee(
						copy.pillars.map((pillar) => pillar.title),
						'  ·  '
					)}</span
				>
			</p>
		</div>
	</div>

	<section id="memory" class="chapter is-paper" data-nav>
		<div class="wrap split">
			<div class="copy" data-reveal>
				<p class="eyebrow">{copy.memory.badge}</p>
				<h2>{copy.memory.title}</h2>
				<p class="lede">{copy.memory.body}</p>
				<ol class="beats">
					{#each copy.memory.features as feature, index}
						<li data-station={memoryStations[index]}>
							<span class="beat-index">0{index + 1}</span>
							<div>
								<h3>{feature.title}</h3>
								<p>{feature.detail}</p>
							</div>
						</li>
					{/each}
				</ol>
				<aside class="note">
					<span>{copy.previewLabel}</span>
					<p>{copy.memoryPreview}</p>
				</aside>
				<a
					class="text-link"
					href="https://docs.anda.bot/docs/memory/brain"
					target="_blank"
					rel="noreferrer">{copy.memoryLink}<ArrowRight class="direction-arrow size-4" /></a
				>
			</div>
			<MemoryBoard copy={copy.demo.memory} />
		</div>
	</section>

	<section id="action" class="chapter is-paper" data-nav>
		<div class="wrap">
			<div class="copy intro" data-reveal>
				<p class="eyebrow">{copy.action.badge}</p>
				<h2>{copy.action.title}</h2>
				<p class="lede">{copy.action.body}</p>
			</div>
			<ActionCards features={copy.action.features} copy={copy.demo.action} />
		</div>
	</section>

	<section id="control" class="chapter is-paper" data-nav>
		<i class="ta lane-only" data-thread="49"></i>
		<div class="wrap split control-split">
			<div class="copy" data-reveal>
				<p class="eyebrow">{copy.control.badge}</p>
				<h2>{copy.control.title}</h2>
				<p class="lede">{copy.control.body}</p>
				<ol class="beats">
					{#each copy.control.features as feature, index}
						<li data-station={controlStations[index]}>
							<span class="beat-index">0{index + 1}</span>
							<div>
								<h3>{feature.title}</h3>
								<p>{feature.detail}</p>
							</div>
						</li>
					{/each}
				</ol>
				<p class="data-note" data-station="ctl-out">
					<ShieldCheck class="size-5 shrink-0" />{copy.dataNote}
				</p>
				<div class="links">
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
			<MachineDiagram copy={copy.demo.control} />
		</div>
	</section>

	<section id="surfaces" class="chapter is-paper" data-nav>
		<i class="ta s-lead" data-thread="59"></i>
		<div class="wrap">
			<div class="copy intro surfaces-intro" data-reveal>
				<p class="eyebrow">{copy.surfaces.badge}</p>
				<h2>{copy.surfaces.title}</h2>
				<p class="lede">{copy.surfaces.body}</p>
				<div class="links">
					<a class="text-link" href={chromeExtensionStoreUrl} target="_blank" rel="noreferrer"
						>Chrome<ExternalLink class="size-3.5" /></a
					>
					<a class="text-link" href={edgeExtensionStoreUrl} target="_blank" rel="noreferrer"
						>Edge<ExternalLink class="size-3.5" /></a
					>
				</div>
			</div>
			<SurfaceFrames items={copy.surfaces.items} links={surfaceLinks} />
		</div>
	</section>

	<div class="interlude" data-interlude="night" aria-hidden="true">
		<i class="ta i-a" data-thread="70"></i>
		<i class="ta i-b" data-thread="71" data-thread-loop="-92"></i>
		<i class="ta i-c" data-thread="72" data-thread-loop="30"></i>
		<div class="interlude-stick">
			<p class="marquee hollow"><span>{marquee(copy.hero.facts, '  ✦  ')}</span></p>
			<p class="marquee filled reverse">
				<span
					>{marquee(
						copy.surfaces.items.map((item) => item.title),
						'  ·  '
					)}</span
				>
			</p>
		</div>
	</div>

	<section id="install" class="chapter is-night" data-nav>
		<i class="ta lane-only" data-thread="79"></i>
		<div class="wrap split install-split">
			<div class="copy" data-reveal>
				<p class="eyebrow">{copy.install.badge}</p>
				<h2>{copy.install.title}</h2>
				<p class="lede">{copy.install.body}</p>
				<p class="requirements">{copy.install.requirements}</p>
			</div>
			<InstallConsole copy={copy.install} {detectedOs} />
		</div>
	</section>

	<section class="chapter is-night faq" aria-labelledby="faq-title">
		<i class="ta lane-only" data-thread="85"></i>
		<div class="wrap split">
			<h2 id="faq-title">{copy.faqTitle}</h2>
			<div class="faq-list">
				<i class="ta f-top" data-thread="90"></i>
				{#each copy.faq as item}
					<details>
						<summary>{item.title}</summary>
						<p>{item.detail}</p>
					</details>
				{/each}
				<i class="ta f-bottom" data-thread="91"></i>
			</div>
		</div>
	</section>

	<section class="final is-night">
		<div class="wrap final-grid">
			<div class="copy" data-reveal>
				<h2>{copy.final.title}</h2>
				<p class="lede">{copy.final.body}</p>
				<div class="ctas">
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
			<PandaMark />
		</div>
	</section>

	<footer class="footer is-night">
		<div class="wrap">
			<a href="/" class="brand">
				<Anda />
				<span>Anda Bot</span>
			</a>
			<nav>
				<a href="/privacy">{info.common.privacy}</a>
				<a href="/terms">{info.common.terms}</a>
				<a href="/support">{info.common.support}</a>
				<a href="https://docs.anda.bot" target="_blank" rel="noreferrer">{info.common.docs}</a>
			</nav>
		</div>
	</footer>
</main>

<style>
	.landing {
		--night: #07110f;
		--paper: #f1eadc;
		--serif:
			'Iowan Old Style', 'Palatino Linotype', Palatino, 'Book Antiqua', 'New York', ui-serif,
			Georgia, 'Songti SC', 'Noto Serif CJK SC', 'Source Han Serif SC', 'Noto Naskh Arabic', serif;
		--mono:
			ui-monospace, 'SF Mono', 'JetBrains Mono', Menlo, Consolas, 'Liberation Mono', monospace;
		--theme: 0;
		position: relative;
		min-height: 100vh;
		overflow-x: clip;
		background: var(--night);
		font-feature-settings: 'ss01' 1;
	}
	/* --theme (0 night … 1 paper) is written by the engine only to the few
	   [data-themed] elements that change with it, so a theme frame never
	   restyles the whole page; each of them re-derives these colours. */
	.landing,
	.landing :global([data-themed]) {
		/* night → dawn → paper, so the transition glows instead of turning grey */
		--bg: color-mix(
			in oklab,
			color-mix(in oklab, var(--night), #a5673a clamp(0%, calc(var(--theme) * 200%), 100%)),
			var(--paper) clamp(0%, calc(var(--theme) * 200% - 100%), 100%)
		);
		--thread: color-mix(in oklab, #f1a64e, #c23b28 calc(var(--theme) * 100%));
		--hfg: color-mix(in oklab, #f4eee3, #14211c calc(var(--theme) * 100%));
	}
	:global(.landing.thread-ready) {
		background: transparent;
	}
	/* Arabic letters must stay joined: no tracking, and no monospace fallback */
	:global(.landing:lang(ar)) {
		--mono: 'SF Arabic', 'Geeza Pro', 'Noto Sans Arabic', system-ui, sans-serif;
	}
	:global(.landing:lang(ar) *) {
		letter-spacing: 0 !important;
	}
	.is-night {
		--fg: #f4eee3;
		--muted: #b3c0b0;
		--accent: #ffd08a;
		--accent-2: #35b2ab;
		--line: rgb(255 255 255 / 0.12);
		--card: rgb(255 255 255 / 0.04);
		color: var(--fg);
	}
	.is-paper {
		--fg: #14211c;
		--muted: #56645c;
		--accent: #b3421f;
		--accent-2: #1a746e;
		--line: rgb(20 33 28 / 0.13);
		--card: #fbf7ef;
		color: var(--fg);
	}
	/* Without the thread engine every section keeps its own ground. */
	:global(.landing:not(.thread-ready)) .is-paper {
		background: var(--paper);
	}
	:global(.landing:not(.thread-ready)) .interlude[data-interlude='paper'] {
		background: linear-gradient(180deg, var(--night), var(--paper));
	}
	:global(.landing:not(.thread-ready)) .interlude[data-interlude='night'] {
		background: linear-gradient(180deg, var(--paper), var(--night));
	}

	/* ---------- stage ---------- */
	.stage {
		position: fixed;
		inset: 0;
		z-index: 0;
		pointer-events: none;
		background: var(--night);
	}
	/* Night, dawn and paper layers crossfade by opacity alone, which the
	   compositor handles without repainting the screen. */
	.stage > div {
		position: absolute;
		inset: 0;
		will-change: opacity;
	}
	.stage-dawn {
		background: #a5673a;
		opacity: min(1, calc(var(--theme) * 2));
	}
	.stage-paper {
		background: var(--paper);
		opacity: max(0, calc(var(--theme) * 2 - 1));
	}
	.stage-glow {
		background:
			radial-gradient(60rem 36rem at 78% 8%, rgb(241 166 78 / 0.13), transparent 70%),
			radial-gradient(50rem 40rem at 8% 92%, rgb(53 178 171 / 0.1), transparent 70%);
		opacity: calc(1 - var(--theme));
	}
	.stage-grain {
		background-image: url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='220' height='220'%3E%3Cfilter id='n'%3E%3CfeTurbulence type='fractalNoise' baseFrequency='.8' numOctaves='3' stitchTiles='stitch'/%3E%3CfeColorMatrix values='0 0 0 0 .32 0 0 0 0 .24 0 0 0 0 .12 0 0 0 .5 0'/%3E%3C/filter%3E%3Crect width='100%25' height='100%25' filter='url(%23n)'/%3E%3C/svg%3E");
		opacity: calc(0.12 + var(--theme) * 0.28);
	}
	.thread-svg {
		position: absolute;
		top: 0;
		left: 0;
		z-index: 1;
		overflow: visible;
		pointer-events: none;
	}
	.landing > :global(section),
	.landing > .interlude,
	.landing > .footer {
		position: relative;
		z-index: 2;
	}

	/* ---------- Anda's trail ---------- */
	/* One warm tone for both grounds, so a theme change never restyles the dots. */
	:global(.t-trail circle) {
		fill: #d9823f;
		opacity: 0;
		transform-box: fill-box;
		transform-origin: center;
	}
	:global(.t-trail circle.on) {
		opacity: 0.32;
		animation: trail-spark 900ms ease-out;
	}
	:global(.ta) {
		position: absolute;
		width: 0;
		height: 0;
		pointer-events: none;
	}

	/* ---------- shared type ---------- */
	.wrap {
		width: 100%;
		max-width: 84rem;
		margin-inline: auto;
		padding-inline: clamp(1.25rem, 4vw, 3rem);
	}
	.eyebrow {
		display: inline-flex;
		align-items: center;
		gap: 0.6rem;
		color: var(--accent);
		font-family: var(--mono);
		font-size: 0.78rem;
		font-weight: 600;
		letter-spacing: 0.08em;
		text-transform: uppercase;
	}
	h2 {
		white-space: pre-line;
		color: var(--fg);
		font-family: var(--serif);
		font-size: clamp(2.3rem, 4.4vw, 4.1rem);
		font-weight: 400;
		line-height: 1.08;
		letter-spacing: -0.015em;
		text-wrap: balance;
	}
	.eyebrow + h2 {
		margin-top: 1.3rem;
	}
	.lede {
		max-width: 36rem;
		margin-top: 1.4rem;
		color: var(--muted);
		font-size: 1.08rem;
		line-height: 1.8;
	}
	.text-link {
		display: inline-flex;
		align-items: center;
		gap: 0.55rem;
		color: var(--accent);
		font-size: 0.92rem;
		font-weight: 600;
		text-underline-offset: 5px;
	}
	.text-link:hover {
		text-decoration: underline;
	}
	.links {
		display: flex;
		flex-wrap: wrap;
		gap: 0.8rem 2rem;
		margin-top: 1.8rem;
	}
	:global(.landing a:focus-visible),
	:global(.landing button:focus-visible),
	:global(.landing select:focus-visible),
	summary:focus-visible {
		outline: 2px solid var(--accent, var(--thread));
		outline-offset: 4px;
	}
	:global(.landing[dir='rtl'] .direction-arrow) {
		transform: scaleX(-1);
	}
	:global(.thread-ready) [data-reveal] > * {
		--e: clamp(0, calc((var(--enter, 1) - var(--d, 0)) * 2.4), 1);
		opacity: var(--e);
		transform: translateY(calc((1 - var(--e)) * 1.8rem));
	}
	[data-reveal] > :nth-child(2) {
		--d: 0.08;
	}
	[data-reveal] > :nth-child(3) {
		--d: 0.16;
	}
	[data-reveal] > :nth-child(n + 4) {
		--d: 0.24;
	}

	/* ---------- header ---------- */
	.site-header {
		position: fixed;
		inset: 0 0 auto;
		z-index: 50;
		display: flex;
		align-items: center;
		gap: 1.5rem;
		height: 4.25rem;
		padding-inline: clamp(1.25rem, 4vw, 3rem);
		color: var(--hfg);
		--scrolled: 0;
	}
	/* The frosted bar lives on a pseudo-element: a backdrop-filter on the
	   header itself would turn the language menu (which overflows it) see-through. */
	.site-header::before {
		position: absolute;
		inset: 0;
		z-index: -1;
		content: '';
		background: color-mix(in oklab, var(--bg) calc(var(--scrolled) * 80%), transparent);
		backdrop-filter: blur(calc(var(--scrolled) * 18px)) saturate(1.2);
		box-shadow: 0 1px 0 color-mix(in oklab, var(--hfg) calc(var(--scrolled) * 12%), transparent);
	}
	.brand {
		display: inline-flex;
		min-width: 0;
		align-items: center;
		gap: 0.7rem;
		font-size: 1.15rem;
		font-weight: 650;
	}
	.brand :global(.anda-mark) {
		width: 2.5rem;
		height: 2.5rem;
	}
	.site-header .brand {
		--mark-line: color-mix(in oklab, #f1a64e, #14211c calc(var(--theme) * 100%));
	}
	.site-header nav {
		display: flex;
		gap: 0.2rem;
		margin-inline: auto;
		font-size: 0.88rem;
		font-weight: 550;
	}
	.site-header nav a {
		position: relative;
		padding: 0.45rem 0.7rem;
		border-radius: 0.6rem;
		opacity: 0.78;
		transition:
			opacity 160ms ease,
			background 160ms ease;
	}
	.site-header nav a:hover,
	.site-header nav a[aria-current] {
		opacity: 1;
	}
	.site-header nav a:hover {
		background: color-mix(in oklab, var(--hfg) 8%, transparent);
	}
	.site-header nav a[aria-current]::after {
		position: absolute;
		inset-inline: 0.7rem;
		bottom: 0.1rem;
		height: 2px;
		border-radius: 2px;
		content: '';
		background: var(--thread);
	}
	.site-header .header-actions {
		margin-inline-start: auto;
	}
	.site-header nav + .header-actions {
		margin-inline-start: 0;
	}
	.site-header :global(.language-menu) {
		--menu-fg: var(--hfg);
		--menu-panel: color-mix(in oklab, var(--bg) 97%, transparent);
		--menu-accent: color-mix(in oklab, #ffd08a, #b3421f calc(var(--theme) * 100%));
	}
	.site-header :global(.github) {
		color: inherit;
	}
	.site-header :global(.github:hover) {
		background: color-mix(in oklab, var(--hfg) 9%, transparent);
		color: inherit;
	}
	.progress {
		position: absolute;
		inset-inline: 0;
		bottom: 0;
		height: 2px;
		background: var(--thread);
		transform: scaleX(var(--page, 0));
		transform-origin: 0 50%;
	}
	:global(.landing[dir='rtl']) .progress {
		transform-origin: 100% 50%;
	}

	/* ---------- hero ---------- */
	.hero {
		display: flex;
		min-height: 100svh;
		flex-direction: column;
		justify-content: flex-end;
		padding-top: 7rem;
	}
	.hero-grid {
		display: grid;
		flex: 1;
		grid-template-columns: minmax(0, 1.12fr) minmax(0, 0.88fr);
		align-items: center;
		gap: clamp(2rem, 5vw, 5rem);
	}
	.beacon {
		position: relative;
		width: 0.5rem;
		height: 0.5rem;
		border-radius: 50%;
		background: var(--thread);
	}
	.beacon::after {
		position: absolute;
		inset: -0.35rem;
		border: 1px solid var(--thread);
		border-radius: 50%;
		content: '';
		animation: ping 2.4s ease-out infinite;
	}
	.hero-title {
		margin-top: 1.6rem;
		color: var(--fg);
		font-family: var(--serif);
		font-size: clamp(3.3rem, 7.2vw, 7.4rem);
		font-weight: 400;
		line-height: 1;
		letter-spacing: -0.025em;
	}
	.line {
		display: block;
		padding-bottom: 0.08em;
		overflow: hidden;
		transform: translateX(calc(var(--hp, 0) * var(--drift)));
		opacity: calc(1 - var(--hp, 0) * 1.25);
	}
	.line:nth-child(1) {
		--drift: -9vw;
	}
	.line:nth-child(2) {
		--drift: 5vw;
	}
	.line:nth-child(3) {
		--drift: -15vw;
		color: var(--accent);
		font-style: italic;
	}
	:global(:lang(zh)) .line:nth-child(3),
	:global(:lang(ar)) .line:nth-child(3),
	:global(:lang(zh)) .hollow,
	:global(:lang(ar)) .hollow {
		font-style: normal;
	}
	:global(.landing[dir='rtl']) .line {
		transform: translateX(calc(var(--hp, 0) * var(--drift) * -1));
	}
	.line > span {
		display: inline-block;
		animation: rise 1.2s cubic-bezier(0.16, 0.9, 0.2, 1) both;
		animation-delay: calc(var(--i) * 120ms + 120ms);
	}
	.hero-copy > .lede,
	.ctas,
	.facts {
		transform: translateY(calc(var(--hp, 0) * -3.5rem));
		opacity: calc(1 - var(--hp, 0) * 1.6);
	}
	.hero .lede {
		max-width: 33rem;
		margin-top: 2rem;
		font-size: 1.12rem;
		animation: fade-up 1s 480ms cubic-bezier(0.2, 0.8, 0.2, 1) both;
	}
	.ctas {
		display: flex;
		flex-wrap: wrap;
		gap: 0.75rem;
		margin-top: 2.2rem;
	}
	.hero .ctas {
		animation: fade-up 1s 600ms cubic-bezier(0.2, 0.8, 0.2, 1) both;
	}
	.ctas :global([data-slot='button']) {
		height: auto;
		min-height: 2.9rem;
		white-space: normal;
		padding-inline: 1.15rem;
		border-radius: 0.8rem;
	}
	.facts {
		display: flex;
		flex-wrap: wrap;
		gap: 0.6rem 1.3rem;
		margin-top: 2rem;
		color: var(--muted);
		font-size: 0.82rem;
		animation: fade-up 1s 720ms cubic-bezier(0.2, 0.8, 0.2, 1) both;
	}
	.facts li {
		display: flex;
		align-items: center;
		gap: 0.4rem;
	}
	.facts :global(svg) {
		color: var(--accent-2);
	}
	.hero-demo {
		position: relative;
		padding-bottom: 2.2rem;
	}
	.demo-fade {
		opacity: calc(1 - var(--hp, 0) * 1.1);
		animation: fade-up 1.1s 300ms cubic-bezier(0.2, 0.8, 0.2, 1) both;
	}
	.h-start {
		bottom: 2.2rem;
		inset-inline-start: 50%;
	}
	.pillars-slot {
		position: relative;
	}
	.h-exit {
		top: 0;
		inset-inline-start: 94%;
	}
	.pillars {
		display: grid;
		grid-template-columns: repeat(3, minmax(0, 1fr));
		margin-top: 3.5rem;
		padding-bottom: 2.5rem;
		transform: translateY(calc(var(--hp, 0) * -2rem));
		opacity: calc(1 - var(--hp, 0) * 0.9);
	}
	.pillars a {
		padding: 1.3rem 1.6rem 0.4rem 0;
		border-top: 1px solid var(--line);
		transition: border-color 200ms ease;
	}
	.pillars a + a {
		padding-inline-start: 1.6rem;
		border-inline-start: 1px solid var(--line);
	}
	.pillars a:hover {
		border-top-color: var(--thread);
	}
	.pillar-index {
		color: var(--accent);
		font-family: var(--mono);
		font-size: 0.75rem;
	}
	.pillars h2 {
		display: flex;
		align-items: center;
		gap: 0.5rem;
		margin-top: 0.6rem;
		font-family: inherit;
		font-size: 1.08rem;
		font-weight: 650;
		letter-spacing: 0;
	}
	.pillars h2 :global(svg) {
		color: var(--accent-2);
	}
	.pillars p {
		margin-top: 0.45rem;
		color: var(--muted);
		font-size: 0.92rem;
		line-height: 1.65;
	}

	/* ---------- interludes ---------- */
	.interlude {
		height: 190vh;
	}
	.interlude-stick {
		position: sticky;
		top: 0;
		display: flex;
		height: 100vh;
		flex-direction: column;
		justify-content: center;
		gap: 1.2rem;
		overflow: hidden;
	}
	.marquee {
		white-space: nowrap;
		font-family: var(--serif);
		line-height: 1;
	}
	.marquee span {
		display: inline-block;
		transform: translateX(calc(var(--ip, 0.5) * -38% + 4%));
		will-change: transform;
	}
	.marquee.reverse span {
		transform: translateX(calc(var(--ip, 0.5) * 38% - 42%));
	}
	:global(.landing[dir='rtl']) .marquee span {
		transform: translateX(calc(var(--ip, 0.5) * 38% - 4%));
	}
	:global(.landing[dir='rtl']) .marquee.reverse span {
		transform: translateX(calc(var(--ip, 0.5) * -38% + 42%));
	}
	.hollow {
		color: transparent;
		font-size: clamp(4.5rem, 13vw, 12rem);
		font-style: italic;
		/* fixed tones: a colour that followed the theme would repaint this huge line */
		-webkit-text-stroke: 1.2px rgb(160 132 98 / 0.62);
	}
	.filled {
		color: rgb(150 128 100 / 0.6);
		font-size: clamp(1.6rem, 3.4vw, 3rem);
	}
	.i-a {
		top: 12%;
		inset-inline-start: 86%;
	}
	.i-b {
		top: 44%;
		inset-inline-start: 30%;
	}
	.i-c {
		top: 82%;
		inset-inline-start: 62%;
	}

	/* ---------- chapters ---------- */
	.chapter {
		padding-block: clamp(6rem, 14vh, 10rem);
		scroll-margin-top: 3rem;
	}
	.split {
		display: grid;
		grid-template-columns: minmax(0, 0.95fr) minmax(0, 1.05fr);
		align-items: start;
		gap: clamp(2.5rem, 6vw, 6rem);
	}
	.intro {
		max-width: 46rem;
	}
	.beats {
		display: grid;
		gap: 1.6rem;
		margin-top: 2.6rem;
	}
	.beats li {
		--a: var(--arrive, 1);
		display: grid;
		grid-template-columns: auto 1fr;
		gap: 1.1rem;
		opacity: calc(0.4 + var(--a) * 0.6);
		transition: opacity 200ms linear;
	}
	.beat-index {
		display: grid;
		width: 2.3rem;
		height: 2.3rem;
		place-items: center;
		border: 1.5px solid color-mix(in oklab, var(--accent) calc(30% + var(--a) * 70%), transparent);
		border-radius: 50%;
		background: color-mix(in oklab, var(--accent) calc(var(--a) * 100%), transparent);
		color: color-mix(in oklab, var(--accent), var(--card) calc(var(--a) * 100%));
		font-family: var(--mono);
		font-size: 0.75rem;
		font-weight: 600;
	}
	.beats h3 {
		color: var(--fg);
		font-size: 1.1rem;
		font-weight: 650;
		line-height: 1.4;
	}
	.beats p {
		margin-top: 0.35rem;
		color: var(--muted);
		line-height: 1.7;
	}
	.note {
		margin-top: 2rem;
		padding-inline-start: 1rem;
		border-inline-start: 2px solid var(--accent-2);
	}
	.note span {
		color: var(--accent-2);
		font-size: 0.75rem;
		font-weight: 650;
	}
	.note p {
		margin-top: 0.35rem;
		color: var(--muted);
		font-size: 0.88rem;
		line-height: 1.7;
	}
	.note + .text-link {
		margin-top: 1.8rem;
	}
	/* the copy waits while Anda works through the memory cards */
	#memory .copy {
		position: sticky;
		top: 6.5rem;
	}
	.control-split {
		align-items: center;
	}
	.data-note {
		display: flex;
		gap: 0.8rem;
		margin-top: 2.2rem;
		padding: 1rem 1.1rem;
		border-radius: 0.9rem;
		background: color-mix(in oklab, var(--accent-2) calc(6% + var(--arrive, 1) * 6%), transparent);
		color: var(--fg);
		font-size: 0.9rem;
		line-height: 1.7;
	}
	.data-note :global(svg) {
		margin-top: 0.15rem;
		color: var(--accent-2);
	}
	.surfaces-intro {
		margin-inline-start: auto;
		max-width: 40rem;
	}
	.surfaces-intro .links {
		margin-top: 1.6rem;
	}
	.s-lead {
		top: 3rem;
		inset-inline-start: 12%;
	}
	/* detours that keep the thread in the margin when the layout is one column */
	.lane-only {
		display: none;
		top: 2rem;
	}

	/* ---------- install / faq / final ---------- */
	.install-split {
		grid-template-columns: minmax(0, 0.8fr) minmax(0, 1.2fr);
		align-items: center;
	}
	.requirements {
		margin-top: 1.6rem;
		padding-inline-start: 1rem;
		border-inline-start: 2px solid var(--thread);
		color: var(--muted);
		font-size: 0.94rem;
		line-height: 1.75;
	}
	.faq .split {
		grid-template-columns: minmax(0, 0.7fr) minmax(0, 1.3fr);
	}
	.faq-list {
		position: relative;
	}
	.f-top {
		top: -1rem;
		inset-inline-start: -2rem;
	}
	.f-bottom {
		bottom: -1rem;
		inset-inline-start: -2rem;
	}
	details {
		border-bottom: 1px solid var(--line);
		padding-block: 1.3rem;
	}
	details:first-of-type {
		padding-top: 0;
	}
	summary {
		display: flex;
		cursor: pointer;
		align-items: baseline;
		justify-content: space-between;
		gap: 1.5rem;
		color: var(--fg);
		font-family: var(--serif);
		font-size: 1.3rem;
		line-height: 1.4;
		list-style: none;
	}
	summary::-webkit-details-marker {
		display: none;
	}
	summary::after {
		flex-shrink: 0;
		color: var(--thread);
		content: '+';
		font-family: var(--mono);
		font-size: 1.2rem;
		transition: transform 240ms ease;
	}
	details[open] summary::after {
		transform: rotate(45deg);
	}
	details p {
		max-width: 44rem;
		margin-top: 0.9rem;
		color: var(--muted);
		line-height: 1.8;
	}
	.final {
		display: flex;
		min-height: 100svh;
		align-items: center;
		padding-block: 6rem 4rem;
	}
	.final-grid {
		display: grid;
		grid-template-columns: minmax(0, 1fr) minmax(0, 0.9fr);
		align-items: center;
		gap: clamp(2rem, 6vw, 6rem);
	}
	.final h2 {
		font-size: clamp(2.6rem, 5.2vw, 5rem);
	}
	.footer {
		padding-block: 2rem;
		border-top: 1px solid var(--line);
	}
	.footer .wrap {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		justify-content: space-between;
		gap: 1.2rem;
	}
	.footer .brand {
		font-size: 1rem;
	}
	.footer nav {
		display: flex;
		flex-wrap: wrap;
		gap: 0.5rem 1.4rem;
		color: var(--muted);
		font-size: 0.88rem;
	}
	.footer nav a:hover {
		color: var(--fg);
	}

	@keyframes rise {
		from {
			transform: translateY(105%);
		}
	}
	@keyframes fade-up {
		from {
			opacity: 0;
			transform: translateY(1.2rem);
		}
	}
	@keyframes -global-trail-spark {
		from {
			opacity: 1;
			transform: scale(2.4);
		}
	}
	@keyframes ping {
		from {
			opacity: 0.9;
			transform: scale(0.6);
		}
		to {
			opacity: 0;
			transform: scale(1.8);
		}
	}

	@media (max-width: 1100px) {
		.site-header nav {
			display: none;
		}
	}
	@media (max-width: 1023px) {
		/* single column: content starts past a margin lane the thread runs down */
		.landing {
			--lane: 1.5rem;
			--lane-in: -1.25rem;
		}
		.wrap {
			padding-inline-start: 2.75rem;
		}
		.hero-grid,
		.split,
		.install-split,
		.faq .split,
		.final-grid {
			grid-template-columns: minmax(0, 1fr);
		}
		.hero-demo {
			width: min(30rem, 100%);
		}
		#memory .copy {
			position: static;
		}
		.surfaces-intro {
			margin-inline-start: 0;
		}
		.lane-only {
			display: block;
		}
		.h-exit,
		.i-a,
		.i-c,
		.s-lead,
		.lane-only {
			inset-inline-start: var(--lane);
		}
		.i-b {
			inset-inline-start: 34%;
		}
		.f-top,
		.f-bottom {
			inset-inline-start: var(--lane-in);
		}
		.final-grid :global(.panda) {
			order: -1;
			width: min(24rem, 80%);
			margin-inline: 0;
		}
	}
	@media (max-width: 767px) {
		.site-header {
			height: 3.75rem;
			gap: 0.8rem;
		}
		.brand :global(.anda-mark) {
			width: 2.2rem;
			height: 2.2rem;
		}
		.site-header :global(.github span) {
			display: none;
		}
		.hero {
			padding-top: 5.5rem;
		}
		.hero-title {
			font-size: clamp(2.9rem, 13.5vw, 4.6rem);
		}
		.hero-demo {
			margin-top: 0.5rem;
		}
		.pillars {
			grid-template-columns: 1fr;
			margin-top: 2.5rem;
		}
		.pillars a,
		.pillars a + a {
			padding: 1.1rem 0 0.9rem;
			border-inline-start: 0;
		}
		.interlude {
			height: 170vh;
		}
		.chapter {
			padding-block: 5rem;
		}
		.final {
			min-height: auto;
			padding-top: 4rem;
		}
		.final-grid :global(.panda) {
			width: min(20rem, 80%);
		}
	}
</style>
