<script lang="ts">
  import { andaClient } from '$lib/anda/client/side-panel.svelte'
  import { type SettingsState, type SubmitKeyMode } from '$lib/anda/client/types'
  import { Button } from '$lib/components/ui/button/index.js'
  import { Input } from '$lib/components/ui/input/index.js'
  import {
    Check,
    ChevronDown,
    Clipboard,
    Download,
    ExternalLink,
    FileCog,
    KeyRound,
    Keyboard,
    LoaderCircle,
    Play,
    PlugZap,
    Save,
    Terminal
  } from '@lucide/svelte'
  import { onMount } from 'svelte'

  let { setupGuideOpen } = $props<{ setupGuideOpen: boolean }>()

  let draftSettings = $state<SettingsState>({
    baseUrl: 'http://127.0.0.1:8042',
    token: '',
    submitKeyMode: 'enter'
  })
  let settingsDirty = $state(false)
  let savingSettings = $state(false)
  let testingConnection = $state(false)
  let copiedCommand = $state('')

  const installCommand = 'brew install ldclabs/tap/anda'
  const installScriptCommand =
    'curl -fsSL https://raw.githubusercontent.com/ldclabs/anda-bot/main/scripts/install.sh | sh'
  const windowsInstallCommand =
    'irm https://raw.githubusercontent.com/ldclabs/anda-bot/main/scripts/install.ps1 | iex'
  const launchCommand = 'anda start'
  const tokenCommand = 'anda browser token --days 365'

  function markSettingsDirty() {
    settingsDirty = true
  }

  function updateSubmitKeyMode(submitKeyMode: SubmitKeyMode) {
    if (draftSettings.submitKeyMode === submitKeyMode) {
      return
    }
    draftSettings = { ...draftSettings, submitKeyMode }
    markSettingsDirty()
  }

  function submitKeyModeButtonClass(submitKeyMode: SubmitKeyMode): string {
    const base = 'min-w-0 rounded-md px-2 py-1.5 text-left transition'
    return draftSettings.submitKeyMode === submitKeyMode
      ? `${base} bg-white text-stone-900 shadow-sm ring-1 ring-emerald-600/20`
      : `${base} text-stone-500 hover:bg-white/70 hover:text-stone-800`
  }

  async function copyCommand(command: string) {
    try {
      await navigator.clipboard.writeText(command)
      copiedCommand = command
      window.setTimeout(() => {
        if (copiedCommand === command) {
          copiedCommand = ''
        }
      }, 1400)
    } catch (error) {
      console.warn('Failed to copy setup command', error)
    }
  }

  async function saveSettings() {
    if (savingSettings) {
      return
    }
    savingSettings = true
    try {
      await andaClient.saveSettings(draftSettings)
      settingsDirty = false
      if (draftSettings.token.trim()) {
        setupGuideOpen = false
      }
      draftSettings = { ...andaClient.settings }
    } finally {
      savingSettings = false
    }
  }

  async function testConnection() {
    if (testingConnection) {
      return
    }
    testingConnection = true
    try {
      await andaClient.testConnection(draftSettings)
      settingsDirty = false
      draftSettings = { ...andaClient.settings }
    } catch (_error) {
    } finally {
      testingConnection = false
    }
  }

  onMount(() => {
    draftSettings = { ...andaClient.settings }
  })
</script>

<section
  class="scrollbar-slim grid gap-4 border-b border-stone-200 bg-[#fbfcfa] px-3 py-3 shadow"
  aria-label={chrome.i18n.getMessage('settings')}
