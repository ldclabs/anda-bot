<script lang="ts">
  import ChatGptSettings from './chatgpt/ChatGptSettings.svelte'
  import { getMessage } from '$lib/i18n'
  import { andaClient } from '$lib/anda/client/side-panel.svelte'
  import {
    type AppearanceTheme,
    type SettingsState,
    type SubmitKeyMode
  } from '$lib/anda/client/types'
  import {
    buttonClass,
    fieldClass,
    fieldGroupClass,
    fieldLabelClass,
    inputClass,
    separatorClass
  } from '$lib/anda/ui'
  import DropdownMenu from '$lib/anda/DropdownMenu.svelte'
  import Modal from '$lib/anda/Modal.svelte'
  import { connectionKey, defaultSettings, errorToMessage } from '$lib/service-worker/settings'
  import {
    BrainCircuit,
    Check,
    ChevronDown,
    Clipboard,
    Download,
    ExternalLink,
    Eye,
    EyeOff,
    FileCog,
    KeyRound,
    Keyboard,
    LoaderCircle,
    Monitor,
    Moon,
    Play,
    PlugZap,
    RefreshCw,
    Save,
    Sun,
    Terminal
  } from '@lucide/svelte'
  import { onMount } from 'svelte'

  let {
    open = $bindable(false),
    setupGuideOpen = $bindable(false),
    dashboard = false
  }: { open?: boolean; setupGuideOpen?: boolean; dashboard?: boolean } = $props()

  let draftSettings = $state<SettingsState>({ ...defaultSettings })
  let tokenVisible = $state(false)
  let feedback = $state<{ error: boolean; text: string } | null>(null)
  const connectionChanged = $derived(
    connectionKey(draftSettings) !== connectionKey(andaClient.settings)
  )
  const settingsDirty = $derived(
    connectionChanged || draftSettings.submitKeyMode !== andaClient.settings.submitKeyMode
  )
  let savingSettings = $state(false)
  let testingConnection = $state(false)
  let loadingModels = $state(false)
  let switchingModel = $state(false)
  let savingAppearanceTheme = $state(false)
  let copiedCommand = $state('')

  const modelNames = $derived(andaClient.modelState.modelNames)
  const activeModel = $derived(andaClient.modelState.activeModel || '')
  const canChangeModel = $derived(
    Boolean(andaClient.settings.token && modelNames.length > 0 && !loadingModels && !switchingModel)
  )

  const themeChoices = [
    { value: 'light', label: getMessage('appearanceLight'), icon: Sun },
    { value: 'dark', label: getMessage('appearanceDark'), icon: Moon },
    { value: 'system', label: getMessage('appearanceSystem'), icon: Monitor }
  ] as const

  const submitChoices = [
    {
      value: 'enter',
      label: getMessage('enterSendsMessage'),
      detail: getMessage('shiftEnterNewLine')
    },
    {
      value: 'modifier-enter',
      label: getMessage('modifierEnterSendsMessage'),
      detail: getMessage('enterNewLineModifierSends')
    }
  ] as const

  const installScriptCommand =
    'curl -fsSL https://raw.githubusercontent.com/ldclabs/anda-bot/main/scripts/install.sh | sh'
  const releaseDownload = 'https://github.com/ldclabs/anda-bot/releases/latest/download'
  const windowsInstallerUrl = `${releaseDownload}/Anda-win-x64.exe`
  const macInstallerUrl = `${releaseDownload}/Anda-mac-arm64.dmg`
  const tokenCommand = 'anda browser token --days 365'
  const configPageUrl = chrome.runtime.getURL('dashboard.html#config')

  function validateGateway(): boolean {
    try {
      const url = new URL(draftSettings.baseUrl.trim() || defaultSettings.baseUrl)
      if (
        ['http:', 'https:'].includes(url.protocol) &&
        !url.username &&
        !url.password &&
        !url.hash &&
        !url.search
      )
        return true
    } catch {
      // Report the same actionable message for a missing scheme or invalid address.
    }
    feedback = { error: true, text: getMessage('invalidGatewayUrl') }
    document.getElementById('base-url')?.focus()
    return false
  }

  function updateSubmitKeyMode(submitKeyMode: SubmitKeyMode) {
    if (draftSettings.submitKeyMode === submitKeyMode) {
      return
    }
    draftSettings = { ...draftSettings, submitKeyMode }
    feedback = null
  }

  async function updateAppearanceTheme(appearanceTheme: AppearanceTheme) {
    if (draftSettings.appearanceTheme === appearanceTheme) {
      return
    }
    feedback = null
    savingAppearanceTheme = true
    try {
      await andaClient.saveAppearanceTheme(appearanceTheme)
      draftSettings = { ...draftSettings, appearanceTheme: andaClient.settings.appearanceTheme }
    } catch (error) {
      feedback = { error: true, text: errorToMessage(error) }
    } finally {
      savingAppearanceTheme = false
    }
  }

  function submitKeyModeButtonClass(submitKeyMode: SubmitKeyMode): string {
    const base = 'h-auto min-w-0 justify-start rounded-sm px-2 py-1.5 text-start transition'
    return draftSettings.submitKeyMode === submitKeyMode
      ? `${base} bg-background text-foreground shadow-xs ring-1 ring-border hover:bg-background`
      : `${base} text-muted-foreground hover:bg-background/80 hover:text-foreground`
  }

  function appearanceThemeButtonClass(appearanceTheme: AppearanceTheme): string {
    const base = 'h-8 min-w-0 justify-center rounded-sm px-1.5 text-xs transition'
    return draftSettings.appearanceTheme === appearanceTheme
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
      feedback = { error: true, text: errorToMessage(error) }
    }
  }

  async function saveSettings() {
    if (savingSettings || testingConnection || !validateGateway()) {
      return
    }
    savingSettings = true
    try {
      await andaClient.saveSettings({
        ...andaClient.settings,
        baseUrl: draftSettings.baseUrl,
        token: draftSettings.token,
        submitKeyMode: draftSettings.submitKeyMode
      })
      feedback = { error: false, text: getMessage('settingsSaved') }
      if (draftSettings.token.trim()) {
        setupGuideOpen = false
      }
      // Saving already re-read the model list from the daemon.
      draftSettings = { ...andaClient.settings }
    } catch (error) {
      feedback = { error: true, text: errorToMessage(error) }
    } finally {
      savingSettings = false
    }
  }

  async function testConnection() {
    if (testingConnection || savingSettings || !validateGateway()) {
      return
    }
    feedback = null
    testingConnection = true
    try {
      await andaClient.testConnection(draftSettings)
      feedback = { error: false, text: getMessage('connectionTestPassed') }
    } catch (error) {
      feedback = { error: true, text: errorToMessage(error) }
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
      await andaClient.refreshModelState({ reload: true })
      feedback = { error: false, text: getMessage('modelsRefreshed') }
    } catch (error) {
      feedback = { error: true, text: errorToMessage(error) }
    } finally {
      loadingModels = false
    }
  }

  async function switchActiveModel(nextModel: string) {
    if (!nextModel || nextModel === activeModel || switchingModel) {
      return
    }
    switchingModel = true
    try {
      await andaClient.setActiveModel(nextModel)
      feedback = { error: false, text: getMessage('modelUpdated') }
    } catch (error) {
      feedback = { error: true, text: errorToMessage(error) }
    } finally {
      switchingModel = false
    }
  }

  onMount(() => {
    draftSettings = { ...andaClient.settings }
    // The client loads the model list on connect; `reload_models` (which
    // rebuilds every model from config.yaml) stays behind the refresh button.
    if (andaClient.settings.token && !andaClient.modelState.modelNames.length) {
      andaClient.refreshModelState().catch(() => undefined)
    }
  })
