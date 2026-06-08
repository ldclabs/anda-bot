<script lang="ts">
	import GithubIcon from '$lib/components/ui/icons/github.svelte';
	import {
		ArrowRight,
		BookOpen,
		Code,
		FileText,
		Folder,
		GitBranch,
		LayoutPanelLeft,
		Menu,
		MessageSquare,
		Network,
		RefreshCcw,
		Search,
		Settings,
		Sparkles,
		Star,
		Terminal,
		Wrench
	} from '@lucide/svelte';

	type PreviewVariant = 'hero' | 'section';

	let { variant = 'section' }: { variant?: PreviewVariant } = $props();

	const tabs = ['ldclabs/anda-bot: Born of panda', 'New Tab'];
	const repoRows = [
		{
			type: 'folder',
			name: '.github/workflows',
			commit: 'ci: add optional Windows support',
			time: '6 hours ago'
		},
		{
			type: 'folder',
			name: 'anda_bot',
			commit: 'chore: prepare v0.9.0 release',
			time: '2 hours ago'
		},
		{
			type: 'folder',
			name: 'chrome-extension',
			commit: 'feat: improve installer UX',
			time: '8 hours ago'
		},
		{
			type: 'folder',
			name: 'docsite',
			commit: 'feat: improve launcher docs',
			time: '17 hours ago'
		},
		{
			type: 'folder',
			name: 'scripts',
			commit: 'fix: use proper launcher app path',
			time: '5 hours ago'
		},
		{
			type: 'folder',
			name: 'skills',
			commit: 'fix: normalize local file URI',
			time: '18 hours ago'
		},
		{
			type: 'folder',
			name: 'website',
			commit: 'chore: remove IRC channel copy',
			time: 'yesterday'
		},
		{
			type: 'file',
			name: 'AGENTS.md',
			commit: 'feat: queue extension follow-ups',
			time: '3 days ago'
		},
		{
			type: 'file',
			name: 'CHANGELOG.md',
			commit: 'chore: prepare v0.9.0 release',
			time: '2 hours ago'
		},
		{
			type: 'file',
			name: 'Cargo.toml',
			commit: 'feat: localize launcher text',
			time: '18 hours ago'
		},
		{ type: 'file', name: 'README.md', commit: 'feat: improve launcher docs', time: '17 hours ago' }
	];
	const channels = [
		{
			name: 'CLI anda-bot',
			state: 'completed',
			source: '~/projects/anda-bot',
			count: '#464 / 38'
		},
		{
			name: 'CLI anda',
			state: 'canceled',
			source: '~/projects/anda-bot',
			count: '#444 / 13'
		},
		{
			name: 'Chrome',
			state: 'idle',
			source: 'chrome:1778757605379',
			count: '#443 / 2',
			active: true
		},
		{
			name: 'CLI anda-brain',
			state: 'completed',
			source: '~/projects/anda-bot',
			count: '#439 / 48'
		},
		{
			name: 'CLI anda-db',
			state: 'completed',
			source: '~/projects/anda-bot',
			count: '#428 / 60'
		},
		{
			name: 'CLI weixin-agent...',
			state: 'completed',
			source: '~/projects/anda-bot',
			count: '#426 / 44'
		},
		{
			name: 'CLI KIP',
			state: 'completed',
			source: '~/projects/anda-bot',
			count: '#421 / 98'
		},
		{ name: 'Edge', state: 'completed', source: 'edge:779451008846', count: '#408 / 2' },
		{ name: 'Wechat', state: 'canceled', source: 'wechat:personal:reply...', count: '#47 / 14' }
	];
	const messageBlocks = [
		'不是重复造轮子，而是目标不一样。',
		'Codex 适合主力写代码；Anda Bot 负责长期记忆、上下文和日常杂活。',
		'模型可以换，记忆不能丢。Anda Brain 才是属于用户的智能资产。'
	];
</script>

<div
	class={`browser-preview browser-preview-${variant}`}
	dir="ltr"
	aria-label="Anda Bot browser side panel preview"