>
  <div class="overflow-hidden rounded-lg border border-emerald-900/10 bg-white shadow-xs">
    <button
      type="button"
      class="grid w-full grid-cols-[1fr_auto] items-start gap-3 px-3 py-3 text-left transition-colors hover:bg-emerald-50/45 focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-emerald-700"
      aria-expanded={setupGuideOpen}
      aria-controls="local-setup-guide"
      aria-label={setupGuideOpen
        ? chrome.i18n.getMessage('collapseLocalSetup')
        : chrome.i18n.getMessage('expandLocalSetup')}
      onclick={() => (setupGuideOpen = !setupGuideOpen)}
    >
      <span class="grid min-w-0 gap-1">
        <span class="flex items-center gap-1.5 text-[11px] font-bold text-emerald-800">
          <Terminal class="size-3.5" />
          <span>{chrome.i18n.getMessage('onboardingEyebrow')}</span>
        </span>
        <span class="text-sm font-bold text-stone-900">
          {chrome.i18n.getMessage('onboardingTitle')}
        </span>
        <span class="text-[11px] leading-relaxed text-stone-600">
          {chrome.i18n.getMessage('onboardingIntro')}
        </span>
      </span>
      <ChevronDown
        class={`mt-0.5 size-4 shrink-0 text-emerald-800 transition-transform ${setupGuideOpen ? 'rotate-180' : ''}`}
      />
    </button>

    {#if setupGuideOpen}
      <div
        id="local-setup-guide"
        class="scrollbar-slim grid max-h-80 gap-3 overflow-y-auto border-t border-stone-200 px-3 py-3"
      >
        <div class="grid grid-cols-[1.75rem_1fr] gap-2 pb-2">
          <div
            class="grid size-7 place-items-center rounded-md border border-sky-900/10 bg-sky-50 text-sky-800"
          >
            <Download class="size-3.5" />
          </div>
          <div class="grid min-w-0 gap-2">
            <div class="grid gap-0.5">
              <h3 class="text-xs font-bold text-stone-800">
                {chrome.i18n.getMessage('onboardingInstallTitle')}
              </h3>
              <p class="text-[11px] leading-relaxed text-stone-600">
                {chrome.i18n.getMessage('onboardingInstallBody')}
              </p>
            </div>

            <div class="grid gap-1.5">
              <div class="flex items-center justify-between gap-2">
                <span class="text-[10px] font-semibold text-stone-500">Homebrew</span>
                <Button
                  variant="ghost"
                  size="icon-xs"
                  aria-label={chrome.i18n.getMessage('copyCommand')}
                  title={chrome.i18n.getMessage('copyCommand')}
                  onclick={() => copyCommand(installCommand)}
                >
                  {#if copiedCommand === installCommand}
                    <Check class="size-3 text-emerald-700" />
                  {:else}
                    <Clipboard class="size-3" />
                  {/if}
                </Button>
              </div>
              <code
                class="block min-w-0 overflow-x-auto rounded-md border border-stone-200 bg-white px-2 py-1.5 font-mono text-[11px] leading-relaxed text-stone-800 shadow-xs"
                >{installCommand}</code
              >
            </div>

            <div class="grid gap-1.5">
              <div class="flex items-center justify-between gap-2">
                <span class="text-[10px] font-semibold text-stone-500">
                  {chrome.i18n.getMessage('macLinux')}
                </span>
                <Button
                  variant="ghost"
                  size="icon-xs"
                  aria-label={chrome.i18n.getMessage('copyCommand')}
                  title={chrome.i18n.getMessage('copyCommand')}
                  onclick={() => copyCommand(installScriptCommand)}
                >
                  {#if copiedCommand === installScriptCommand}
                    <Check class="size-3 text-emerald-700" />
                  {:else}
                    <Clipboard class="size-3" />
                  {/if}
                </Button>
              </div>
              <code
                class="block min-w-0 overflow-x-auto rounded-md border border-stone-200 bg-white px-2 py-1.5 font-mono text-[11px] leading-relaxed text-stone-800 shadow-xs"
                >{installScriptCommand}</code
              >
            </div>

            <div class="grid gap-1.5">
              <div class="flex items-center justify-between gap-2">
                <span class="text-[10px] font-semibold text-stone-500">Windows PowerShell</span>
                <Button
                  variant="ghost"
                  size="icon-xs"
                  aria-label={chrome.i18n.getMessage('copyCommand')}
                  title={chrome.i18n.getMessage('copyCommand')}
                  onclick={() => copyCommand(windowsInstallCommand)}
                >
                  {#if copiedCommand === windowsInstallCommand}
                    <Check class="size-3 text-emerald-700" />
                  {:else}
                    <Clipboard class="size-3" />
                  {/if}
                </Button>
              </div>
              <code
                class="block min-w-0 overflow-x-auto rounded-md border border-stone-200 bg-white px-2 py-1.5 font-mono text-[11px] leading-relaxed text-stone-800 shadow-xs"
                >{windowsInstallCommand}</code
              >
            </div>
          </div>
        </div>

        <div class="grid grid-cols-[1.75rem_1fr] gap-2 pb-2">
          <div
            class="grid size-7 place-items-center rounded-md border border-amber-900/10 bg-amber-50 text-amber-800"
          >
            <FileCog class="size-3.5" />
          </div>
          <div class="grid min-w-0 gap-2">
            <div class="grid gap-0.5">
              <h3 class="text-xs font-bold text-stone-800">
                {chrome.i18n.getMessage('onboardingConfigureTitle')}
              </h3>
              <p class="text-[11px] leading-relaxed text-stone-600">
                {chrome.i18n.getMessage('onboardingConfigureBody')}
              </p>
            </div>

            <div class="grid gap-1.5">
              <div class="flex items-center justify-between gap-2">
                <span class="text-[10px] font-semibold text-stone-500">
                  {chrome.i18n.getMessage('launchCommandLabel')}
                </span>
                <Button
                  variant="ghost"
                  size="icon-xs"
                  aria-label={chrome.i18n.getMessage('copyCommand')}
                  title={chrome.i18n.getMessage('copyCommand')}
                  onclick={() => copyCommand(launchCommand)}
                >
                  {#if copiedCommand === launchCommand}
                    <Check class="size-3 text-emerald-700" />
                  {:else}
                    <Clipboard class="size-3" />
                  {/if}
                </Button>
              </div>
              <code
                class="block min-w-0 overflow-x-auto rounded-md border border-stone-200 bg-white px-2 py-1.5 font-mono text-[11px] leading-relaxed text-stone-800 shadow-xs"
                >{launchCommand}</code
              >
            </div>
          </div>
        </div>

        <div class="grid grid-cols-[1.75rem_1fr] gap-2 pb-2">
          <div
            class="grid size-7 place-items-center rounded-md border border-emerald-900/10 bg-emerald-50 text-emerald-800"
          >
            <KeyRound class="size-3.5" />
          </div>
          <div class="grid min-w-0 gap-2">
            <div class="grid gap-0.5">
              <h3 class="text-xs font-bold text-stone-800">
                {chrome.i18n.getMessage('onboardingTokenTitle')}
              </h3>
              <p class="text-[11px] leading-relaxed text-stone-600">
                {chrome.i18n.getMessage('onboardingTokenBody')}
              </p>
            </div>

            <div class="grid gap-1.5">
              <div class="flex items-center justify-between gap-2">
                <span class="text-[10px] font-semibold text-stone-500">
                  {chrome.i18n.getMessage('tokenCommandLabel')}
                </span>
                <Button
                  variant="ghost"
                  size="icon-xs"
                  aria-label={chrome.i18n.getMessage('copyCommand')}
                  title={chrome.i18n.getMessage('copyCommand')}
                  onclick={() => copyCommand(tokenCommand)}
                >
                  {#if copiedCommand === tokenCommand}
                    <Check class="size-3 text-emerald-700" />
                  {:else}
                    <Clipboard class="size-3" />
                  {/if}
                </Button>
              </div>
              <code
                class="block min-w-0 overflow-x-auto rounded-md border border-stone-200 bg-white px-2 py-1.5 font-mono text-[11px] leading-relaxed text-stone-800 shadow-xs"
                >{tokenCommand}</code
              >
            </div>
          </div>
        </div>
      </div>
    {/if}
  </div>

  <div class="grid gap-3 border-t border-stone-200 pt-3">
    <div class="flex items-center justify-between gap-2">
      <div class="flex min-w-0 items-center gap-1.5 text-xs font-bold text-stone-800">
        <Play class="size-3.5 text-emerald-800" />
        <span class="truncate">{chrome.i18n.getMessage('connectionDetails')}</span>
      </div>
      {#if andaClient.settings.token}
        <span
          class="inline-flex shrink-0 items-center gap-1 rounded-md bg-emerald-50 px-1.5 py-0.5 text-[10px] font-semibold text-emerald-800"
        >
          <Check class="size-3" />
          {chrome.i18n.getMessage('savedConnection')}
        </span>
      {/if}
    </div>

    <label class="grid gap-1.5 text-[11px] font-bold text-stone-500" for="base-url">
      <span class="flex items-center gap-1.5"
        ><ExternalLink class="size-3" />{chrome.i18n.getMessage('gatewayUrl')}</span
      >
      <Input
        id="base-url"
        type="url"
        spellcheck={false}
        placeholder="http://127.0.0.1:8042"
        bind:value={draftSettings.baseUrl}
        oninput={markSettingsDirty}
      />
    </label>

    <label class="grid gap-1.5 text-[11px] font-bold text-stone-500" for="token">
      <span class="flex items-center gap-1.5"
        ><KeyRound class="size-3" />{chrome.i18n.getMessage('bearerToken')}</span
      >
      <Input
        id="token"
        type="text"
        spellcheck={false}
        placeholder={chrome.i18n.getMessage('tokenPlaceholder')}
        bind:value={draftSettings.token}
        oninput={markSettingsDirty}
      />
    </label>

    <div class="grid gap-1.5">
      <div class="flex items-center gap-1.5 text-[11px] font-bold text-stone-500">
        <Keyboard class="size-3" />
        <span>{chrome.i18n.getMessage('enterKeyBehavior')}</span>
      </div>
      <div
        class="grid grid-cols-2 gap-1 rounded-md border border-stone-200 bg-stone-50 p-1"
        role="radiogroup"
        aria-label={chrome.i18n.getMessage('enterKeyBehavior')}
      >
        <button
          type="button"
          role="radio"
          aria-checked={draftSettings.submitKeyMode === 'enter'}
          class={submitKeyModeButtonClass('enter')}
          onclick={() => updateSubmitKeyMode('enter')}
        >
          <span class="block truncate text-[11px] font-bold"
            >{chrome.i18n.getMessage('enterSendsMessage')}</span
          >
          <span class="block truncate text-[10px] font-semibold opacity-70"
            >{chrome.i18n.getMessage('shiftEnterNewLine')}</span
          >
        </button>
        <button
          type="button"
          role="radio"
          aria-checked={draftSettings.submitKeyMode === 'modifier-enter'}
          class={submitKeyModeButtonClass('modifier-enter')}
          onclick={() => updateSubmitKeyMode('modifier-enter')}
        >
          <span class="block truncate text-[11px] font-bold"
            >{chrome.i18n.getMessage('modifierEnterSendsMessage')}</span
          >
          <span class="block truncate text-[10px] font-semibold opacity-70"
            >{chrome.i18n.getMessage('enterNewLineModifierSends')}</span
          >
        </button>
      </div>
    </div>

    <div class="grid grid-cols-2 gap-2">
      <Button
        size="sm"
        class="w-full"
        disabled={savingSettings || !settingsDirty}
        onclick={saveSettings}
      >
        {#if savingSettings}
          <LoaderCircle class="size-3.5 animate-spin" />
        {:else}
          <Save class="size-3.5" />
        {/if}
        {chrome.i18n.getMessage('save')}
      </Button>
      <Button
        variant="outline"
        size="sm"
        class="w-full bg-white"
        disabled={testingConnection}
        onclick={testConnection}
      >
        {#if testingConnection}
          <LoaderCircle class="size-3.5 animate-spin" />
        {:else}
          <PlugZap class="size-3.5" />
        {/if}
        {chrome.i18n.getMessage('test')}
      </Button>
    </div>
  </div>
</section>
