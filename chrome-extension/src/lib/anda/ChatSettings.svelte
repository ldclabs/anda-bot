<script lang="ts">
  import { andaClient } from '$lib/anda/client/side-panel.svelte'
  import { type SettingsState, type SubmitKeyMode } from '$lib/anda/client/types'
  import {
    buttonClass,
    dialogContentClass,
    dialogDescriptionClass,
    dialogOverlayClass,
    fieldClass,
    fieldGroupClass,
    fieldLabelClass,
    inputClass,
    nativeSelectClass,
    nativeSelectWrapperClass,
    separatorClass
  } from '$lib/anda/ui'
  import { delay } from '$lib/utils/helper'
  import {
    BrainCircuit,
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
    RefreshCw,
    Save,
    Terminal,
    X
  } from '@lucide/svelte'
  import { Dialog } from 'bits-ui'
  import { onMount } from 'svelte'

  let {
    open = $bindable(false),
    setupGuideOpen = $bindable(false)
  }: { open?: boolean; setupGuideOpen?: boolean } = $props()

  let draftSettings = $state<SettingsState>({
    baseUrl: 'http://127.0.0.1:8042',
    token: '',
    submitKeyMode: 'enter'
  })
  let settingsDirty = $state(false)
  let savingSettings = $state(false)
  let testingConnection = $state(false)
  let loadingModels = $state(false)
  let switchingModel = $state(false)
  let copiedCommand = $state('')

  const modelNames = $derived(andaClient.modelState.modelNames)
  const activeModel = $derived(andaClient.modelState.activeModel || '')
  const canChangeModel = $derived(
    Boolean(andaClient.settings.token && modelNames.length > 0 && !loadingModels && !switchingModel)
  )

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
    const base = 'h-auto min-w-0 justify-start rounded-sm px-2 py-1.5 text-left transition'
    return draftSettings.submitKeyMode === submitKeyMode
      ? `${base} bg-background text-foreground shadow-xs ring-1 ring-border hover:bg-background`
      : `${base} text-muted-foreground hover:bg-background/80 hover:text-foreground`
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
      if (andaClient.settings.token) {
        await refreshModels()
      }
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
      await refreshModels()
    } catch (_error) {
    } finally {
      testingConnection = false
    }
  }

  async function refreshModels() {
    if (loadingModels || !andaClient.settings.token) {
      return
    }
    loadingModels = true
    try {
      await Promise.all([
        andaClient.refreshModelState(),
        delay(800) // Ensure the loading spinner is visible for at least 1.2 seconds to avoid flickering
      ])
    } catch (_error) {
    } finally {
      loadingModels = false
    }
  }

  async function switchActiveModel(event: Event) {
    const nextModel = (event.currentTarget as HTMLSelectElement | null)?.value || ''
    if (!nextModel || nextModel === activeModel || switchingModel) {
      return
    }
    switchingModel = true
    try {
      await Promise.all([
        andaClient.setActiveModel(nextModel),
        delay(800) // Ensure the loading spinner is visible for at least 1.2 seconds to avoid flickering
      ])
    } catch (_error) {
    } finally {
      switchingModel = false
    }
  }

  onMount(() => {
    draftSettings = { ...andaClient.settings }
    refreshModels().catch(() => undefined)
  })
</script>