>
	<div class="browser-preview-shell">
		<div class="browser-preview-titlebar">
			<div class="browser-preview-window-controls" aria-hidden="true">
				<span></span>
				<span></span>
				<span></span>
			</div>
			<div class="browser-preview-tabs">
				{#each tabs as tab, index}
					<div class={`browser-preview-tab ${index === 0 ? 'browser-preview-tab-active' : ''}`}>
						<span>{tab}</span>
					</div>
				{/each}
			</div>
		</div>

		<div class="browser-preview-addressbar">
			<div class="browser-preview-actions" aria-hidden="true">
				<span></span>
				<span></span>
				<span></span>
			</div>
			<div class="browser-preview-url">
				<GithubIcon class="size-3.5" />
				<span>github.com/ldclabs/anda-bot</span>
			</div>
			<div class="browser-preview-toolbar">
				<LayoutPanelLeft class="size-3.5" />
				<Settings class="size-3.5" />
			</div>
		</div>

		<div class="browser-preview-body">
			<section class="browser-preview-github-page" aria-label="GitHub repository preview">
				<div class="browser-preview-github-nav">
					<Menu class="size-4" />
					<GithubIcon class="size-5" />
					<strong>ldclabs</strong>
					<span>/</span>
					<strong>anda-bot</strong>
					<div class="browser-preview-github-search">
						<Search class="size-3.5" />
					</div>
				</div>
				<div class="browser-preview-github-tabs">
					<strong>Code</strong>
					<span>Issues</span>
					<span>Pull requests</span>
					<span>Agents</span>
					<span>Actions</span>
					<span>More</span>
				</div>

				<div class="browser-preview-repo-layout">
					<div class="browser-preview-repo-main">
						<div class="browser-preview-repo-title">
							<div>
								<GithubIcon class="size-5" />
								<strong>anda-bot</strong>
								<span>Public</span>
							</div>
							<div>
								<span><Star class="size-3.5" /> Starred 19</span>
							</div>
						</div>

						<div class="browser-preview-repo-toolbar">
							<span><GitBranch class="size-3.5" /> main</span>
							<button>Go to file</button>
							<button><Code class="size-3.5" /> Code</button>
						</div>

						<div class="browser-preview-commit-row">
							<strong>zensh and anda-ai</strong>
							<span>chore: prepare v0.9.0 release</span>
							<small>2 hours ago</small>
						</div>

						<div class="browser-preview-file-list">
							{#each repoRows as row}
								<div class="browser-preview-file-row">
									{#if row.type === 'folder'}
										<Folder class="size-4" />
									{:else}
										<FileText class="size-4" />
									{/if}
									<strong>{row.name}</strong>
									<span>{row.commit}</span>
									<small>{row.time}</small>
								</div>
							{/each}
						</div>
					</div>

					<aside class="browser-preview-repo-about">
						<strong>About</strong>
						<p>A memory-first local assistant for your computer, powered by the Anda Brain.</p>
						<span><BookOpen class="size-3.5" /> Readme</span>
						<span><Star class="size-3.5" /> 19 stars</span>
						<strong>Releases</strong>
						<mark>memory-first</mark>
					</aside>
				</div>
			</section>

			<aside class="browser-preview-panel" aria-label="Anda Bot side panel preview">
				<div class="browser-preview-panel-title">
					<div class="browser-preview-brand">
						<img src="/_assets/logo.hdr.png" alt="" class="hdr-img" />
						<strong>Anda Bot</strong>
					</div>
					<div class="browser-preview-panel-actions">
						<LayoutPanelLeft class="size-3.5" />
						<span>×</span>
					</div>
				</div>

				<div class="browser-preview-app">
					<div class="browser-preview-channel-rail">
						<div class="browser-preview-rail-header">
							<RefreshCcw class="size-4" />
							<strong>Channels</strong>
							<span>20</span>
						</div>
						<div class="browser-preview-channel-list">
							{#each channels as channel}
								<div
									class={`browser-preview-channel ${channel.active ? 'browser-preview-channel-active' : ''}`}
								>
									{#if channel.active}
										<Network class="browser-preview-channel-icon size-4" />
									{:else}
										<Terminal class="browser-preview-channel-icon size-4" />
									{/if}
									<div>
										<strong>{channel.name}</strong>
										<span>{channel.state} {channel.source}</span>
									</div>
									<small>{channel.count}</small>
								</div>
							{/each}
						</div>
					</div>

					<div class="browser-preview-chat">
						<div class="browser-preview-chat-header">
							<span class="browser-preview-status-pill">
								<span></span>
								idle
							</span>
							<strong>browser:chrome:1778757605379</strong>
							<Settings class="size-4" />
						</div>
						<div class="browser-preview-thread">
							{#each messageBlocks as block}
								<p>{block}</p>
							{/each}
						</div>
						<div class="browser-preview-composer">
							<span>Message Anda</span>
							<div>
								<Wrench class="size-4" />
								<MessageSquare class="size-4" />
							</div>
							<ArrowRight class="size-4" />
						</div>
					</div>
				</div>
			</aside>
		</div>
	</div>
</div>
