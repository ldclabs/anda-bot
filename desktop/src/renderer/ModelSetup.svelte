<script lang="ts">
  import { onDestroy, tick } from 'svelte'
  import { ArrowLeft, ArrowRight, Check, KeyRound, LoaderCircle, X } from '@lucide/svelte'
  import ChatGptSettings from '$lib/anda/chatgpt/ChatGptSettings.svelte'
  import type { DaemonConfigResponse } from '$lib/anda/config/api'
  import template from '../../../anda_bot/assets/config.yaml?raw'
  import pandaLogo from '../../../anda_bot/assets/logo.png'
  import { configurePreset, modelPresets } from '../shared/model-setup'
  import type { DesktopClient } from './client.svelte'
  import { label, type Label } from './labels'
  import { focusDialog } from './dialog'

  let { client, onadvanced }: { client: DesktopClient; onadvanced: () => void } = $props()
  const t = (key: Label) => label(client.preferences.language, key)
  const presets = modelPresets(template)
  let step = $state<'choose' | 'chatgpt' | 'preset' | 'activating' | 'done'>('choose')
  let selected = $state(presets[0]!.model)
  let apiKey = $state('')
  let busy = $state(false)
  let error = $state('')
  let expectedModel = $state('')
  let alive = true
  let title: HTMLHeadingElement | undefined = $state()
  const preset = $derived(presets.find((item) => item.model === selected)!)
  const heading = $derived(
    step === 'choose'
      ? 'setupWelcome'
      : step === 'chatgpt'
        ? 'setupChatGpt'
        : step === 'preset'
          ? 'setupPreset'
          : step === 'done'
            ? 'setupLoaded'
            : 'setupActivating'
  )
  onDestroy(() => {
    alive = false
    apiKey = ''
  })
  $effect(() => {
    const current = step
    void tick().then(() => {
      if (alive && current === step) title?.focus()
    })
  })

  function close() {
    void client.dismissModelSetup().catch((cause) => client.fail(cause))
  }
  function advanced() {
    close()
    onadvanced()
  }
  function back() {
    step = 'choose'
    apiKey = ''
    error = ''
  }

  async function activate(model: string) {
    expectedModel = model
    step = 'activating'
    busy = true
    error = ''
    try {
      const deadline = Date.now() + 45_000
      let reload = true
      while (alive && Date.now() < deadline) {
        client.connection = await window.anda.connect()
        if (!alive) return
        if (client.authorized) {
          await client.refreshModelState(reload)
          reload = false
          if (!alive) return
          if (
            client.modelState.activeModel === model &&
            client.modelState.modelNames.includes(model)
          ) {
            step = 'done'
            return
          }
        }
        await new Promise((resolve) => setTimeout(resolve, 1000))
      }
      if (alive) error = t('setupUnavailable')
    } catch {
      if (alive) error = t('setupUnavailable')
    } finally {
      busy = false
    }
  }
  async function savePreset() {
    if (busy) return
    busy = true
    error = ''
    try {
      // Load at submission time and use its revision. Never overwrite edits
      // made by the CLI or another client while this dialog was open.
      const current = await window.anda.config<DaemonConfigResponse>('GET')
      if (!alive) return
      const content = configurePreset(current.content, preset, apiKey)
      const result = await window.anda.config<DaemonConfigResponse>(
        'PUT',
        content,
        current.revision
      )
      apiKey = ''
      if (!alive) return
      if (result.models_error) throw new Error(result.models_error)
      await activate(preset.model)
    } catch (cause) {
      if (!alive) return
      const message = cause instanceof Error ? cause.message : String(cause)
      error = ['setupInvalidConfig', 'setupKeyRequired', 'setupPresetConflict'].includes(message)
        ? t(message as Label)
        : message
    } finally {
      busy = false
    }
  }
  async function finish() {
    client.view = 'chat'
    await client.dismissModelSetup().catch((cause) => client.fail(cause))
    await tick()
    document.querySelector<HTMLTextAreaElement>('.composer-textarea')?.focus()
  }
</script>