{#snippet commandBlock(label: string, command: string)}
  <div class="grid min-w-0 gap-1.5">
    <div class="flex min-w-0 items-center justify-between gap-2">
      <span class="min-w-0 truncate text-[10px] font-semibold text-muted-foreground">{label}</span>
      <button
        type="button"
        class={buttonClass('ghost', 'icon-xs')}
        aria-label={chrome.i18n.getMessage('copyCommand')}
        title={chrome.i18n.getMessage('copyCommand')}
        onclick={() => copyCommand(command)}
      >
        {#if copiedCommand === command}
          <Check class="size-3 text-emerald-700" />
        {:else}
          <Clipboard class="size-3" />
        {/if}
      </button>
    </div>
    <code
      class="block min-w-0 overflow-x-auto rounded-md border bg-muted/35 px-2 py-1.5 font-mono text-[11px] leading-relaxed text-foreground shadow-xs"
      >{command}</code
    >
  </div>
{/snippet}

<Dialog.Root bind:open>
  <Dialog.Portal>
    <Dialog.Overlay class={dialogOverlayClass()} />
    <Dialog.Content
      class={dialogContentClass(
        'flex max-h-[min(90vh,46rem)] min-h-0 flex-col gap-0 overflow-hidden p-0 sm:max-w-2xl'
      )}
      aria-label={chrome.i18n.getMessage('settings')}
    >
      <Dialog.Close>
        {#snippet child({ props })}
          <button
            {...props}
            type="button"
            class={buttonClass('ghost', 'icon-sm', 'absolute top-4 right-4 z-10')}
          >
            <X class="size-4" />
            <span class="sr-only">Close</span>
          </button>
        {/snippet}
      </Dialog.Close>

      <div class="shrink-0 flex flex-col gap-2 border-b bg-muted/35 px-5 py-4 pr-12">
        <div class="flex min-w-0 items-start justify-between gap-3">
          <div class="grid min-w-0 gap-1">
            <Dialog.Title class="flex min-w-0 items-center gap-2 text-base font-bold">
              <Terminal class="size-4 shrink-0 text-emerald-800" />
              <span class="truncate">{chrome.i18n.getMessage('settings')}</span>
            </Dialog.Title>
            <Dialog.Description class={dialogDescriptionClass('text-xs leading-relaxed')}>
              {chrome.i18n.getMessage('onboardingIntro')}
            </Dialog.Description>
          </div>
        </div>
      </div>

      <div class="scrollbar-slim min-h-0 flex-1 flex flex-col gap-4 overflow-y-auto px-5 py-4">
        <div class="rounded-lg border bg-background shadow-xs">
          <button
            type="button"
            class="grid w-full grid-cols-[1fr_auto] items-start gap-3 px-3 py-3 text-left transition-colors hover:bg-muted/45 focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-ring"
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
              <span class="text-sm font-bold text-foreground">
                {chrome.i18n.getMessage('onboardingTitle')}
              </span>
              <span class="text-[11px] leading-relaxed text-muted-foreground">
                {chrome.i18n.getMessage('onboardingIntro')}
              </span>
            </span>
            <ChevronDown
              class={`mt-0.5 size-4 shrink-0 text-emerald-800 transition-transform ${setupGuideOpen ? 'rotate-180' : ''}`}
            />
          </button>

          {#if setupGuideOpen}
            <div id="local-setup-guide" class="grid gap-3 border-t px-3 py-3">
              <div class="grid grid-cols-[1.75rem_1fr] gap-3">
                <div
                  class="grid size-7 place-items-center rounded-md border border-sky-900/10 bg-sky-50 text-sky-800"
                >
                  <Download class="size-3.5" />
                </div>
                <div class="grid min-w-0 gap-2">
                  <div class="grid gap-0.5">
                    <h3 class="text-xs font-bold text-foreground">
                      {chrome.i18n.getMessage('onboardingInstallTitle')}
                    </h3>
                    <p class="text-[11px] leading-relaxed text-muted-foreground">
                      {chrome.i18n.getMessage('onboardingInstallBody')}
                    </p>
                  </div>
                  <div class="grid gap-2">
                    {@render commandBlock('Homebrew', installCommand)}
                    {@render commandBlock(chrome.i18n.getMessage('macLinux'), installScriptCommand)}
                    {@render commandBlock('Windows PowerShell', windowsInstallCommand)}
                  </div>
                </div>
              </div>

              <div class={separatorClass()} data-orientation="horizontal"></div>

              <div class="grid grid-cols-[1.75rem_1fr] gap-3">
                <div
                  class="grid size-7 place-items-center rounded-md border border-amber-900/10 bg-amber-50 text-amber-800"
                >
                  <FileCog class="size-3.5" />
                </div>
                <div class="grid min-w-0 gap-2">
                  <div class="grid gap-0.5">
                    <h3 class="text-xs font-bold text-foreground">
                      {chrome.i18n.getMessage('onboardingConfigureTitle')}
                    </h3>
                    <p class="text-[11px] leading-relaxed text-muted-foreground">
                      {chrome.i18n.getMessage('onboardingConfigureBody')}
                    </p>
                  </div>
                  {@render commandBlock(
                    chrome.i18n.getMessage('launchCommandLabel'),
                    launchCommand
                  )}
                </div>
              </div>

              <div class={separatorClass()} data-orientation="horizontal"></div>

              <div class="grid grid-cols-[1.75rem_1fr] gap-3">
                <div
                  class="grid size-7 place-items-center rounded-md border border-emerald-900/10 bg-emerald-50 text-emerald-800"
                >
                  <KeyRound class="size-3.5" />
                </div>
                <div class="grid min-w-0 gap-2">
                  <div class="grid gap-0.5">
                    <h3 class="text-xs font-bold text-foreground">
                      {chrome.i18n.getMessage('onboardingTokenTitle')}
                    </h3>
                    <p class="text-[11px] leading-relaxed text-muted-foreground">
                      {chrome.i18n.getMessage('onboardingTokenBody')}
                    </p>
                  </div>
                  {@render commandBlock(chrome.i18n.getMessage('tokenCommandLabel'), tokenCommand)}
                </div>
              </div>
            </div>
          {/if}
        </div>

        <div data-slot="field-group" class={fieldGroupClass('gap-4')}>
          <div class="flex items-center justify-between gap-2">
            <div class="flex min-w-0 items-center gap-1.5 text-xs font-bold text-foreground">
              <Play class="size-3.5 shrink-0 text-emerald-800" />
              <span class="truncate">{chrome.i18n.getMessage('connectionDetails')}</span>
            </div>
          </div>

          <div data-slot="field" class={fieldClass('gap-1.5')}>
            <label
              class={fieldLabelClass('text-[11px] font-bold text-muted-foreground')}
              for="base-url"
            >
              <ExternalLink class="size-3" />
              {chrome.i18n.getMessage('gatewayUrl')}
            </label>
            <input
              id="base-url"
              type="url"
              class={inputClass()}
              spellcheck={false}
              placeholder="http://127.0.0.1:8042"
              bind:value={draftSettings.baseUrl}
              oninput={markSettingsDirty}
            />
          </div>

          <div data-slot="field" class={fieldClass('gap-1.5')}>
            <label
              class={fieldLabelClass('text-[11px] font-bold text-muted-foreground')}
              for="token"
            >
              <KeyRound class="size-3" />
              {chrome.i18n.getMessage('bearerToken')}
            </label>
            <input
              id="token"
              type="text"
              class={inputClass()}
              spellcheck={false}
              placeholder={chrome.i18n.getMessage('tokenPlaceholder')}
              bind:value={draftSettings.token}
              oninput={markSettingsDirty}
            />
          </div>

          <div data-slot="field" class={fieldClass('gap-1.5')}>
            <div class="flex items-center justify-between gap-2">
              <label
                class={fieldLabelClass('min-w-0 text-[11px] font-bold text-muted-foreground')}
                for="active-model"
              >
                <BrainCircuit class="size-3" />
                <span class="truncate">{chrome.i18n.getMessage('activeModel')}</span>
              </label>
            </div>
            <div class="grid grid-cols-[1fr_auto] items-center gap-2">
              <div
                class={nativeSelectWrapperClass('w-full')}
                data-slot="native-select-wrapper"
                data-size="sm"
              >
                <select
                  id="active-model"
                  data-slot="native-select"
                  data-size="sm"
                  class={nativeSelectClass()}
                  value={activeModel}
                  disabled={!canChangeModel}
                  aria-label={chrome.i18n.getMessage('activeModel')}
                  onchange={switchActiveModel}
                >
                  {#if modelNames.length === 0}
                    <option class="bg-[Canvas] text-[CanvasText]" value="">
                      {chrome.i18n.getMessage('modelListEmpty')}
                    </option>
                  {/if}
                  {#each modelNames as modelName}
                    <option class="bg-[Canvas] text-[CanvasText]" value={modelName}>
                      {modelName}
                    </option>
                  {/each}
                </select>
                <ChevronDown
                  class="pointer-events-none absolute top-1/2 right-2.5 size-4 -translate-y-1/2 text-muted-foreground select-none"
                  aria-hidden="true"
                />
              </div>
              <button
                type="button"
                class={buttonClass('ghost')}
                disabled={!andaClient.settings.token || loadingModels || switchingModel}
                aria-label={chrome.i18n.getMessage('refreshModels')}
                title={chrome.i18n.getMessage('refreshModels')}
                onclick={refreshModels}
              >
                <RefreshCw
                  class={`size-4 ${loadingModels || switchingModel ? 'animate-spin text-emerald-700' : ''}`}
                />
              </button>
            </div>
          </div>

          <div data-slot="field" class={fieldClass('gap-1.5')}>
            <label class={fieldLabelClass('text-[11px] font-bold text-muted-foreground')}>
              <Keyboard class="size-3" />
              {chrome.i18n.getMessage('enterKeyBehavior')}
            </label>
            <div
              class="grid grid-cols-2 gap-1 rounded-md border bg-muted/45 p-1"
              role="radiogroup"
              aria-label={chrome.i18n.getMessage('enterKeyBehavior')}
            >
              <button
                type="button"
                role="radio"
                aria-checked={draftSettings.submitKeyMode === 'enter'}
                class={buttonClass('ghost', 'default', submitKeyModeButtonClass('enter'))}
                onclick={() => updateSubmitKeyMode('enter')}
              >
                <span class="grid min-w-0 gap-0.5">
                  <span class="block truncate text-[11px] font-bold">
                    {chrome.i18n.getMessage('enterSendsMessage')}
                  </span>
                  <span class="block truncate text-[10px] font-semibold opacity-70">
                    {chrome.i18n.getMessage('shiftEnterNewLine')}
                  </span>
                </span>
              </button>
              <button
                type="button"
                role="radio"
                aria-checked={draftSettings.submitKeyMode === 'modifier-enter'}
                class={buttonClass('ghost', 'default', submitKeyModeButtonClass('modifier-enter'))}
                onclick={() => updateSubmitKeyMode('modifier-enter')}
              >
                <span class="grid min-w-0 gap-0.5">
                  <span class="block truncate text-[11px] font-bold">
                    {chrome.i18n.getMessage('modifierEnterSendsMessage')}
                  </span>
                  <span class="block truncate text-[10px] font-semibold opacity-70">
                    {chrome.i18n.getMessage('enterNewLineModifierSends')}
                  </span>
                </span>
              </button>
            </div>
          </div>
        </div>
      </div>

      <div
        class="shrink-0 grid grid-cols-2 gap-2 border-t bg-muted/25 px-5 py-4 sm:grid-cols-2 sm:justify-stretch"
      >
        <button
          type="button"
          class={buttonClass('default', 'sm', 'w-full')}
          disabled={savingSettings || !settingsDirty}
          onclick={saveSettings}
        >
          {#if savingSettings}
            <LoaderCircle class="size-3.5 animate-spin" />
          {:else}
            <Save class="size-3.5" />
          {/if}
          {chrome.i18n.getMessage('save')}
        </button>
        <button
          type="button"
          class={buttonClass('outline', 'sm', 'w-full bg-background')}
          disabled={testingConnection}
          onclick={testConnection}
        >
          {#if testingConnection}
            <LoaderCircle class="size-3.5 animate-spin" />
          {:else}
            <PlugZap class="size-3.5" />
          {/if}
          {chrome.i18n.getMessage('test')}
        </button>
      </div>
    </Dialog.Content>
  </Dialog.Portal>
</Dialog.Root>
