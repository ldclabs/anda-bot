<script lang="ts">
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import Anda from '$lib/components/ui/icons/anda.svelte';
	import Github from '$lib/components/ui/icons/github.svelte';
	import {
		detectLocale,
		fallbackLocale,
		isLocale,
		localeMeta,
		localeOrder,
		type Locale
	} from '$lib/content/landing';
	import { infoCopy, type InfoPageKind } from '$lib/content/info';
	import { ArrowLeft, BookOpen, ExternalLink, Languages } from '@lucide/svelte';
	import { onMount } from 'svelte';

	let { kind }: { kind: InfoPageKind } = $props();

	const localeStorageKey = 'anda-bot-landing-locale';

	let activeLocale = $state<Locale>(fallbackLocale);
	let copy = $derived(infoCopy[activeLocale]);
	let page = $derived(copy.pages[kind]);
	let activeDirection = $derived(localeMeta[activeLocale].dir);

	$effect(() => {
		document.documentElement.lang = localeMeta[activeLocale].htmlLang;
		document.documentElement.dir = localeMeta[activeLocale].dir;
	});

	function selectLocale(locale: Locale) {
		activeLocale = locale;
		try {
			localStorage.setItem(localeStorageKey, locale);
		} catch {
			// Ignore private browsing storage failures.
		}
	}

	function isExternal(href: string) {
		return /^https?:\/\//i.test(href);
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
	});
</script>

<svelte:head>
	<title>{page.meta.title}</title>
	<meta name="description" content={page.meta.description} />
	<meta property="og:title" content={page.meta.title} />
	<meta property="og:description" content={page.meta.description} />
	<meta content="website" property="og:type" />
	<meta content={`https://anda.bot/${kind}`} property="og:url" />
	<meta content="https://anda.bot/_assets/images/anda_bot.webp" property="og:image" />
	<meta content="summary_large_image" name="twitter:card" />
</svelte:head>