<div class="modal-backdrop setup-backdrop">
  <div
    class="model-setup"
    role="dialog"
    aria-modal="true"
    aria-labelledby="model-setup-title"
    tabindex="-1"
    use:focusDialog={close}
    dir={client.preferences.language === 'ar' ? 'rtl' : 'ltr'}
  >
    <header>
      <img src={pandaLogo} alt="Anda" width="42" height="42" />
      <button class="icon-button" onclick={close} aria-label={t('close')}><X size={18} /></button>
    </header>
    <h1 id="model-setup-title" bind:this={title} tabindex="-1">{t(heading)}</h1>
    {#if step === 'choose'}
      <p class="hint intro">{t('setupIntro')}</p>
      <div class="choices">
        <button class="choice" onclick={() => (step = 'chatgpt')}>
          <span><strong>{t('setupChatGpt')}</strong><small>{t('setupChatGptHint')}</small></span>
          <ArrowRight size={19} />
        </button>
        <button class="choice" onclick={() => (step = 'preset')}>
          <span><strong>{t('setupProviders')}</strong><small>{t('setupProvidersHint')}</small></span
          >
          <KeyRound size={19} />
        </button>
      </div>
      <label class="login-choice"
        ><input
          type="checkbox"
          checked={client.preferences.launchAtLogin}
          onchange={(event) =>
            void client
              .savePreferences({ launchAtLogin: event.currentTarget.checked })
              .catch((cause) => client.fail(cause))}
        />
        {t('startAtLogin')}
      </label>
    {:else if step === 'chatgpt'}
      <p class="hint intro">{t('setupChatGptHint')}</p>
      <ChatGptSettings
        settings={client.settings}
        onboarding
        noModelsMessage={t('setupNoModels')}
        onModelSelected={activate}
      />
    {:else if step === 'preset'}
      <form
        onsubmit={(event) => {
          event.preventDefault()
          void savePreset()
        }}
      >
        <fieldset disabled={busy}>
          <legend class="hint">{t('setupProvidersHint')}</legend>
          <div class="presets">
            {#each presets as item}
              <label class="preset" class:selected={selected === item.model}>
                <input
                  type="radio"
                  name="preset"
                  value={item.model}
                  bind:group={selected}
                  onchange={() => {
                    apiKey = ''
                    error = ''
                  }}
                />
                <span
                  ><strong>{item.model}</strong><small>{new URL(item.api_base).hostname}</small
                  ></span
                >
              </label>
            {/each}
          </div>
          <div class="key-heading">
            <label class="key-label" for="setup-api-key">{t('setupApiKey')}</label>
            <span>{preset.model}</span>
          </div>
          <input
            id="setup-api-key"
            class="api-key"
            type="password"
            bind:value={apiKey}
            required
            autocomplete="off"
            spellcheck="false"
            aria-describedby="setup-key-hint"
          />
          <p id="setup-key-hint" class="hint key-hint">{t('setupKeyHint')}</p>
          <p class="endpoint" dir="ltr">{preset.api_base}</p>
        </fieldset>
        <button class="primary connect-button" type="submit" disabled={busy || !apiKey.trim()}>
          {#if busy}<LoaderCircle size={16} class="spinner" />{/if}{t('setupConnect')}
        </button>
      </form>
    {:else if step === 'activating'}
      <div class="result" role="status" aria-live="polite">
        {#if busy}<LoaderCircle size={30} class="spinner" />{/if}
        <p class="hint">{t('setupWaitingHint')}</p>
      </div>
    {:else}
      <div class="result" role="status">
        <Check size={32} />
        <strong class="model-name">{client.modelState.activeModel}</strong>
        <p class="hint">{t('setupLoadedHint')}</p>
      </div>
      <button class="primary connect-button" onclick={() => void finish()}>{t('setupStart')}</button
      >
    {/if}
    {#if error}
      <p class="setup-error" role="alert">{error}</p>
      {#if step === 'activating'}<button
          class="primary"
          onclick={() => void activate(expectedModel)}>{t('setupRetry')}</button
        >{/if}
    {/if}
    {#if step !== 'done'}
      <footer>
        {#if step === 'chatgpt' || step === 'preset'}
          <button class="text-button" disabled={busy} onclick={back}
            ><ArrowLeft size={14} />{t('setupBack')}</button
          >
        {:else}<button class="text-button" onclick={close}>{t('setupLater')}</button>{/if}
        <button class="text-button" disabled={busy} onclick={advanced}>{t('setupAdvanced')}</button>
      </footer>
    {/if}
  </div>
</div>

<style>
  .setup-backdrop {
    align-items: center;
    padding: 20px;
  }
  .model-setup {
    width: min(560px, 100%);
    max-height: calc(100dvh - 40px);
    overflow-y: auto;
    padding: 28px;
    border: 1px solid var(--desk-border);
    border-radius: 18px;
    background: var(--desk-bg);
    color: var(--desk-text);
    box-shadow: 0 24px 90px #0003;
  }
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 18px;
  }
  header img {
    object-fit: contain;
  }
  h1 {
    font-size: 25px;
    font-weight: 600;
    letter-spacing: -0.6px;
    line-height: 1.3;
    margin-bottom: 12px;
  }
  h1:focus {
    outline: none;
  }
  .hint {
    color: var(--desk-muted);
    font-size: 13px;
    line-height: 1.65;
  }
  .intro {
    margin-bottom: 24px;
  }
  .choices {
    display: grid;
    gap: 12px;
  }
  .choice {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 18px;
    padding: 20px;
    text-align: start;
    border: 1px solid var(--desk-border);
    border-radius: 12px;
  }
  .choice:first-child {
    background: var(--desk-text);
    color: var(--desk-bg);
    border-color: var(--desk-text);
  }
  .choice:hover {
    opacity: 0.85;
  }
  .choice span,
  .preset span {
    display: grid;
    gap: 6px;
    min-width: 0;
  }
  .choice strong {
    font-size: 15px;
    font-weight: 550;
  }
  small {
    font-size: 12px;
    line-height: 1.5;
  }
  .choice small {
    opacity: 0.75;
  }
  .login-choice {
    display: flex;
    align-items: center;
    gap: 9px;
    margin-top: 24px;
    font-size: 12px;
    color: var(--desk-muted);
  }
  input {
    accent-color: var(--desk-text);
  }
  fieldset {
    border: 0;
    padding: 0;
    margin: 0;
    min-width: 0;
  }
  .presets {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 8px;
    max-height: min(246px, 30dvh);
    overflow: auto;
    margin: 14px 0 20px;
    padding: 3px;
  }
  .preset {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 12px;
    border: 1px solid var(--desk-border);
    border-radius: 9px;
    cursor: pointer;
    min-width: 0;
  }
  .preset.selected {
    border-color: var(--desk-text);
    background: var(--desk-hover);
  }
  .preset strong {
    font-size: 12px;
    font-weight: 550;
    overflow-wrap: anywhere;
  }
  .preset small {
    font-size: 10px;
    color: var(--desk-muted);
    overflow-wrap: anywhere;
  }
  .key-label {
    display: block;
    font-size: 13px;
    margin-bottom: 7px;
  }
  .key-heading {
    display: flex;
    flex-wrap: wrap;
    justify-content: space-between;
    gap: 4px 12px;
  }
  .key-heading span {
    color: var(--desk-muted);
    font-size: 12px;
    overflow-wrap: anywhere;
  }
  .api-key {
    width: 100%;
    background: var(--desk-input);
    border: 1px solid var(--desk-border);
    border-radius: 7px;
    padding: 10px 12px;
  }
  .key-hint {
    font-size: 11px;
    margin-top: 8px;
  }
  .endpoint {
    font-size: 11px;
    color: var(--desk-muted);
    overflow-wrap: anywhere;
    margin-top: 6px;
  }
  .connect-button {
    width: 100%;
    margin-top: 20px;
    min-height: 40px;
  }
  .result {
    display: grid;
    justify-items: center;
    gap: 18px;
    padding: 24px 0;
    text-align: center;
  }
  .model-name {
    overflow-wrap: anywhere;
    max-width: 100%;
  }
  .setup-error {
    font-size: 13px;
    color: var(--destructive, #b34736);
    overflow-wrap: anywhere;
    margin: 16px 0;
  }
  footer {
    display: flex;
    flex-wrap: wrap;
    justify-content: space-between;
    gap: 12px;
    margin-top: 24px;
  }
  .text-button {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    font-size: 12px;
    color: var(--desk-muted);
  }
  button:disabled,
  fieldset:disabled {
    opacity: 0.55;
    cursor: default;
  }
  button:focus-visible,
  input:focus-visible {
    outline: 2px solid var(--desk-text);
    outline-offset: 3px;
  }
  .model-setup :global(.spinner) {
    animation: setup-spin 1s linear infinite;
  }
  @keyframes setup-spin {
    to {
      transform: rotate(360deg);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .model-setup :global(.spinner) {
      animation: none;
    }
  }
  @media (max-width: 520px) {
    .setup-backdrop {
      padding: 12px;
    }
    .model-setup {
      padding: 22px;
      max-height: calc(100dvh - 24px);
    }
    .presets {
      grid-template-columns: 1fr;
      max-height: min(210px, 25dvh);
    }
  }
</style>