</script>

{#snippet commandBlock(label: string, command: string)}
  <div class="grid min-w-0 gap-1.5">
    <div class="flex min-w-0 items-center justify-between gap-2">
      <span class="min-w-0 truncate text-[10px] font-semibold text-muted-foreground">{label}</span>
      <button
        type="button"
        class={buttonClass('ghost', 'icon-xs')}
        aria-label={getMessage('copyCommand')}
        title={getMessage('copyCommand')}
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
      class="block min-w-0 overflow-x-auto rounded-md border bg-muted/35 px-2 py-1.5 font-mono text-xs leading-relaxed text-foreground shadow-xs"
      >{command}</code
    >
  </div>
{/snippet}

{#snippet downloadBlock(label: string, url: string)}
  <div class="grid min-w-0 gap-1.5">
    <span class="min-w-0 truncate text-[10px] font-semibold text-muted-foreground">{label}</span>
    <a
      class={buttonClass('outline', 'sm', 'w-full justify-between bg-background text-xs')}
      href={url}
      target="_blank"
      rel="noreferrer"
    >
      <span class="min-w-0 truncate">{getMessage('openDownload')}</span>
      <ExternalLink class="size-3.5" />
    </a>
  </div>
{/snippet}

{#snippet actions()}
  {#if feedback}
    <p
      class={`w-full text-sm break-words ${feedback.error ? 'text-destructive' : 'text-foreground'}`}
      role={feedback.error ? 'alert' : 'status'}
    >
      {feedback.text}
    </p>
  {/if}
  <button
    type="button"
    class={buttonClass('default', 'sm', 'flex-1')}
    disabled={savingAppearanceTheme || savingSettings || testingConnection || !settingsDirty}
    onclick={saveSettings}
  >
    {#if savingSettings}
      <LoaderCircle class="size-3.5 animate-spin" />
    {:else}
      <Save class="size-3.5" />
    {/if}
    {getMessage('save')}
  </button>
  <button
    type="button"
    class={buttonClass('outline', 'sm', 'flex-1 bg-background')}
    disabled={testingConnection || savingSettings || !draftSettings.token.trim()}
    onclick={testConnection}
  >
    {#if testingConnection}
      <LoaderCircle class="size-3.5 animate-spin" />
    {:else}
      <PlugZap class="size-3.5" />
    {/if}
    {getMessage('test')}
  </button>
{/snippet}

<Modal
  bind:open
  title={getMessage(dashboard ? 'dashboardConnectionSettings' : 'settings')}
  footer={actions}
>
  <div class="flex flex-col gap-4">
    <div data-slot="field-group" class={fieldGroupClass('gap-4')}>
      <div class="flex items-center justify-between gap-2">
        <div class="flex min-w-0 items-center gap-1.5 text-xs font-bold text-foreground">
          <Play class="size-3.5 shrink-0 text-emerald-800" />
          <span class="truncate">{getMessage('connectionDetails')}</span>
        </div>
      </div>

      <div data-slot="field" class={fieldClass('gap-1.5')}>
        <label class={fieldLabelClass('text-xs font-bold text-muted-foreground')} for="base-url">
          <ExternalLink class="size-3" />
          {getMessage('gatewayUrl')}
        </label>
        <input
          id="base-url"
          type="url"
          class={inputClass()}
          spellcheck={false}
          placeholder="http://127.0.0.1:8042"
          bind:value={draftSettings.baseUrl}
          disabled={savingSettings || testingConnection}
          oninput={() => (feedback = null)}
        />
      </div>

      <div data-slot="field" class={fieldClass('gap-1.5')}>
        <label class={fieldLabelClass('text-xs font-bold text-muted-foreground')} for="token">
          <KeyRound class="size-3" />
          {getMessage('bearerToken')}
        </label>
        <div class="relative">
          <input
            id="token"
            type={tokenVisible ? 'text' : 'password'}
            autocomplete="off"
            class={inputClass('pe-10')}
            spellcheck={false}
            placeholder={getMessage('tokenPlaceholder')}
            bind:value={draftSettings.token}
            disabled={savingSettings || testingConnection}
            oninput={() => (feedback = null)}
          />
          <button
            type="button"
            class={buttonClass('ghost', 'icon-sm', 'absolute end-1 top-1/2 -translate-y-1/2')}
            aria-label={getMessage(tokenVisible ? 'hideToken' : 'showToken')}
            title={getMessage(tokenVisible ? 'hideToken' : 'showToken')}
            onclick={() => (tokenVisible = !tokenVisible)}
          >
            {#if tokenVisible}<EyeOff class="size-4" />{:else}<Eye class="size-4" />{/if}
          </button>
        </div>
      </div>

      <div class="rounded-lg border bg-background shadow-xs">
        <button
          type="button"
          class="grid w-full grid-cols-[1fr_auto] items-start gap-3 px-3 py-3 text-left transition-colors hover:bg-muted/45 focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-ring"
          aria-expanded={setupGuideOpen}
          aria-controls="local-setup-guide"
          aria-label={setupGuideOpen
            ? getMessage('collapseLocalSetup')
            : getMessage('expandLocalSetup')}
          onclick={() => (setupGuideOpen = !setupGuideOpen)}
        >
          <span class="grid min-w-0 gap-1">
            <span class="flex items-center gap-1.5 text-xs font-bold text-emerald-800">
              <Terminal class="size-3.5" />
              <span>{getMessage('onboardingEyebrow')}</span>
            </span>
            <span class="text-sm font-bold text-foreground">
              {getMessage('onboardingTitle')}
            </span>
            <span class="text-xs leading-relaxed text-muted-foreground">
              {getMessage('onboardingIntro')}
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
                    {getMessage('onboardingInstallTitle')}
                  </h3>
                  <p class="text-xs leading-relaxed text-muted-foreground">
                    {getMessage('onboardingInstallBody')}
                  </p>
                </div>
                <div class="grid gap-2">
                  {@render downloadBlock(getMessage('windowsInstaller'), windowsInstallerUrl)}
                  {@render downloadBlock(getMessage('macosInstaller'), macInstallerUrl)}
                  {@render commandBlock(getMessage('cliInstaller'), installScriptCommand)}
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
                    {getMessage('onboardingConfigureTitle')}
                  </h3>
                  <p class="text-xs leading-relaxed text-muted-foreground">
                    {getMessage('onboardingConfigureBody')}
                  </p>
                </div>
                <a
                  class={buttonClass(
                    'outline',
                    'sm',
                    'w-full justify-between bg-background text-xs'
                  )}
                  href={configPageUrl}
                  target="_blank"
                  rel="noreferrer"
                >
                  <span class="min-w-0 truncate">{getMessage('openConfigEditor')}</span>
                  <ExternalLink class="size-3.5" />
                </a>
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
                    {getMessage('onboardingTokenTitle')}
                  </h3>
                  <p class="text-xs leading-relaxed text-muted-foreground">
                    {getMessage('onboardingTokenBody')}
                  </p>
                </div>
                {@render commandBlock(getMessage('tokenCommandLabel'), tokenCommand)}
              </div>
            </div>
          </div>
        {/if}
      </div>

      {#if andaClient.settings.token && !connectionChanged}
        {#key connectionKey(andaClient.settings)}<ChatGptSettings
            settings={andaClient.settings}
            onModelSelected={refreshModels}
          />{/key}

        <div data-slot="field" class={fieldClass('gap-1.5')}>
          <div class="flex items-center justify-between gap-2">
            <label
              class={fieldLabelClass('min-w-0 text-xs font-bold text-muted-foreground')}
              for="active-model"
            >
              <BrainCircuit class="size-3" />
              <span class="truncate">{getMessage('activeModel')}</span>
            </label>
          </div>
          <div class="grid grid-cols-[1fr_auto] items-center gap-2">
            <DropdownMenu
              id="active-model"
              class="h-8"
              items={modelNames.length > 0
                ? modelNames.map((modelName) => ({ value: modelName, label: modelName }))
                : [{ value: '', label: getMessage('modelListEmpty') }]}
              value={activeModel}
              disabled={!canChangeModel}
              onSelect={(model) => void switchActiveModel(model)}
              ariaLabel={getMessage('activeModel')}
            />
            <button
              type="button"
              class={buttonClass('ghost')}
              disabled={!andaClient.settings.token || loadingModels || switchingModel}
              aria-label={getMessage('refreshModels')}
              title={getMessage('refreshModels')}
              onclick={refreshModels}
            >
              <RefreshCw
                class={`size-4 ${loadingModels || switchingModel ? 'animate-spin text-emerald-700' : ''}`}
              />
            </button>
          </div>
        </div>
      {/if}
      <div data-slot="field" class={fieldClass('gap-1.5')}>
        <label class={fieldLabelClass('text-xs font-bold text-muted-foreground')}>
          <Monitor class="size-3" />
          {getMessage('appearanceTheme')}
        </label>
        <div
          class="grid grid-cols-3 gap-1 rounded-md border bg-muted/45 p-1"
          role="radiogroup"
          aria-label={getMessage('appearanceTheme')}
        >
          {#each themeChoices as item}
            <label
              class={buttonClass(
                'ghost',
                'default',
                appearanceThemeButtonClass(item.value) +
                  ' relative has-[:focus-visible]:ring-2 has-[:focus-visible]:ring-ring'
              )}
            >
              <input
                class="sr-only"
                type="radio"
                name="appearance-theme"
                value={item.value}
                checked={draftSettings.appearanceTheme === item.value}
                disabled={savingAppearanceTheme || savingSettings || testingConnection}
                onchange={() => updateAppearanceTheme(item.value)}
              />
              <item.icon class="size-3.5 shrink-0" /><span>{item.label}</span>
            </label>
          {/each}
        </div>
      </div>

      <div data-slot="field" class={fieldClass('gap-1.5')}>
        <label class={fieldLabelClass('text-xs font-bold text-muted-foreground')}>
          <Keyboard class="size-3" />
          {getMessage('enterKeyBehavior')}
        </label>
        <div
          class="grid grid-cols-2 gap-1 rounded-md border bg-muted/45 p-1"
          role="radiogroup"
          aria-label={getMessage('enterKeyBehavior')}
        >
          {#each submitChoices as item}
            <label
              class={buttonClass(
                'ghost',
                'default',
                submitKeyModeButtonClass(item.value) +
                  ' relative whitespace-normal has-[:focus-visible]:ring-2 has-[:focus-visible]:ring-ring'
              )}
            >
              <input
                type="radio"
                class="sr-only"
                name="submit-key"
                value={item.value}
                checked={draftSettings.submitKeyMode === item.value}
                disabled={savingSettings || testingConnection}
                onchange={() => updateSubmitKeyMode(item.value)}
              />
              <span class="grid min-w-0 gap-0.5"
                ><span class="text-xs font-bold">{item.label}</span><span
                  class="text-[11px] text-muted-foreground">{item.detail}</span
                ></span
              >
            </label>
          {/each}
        </div>
      </div>
    </div>
    {#if !dashboard && andaClient.settings.token}
      <div class="rounded-lg border bg-background p-3 shadow-xs">
        <div class="grid grid-cols-[1.75rem_1fr] gap-3">
          <div
            class="grid size-7 place-items-center rounded-md border border-emerald-900/10 bg-emerald-50 text-emerald-800"
          >
            <FileCog class="size-3.5" />
          </div>
          <div class="grid min-w-0 gap-2">
            <div class="grid gap-0.5">
              <h3 class="text-xs font-bold text-foreground">
                {getMessage('configEditorTitle')}
              </h3>
              <p class="text-xs leading-relaxed text-muted-foreground">
                {getMessage('configEditorBody')}
              </p>
            </div>
            <a
              class={buttonClass('outline', 'sm', 'w-full justify-between bg-background text-xs')}
              href={configPageUrl}
              target="_blank"
              rel="noreferrer"
            >
              <span class="min-w-0 truncate">{getMessage('openConfigEditor')}</span>
              <ExternalLink class="size-3.5" />
            </a>
          </div>
        </div>
      </div>
    {/if}
  </div>
</Modal>