<main dir={activeDirection} class="info-shell min-h-screen overflow-x-clip text-(--anda-parchment)">
	<div class="info-map pointer-events-none fixed inset-0 opacity-70"></div>
	<header
		class="relative z-20 mx-auto flex w-full max-w-7xl items-center justify-between px-5 py-5 sm:px-6 lg:px-8"
	>
		<a href="/" class="group inline-flex items-center gap-3 text-sm font-semibold text-white">
			<img src="/_assets/logo.hdr.png" alt="Anda Bot" class="hdr-img size-12 rounded-lg" />
			<span class="text-2xl">Anda Bot</span>
		</a>

		<nav
			class="hidden items-center gap-1 rounded-lg border border-white/10 bg-black/20 p-1 text-sm text-white/74 backdrop-blur-xl md:flex"
			aria-label={copy.common.navigationLabel}
		>
			<a href="/" class="rounded-md px-3 py-2 transition hover:bg-white/8 hover:text-white">
				{copy.common.home}
			</a>
			<a href="/privacy" class="rounded-md px-3 py-2 transition hover:bg-white/8 hover:text-white">
				{copy.common.privacy}
			</a>
			<a href="/terms" class="rounded-md px-3 py-2 transition hover:bg-white/8 hover:text-white">
				{copy.common.terms}
			</a>
			<a href="/support" class="rounded-md px-3 py-2 transition hover:bg-white/8 hover:text-white">
				{copy.common.support}
			</a>
		</nav>

		<div class="header-actions">
			<label class="language-switcher mr-2">
				<Languages class="size-4" />
				<span class="sr-only">{copy.common.languageLabel}</span>
				<select
					aria-label={copy.common.languageLabel}
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
				href="https://docs.anda.bot"
				target="_blank"
				rel="noreferrer"
				variant="ghost"
				size="sm"
			>
				<BookOpen class="size-5" />
				{copy.common.docs}
			</Button>

			<Button
				href="https://github.com/ldclabs/anda-bot"
				target="_blank"
				rel="noreferrer"
				variant="ghost"
				size="sm"
			>
				<Github class="size-6" />
				{copy.common.github}
			</Button>
		</div>
	</header>

	<section class="relative z-10 mx-auto max-w-7xl px-5 pt-8 pb-8 sm:px-6 lg:px-8 lg:pt-16">
		<a href="/" class="mb-8 inline-flex items-center gap-2 text-sm text-white/62 hover:text-white">
			<ArrowLeft class="size-4" />
			{copy.common.home}
		</a>

		<div class="max-w-4xl">
			<Badge tone="warm" class="gap-2">{page.eyebrow}</Badge>
			<h1 class="anda-display mt-6 text-5xl leading-tight text-white sm:text-6xl lg:text-7xl">
				{page.title}
			</h1>
			<p class="mt-6 max-w-3xl text-lg leading-8 text-white/72 sm:text-xl">
				{page.intro}
			</p>
			<p class="mt-5 font-mono text-sm text-(--anda-amber-soft)">
				{copy.common.updatedLabel}: {page.updated}
			</p>
		</div>
	</section>

	<section
		class="relative z-10 mx-auto grid max-w-7xl gap-5 px-5 pb-16 sm:px-6 lg:grid-cols-[minmax(0,0.72fr)_minmax(280px,0.28fr)] lg:px-8 lg:pb-24"
	>
		<article class="space-y-4">
			{#each page.sections as section, index}
				<section class="info-section-card">
					<div class="flex items-start gap-4">
						<span class="info-section-number">{String(index + 1).padStart(2, '0')}</span>
						<div class="min-w-0 flex-1">
							<h2 class="text-2xl font-semibold text-white">{section.title}</h2>

							{#if section.body}
								<div class="mt-4 space-y-4 text-base leading-8 text-white/68">
									{#each section.body as paragraph}
										<p>{paragraph}</p>
									{/each}
								</div>
							{/if}

							{#if section.items}
								<div class="mt-5 grid gap-3 md:grid-cols-2">
									{#each section.items as item}
										<div class="info-item">
											<h3 class="font-semibold text-(--anda-amber-soft)">{item.title}</h3>
											<p class="mt-2 leading-7 text-white/64">{item.detail}</p>
										</div>
									{/each}
								</div>
							{/if}
						</div>
					</div>
				</section>
			{/each}
		</article>

		<aside class="info-rail h-fit lg:sticky lg:top-6">
			<div class="flex items-center gap-3 border-b border-white/10 pb-4">
				<div class="grid size-11 place-items-center rounded-lg bg-white/8 text-(--anda-amber-soft)">
					<Anda class="size-7" />
				</div>
				<div>
					<p class="text-sm font-semibold text-white">Anda Bot</p>
					<p class="text-xs text-white/52">{copy.common.moreLinks}</p>
				</div>
			</div>

			<nav class="mt-4 grid gap-2" aria-label={copy.common.moreLinks}>
				<a
					class={`info-rail-link ${kind === 'privacy' ? 'info-rail-link-active' : ''}`}
					href="/privacy"
				>
					{copy.common.privacy}
				</a>
				<a
					class={`info-rail-link ${kind === 'terms' ? 'info-rail-link-active' : ''}`}
					href="/terms"
				>
					{copy.common.terms}
				</a>
				<a
					class={`info-rail-link ${kind === 'support' ? 'info-rail-link-active' : ''}`}
					href="/support"
				>
					{copy.common.support}
				</a>
			</nav>

			{#if page.actions?.length}
				<div class="mt-5 grid gap-2 border-t border-white/10 pt-5">
					{#each page.actions as action}
						<Button
							href={action.href}
							target={isExternal(action.href) ? '_blank' : undefined}
							rel={isExternal(action.href) ? 'noreferrer' : undefined}
							variant="secondary"
							class="justify-between"
						>
							{action.label}
							{#if isExternal(action.href)}
								<ExternalLink class="size-4" />
							{/if}
						</Button>
					{/each}
				</div>
			{/if}
		</aside>
	</section>
</main>
