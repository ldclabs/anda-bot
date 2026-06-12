<script lang="ts">
  import { applyAppearanceTheme } from '$lib/anda/theme'
  import {
    buttonClass,
    fieldClass,
    fieldLabelClass,
    inputClass,
    nativeSelectClass,
    nativeSelectWrapperClass,
    textareaClass
  } from '$lib/anda/ui'
  import { defaultSettings, errorToMessage } from '$lib/service-worker/settings'
  import type { SettingsState } from '$lib/service-worker/types'
  import {
    AlertCircle,
    Check,
    ChevronDown,
    FileCode2,
    LoaderCircle,
    Plus,
    RefreshCw,
    Save,
    SlidersHorizontal,
    Trash2
  } from '@lucide/svelte'
  import { onMount, tick } from 'svelte'
  import { DaemonConfigApi, loadConfigSettings, saveConfigSettings } from './lib/anda/config/api'
  import type { FieldSchema, JsonObject } from './lib/anda/config/schema'
  import {
    asObject,
    booleanValue,
    channelSchemas,
    createChannel,
    createModelProvider,
    createTranscriptionProvider,
    createTtsProvider,
    createUser,
    getObject,
    modelProviderFields,
    normalizeConfigDraft,
    numberValue,
    objectArray,
    optionalObject,
    parseConfigDraft,
    removeArrayItem,
    renderConfigYaml,
    runtimeFields,
    setNumberValue,
    setStringListValue,
    setStringValue,
    stringListValue,
    stringValue,
    transcriptionFields,
    transcriptionProviderSchemas,
    ttsFields,
    ttsProviderSchemas,
    userFields
  } from './lib/anda/config/schema'

  type SectionId = 'runtime' | 'models' | 'tts' | 'transcription' | 'channels' | 'users'

  const sections: { id: SectionId; label: string; detail: string }[] = [
    { id: 'runtime', label: 'Runtime', detail: 'Gateway, proxy, and workspaces' },
    { id: 'models', label: 'Models', detail: 'Provider list and active model' },
    { id: 'tts', label: 'TTS', detail: 'Speech synthesis providers' },
    { id: 'transcription', label: 'Transcription', detail: 'Voice-to-text providers' },
    { id: 'channels', label: 'Channels', detail: 'IM channel runtimes' },
    { id: 'users', label: 'Users', detail: 'Shared daemon identities' }
  ]

  let settings = $state<SettingsState>({ ...defaultSettings })
  let draft = $state<JsonObject>(normalizeConfigDraft({}))
  let source = $state('')
  let configPath = $state('')
  let activeSection = $state<SectionId>('runtime')
  let loading = $state(true)
  let saving = $state(false)
  let dirty = $state(false)
  let statusMessage = $state('')
  let errorMessage = $state('')
  let formPanel = $state<HTMLElement | null>(null)

  const model = $derived(getObject(draft, 'model'))
  const tts = $derived(getObject(draft, 'tts'))
  const transcription = $derived(getObject(draft, 'transcription'))
  const channels = $derived(getObject(draft, 'channels'))

  $effect(() => {
    applyAppearanceTheme(settings.appearanceTheme)
  })

  onMount(() => {
    loadConfig().catch((error) => {
      errorMessage = errorToMessage(error)
      loading = false
    })
  })

  async function loadConfig() {
    loading = true
    errorMessage = ''
    statusMessage = ''
    settings = await loadConfigSettings()
    const response = await new DaemonConfigApi(settings).load()
    draft = normalizeConfigDraft(response.config)
    source = response.content
    configPath = response.path
    dirty = false
    loading = false
  }

  async function reconnect() {
    loading = true
    errorMessage = ''
    statusMessage = ''
    try {
      await saveConfigSettings(settings)
      await loadConfig()
    } catch (error) {
      errorMessage = errorToMessage(error)
      loading = false
    }
  }

  function markFormDirty() {
    source = renderConfigYaml(draft, source)
    dirty = true
    statusMessage = ''
    errorMessage = ''
  }

  function markSourceDirty(event: Event) {
    source = (event.currentTarget as HTMLTextAreaElement).value
    // Keep the form in sync with hand-edited YAML so a later form edit does
    // not overwrite manual changes with stale draft values.
    const parsed = parseConfigDraft(source)
    if (parsed) {
      draft = parsed
    }
    dirty = true
    statusMessage = ''
    errorMessage = ''
  }

  function updateString(target: JsonObject, field: FieldSchema, event: Event) {
    setStringValue(
      target,
      field.key,
      (event.currentTarget as HTMLInputElement).value,
      field.nullable
    )
    markFormDirty()
  }

  function updateNumber(target: JsonObject, field: FieldSchema, event: Event) {
    setNumberValue(
      target,
      field.key,
      (event.currentTarget as HTMLInputElement).value,
      field.nullable
    )
    markFormDirty()
  }

  function updateBoolean(target: JsonObject, field: FieldSchema, event: Event) {
    target[field.key] = (event.currentTarget as HTMLInputElement).checked
    markFormDirty()
  }

  function updateStringList(target: JsonObject, field: FieldSchema, event: Event) {
    setStringListValue(target, field.key, (event.currentTarget as HTMLTextAreaElement).value)
    markFormDirty()
  }

  function updateSettingsString(key: 'baseUrl' | 'token', event: Event) {
    settings = {
      ...settings,
      [key]: (event.currentTarget as HTMLInputElement).value
    }
  }

  function addObjectItem(target: JsonObject, key: string, value: JsonObject) {
    const items = target[key]
    if (Array.isArray(items)) {
      items.push(value)
    } else {
      target[key] = [value]
    }
    markFormDirty()
  }

  async function addModelProvider() {
    const providers = Array.isArray(model.providers) ? model.providers : []
    const nextIndex = providers.length
    addObjectItem(model, 'providers', createModelProvider())
    await tick()
    scrollFormPanelTo(`[data-provider-index="${nextIndex}"]`)
  }

  function removeObjectItem(target: JsonObject, key: string, index: number) {
    removeArrayItem(target, key, index)
    markFormDirty()
  }

  function setOptionalProvider(
    target: JsonObject,
    key: string,
    enabled: boolean,
    value: JsonObject
  ) {
    target[key] = enabled ? value : null
    markFormDirty()
  }

  function formatFromForm() {
    source = renderConfigYaml(draft, source)
    dirty = true
    statusMessage = 'YAML formatted from the form. Comments from the previous file were preserved.'
  }

  function scrollFormPanelTo(selector: string) {
    const target = formPanel?.querySelector(selector)
    if (!target) {
      return
    }

    target.scrollIntoView({
      behavior: 'smooth',
      block: 'start'
    })
  }

  async function saveConfig() {
    saving = true
    errorMessage = ''
    statusMessage = ''
    try {
      await saveConfigSettings(settings)
      const response = await new DaemonConfigApi(settings).save(source)
      draft = normalizeConfigDraft(response.config)
      source = response.content
      configPath = response.path
      dirty = false
      statusMessage = 'Saved config.yaml. Restart Anda for daemon startup settings to take effect.'
    } catch (error) {
      errorMessage = errorToMessage(error)
    } finally {
      saving = false
    }
  }
</script>

{#snippet fieldControl(target: JsonObject, field: FieldSchema)}
  <div data-slot="field" class={fieldClass('gap-1.5')}>
    <label
      class={fieldLabelClass('text-xs font-bold text-muted-foreground')}
      for={`${field.key}-${field.label}`}
    >
      {field.label}
    </label>

    {#if field.kind === 'boolean'}
      <label
        class="flex h-9 items-center gap-2 rounded-md border bg-background px-2.5 text-sm shadow-xs"
      >
        <input
          type="checkbox"
          class="size-4 accent-foreground"
          checked={booleanValue(target, field.key)}
          onchange={(event) => updateBoolean(target, field, event)}
        />
        <span class="text-muted-foreground"
          >{booleanValue(target, field.key) ? 'Enabled' : 'Disabled'}</span
        >
      </label>
    {:else if field.kind === 'select'}
      <div class={nativeSelectWrapperClass('w-full')} data-slot="native-select-wrapper">
        <select
          class={nativeSelectClass()}
          value={stringValue(target, field.key)}
          onchange={(event) => updateString(target, field, event)}
        >
          {#each field.options || [] as option}
            <option class="bg-[Canvas] text-[CanvasText]" value={option}>{option}</option>
          {/each}
        </select>
        <ChevronDown
          class="pointer-events-none absolute top-1/2 right-2.5 size-4 -translate-y-1/2 text-muted-foreground"
          aria-hidden="true"
        />
      </div>
    {:else if field.kind === 'number'}
      <input
        class={inputClass('h-9 text-sm')}
        type="number"
        value={numberValue(target, field.key)}
        placeholder={field.nullable ? 'optional' : undefined}
        oninput={(event) => updateNumber(target, field, event)}
      />
    {:else if field.kind === 'string-list'}
      <textarea
        class={textareaClass('min-h-20 resize-y font-mono text-xs')}
        spellcheck={false}
        placeholder="One item per line"
        value={stringListValue(target, field.key)}
        oninput={(event) => updateStringList(target, field, event)}
      ></textarea>
    {:else if field.kind === 'object'}
      <div class="grid gap-3 rounded-md border bg-muted/20 p-3">
        {#each field.fields || [] as child}
          {@render fieldControl(getObject(target, field.key), child)}
        {/each}
      </div>
    {:else}
      <input
        class={inputClass('h-9 text-sm')}
        type={field.kind === 'secret' ? 'password' : 'text'}
        autocomplete="off"
        spellcheck={false}
        value={stringValue(target, field.key)}
        placeholder={field.placeholder || (field.nullable ? 'optional' : undefined)}
        oninput={(event) => updateString(target, field, event)}
      />
    {/if}
  </div>
{/snippet}

{#snippet objectEditor(target: JsonObject, fields: FieldSchema[])}
  <div class="grid gap-3 sm:grid-cols-2 xl:grid-cols-3">
    {#each fields as field}
      <div
        class={field.kind === 'string-list' || field.kind === 'object'
          ? 'sm:col-span-2 xl:col-span-3'
          : ''}
      >
        {@render fieldControl(target, field)}
      </div>
    {/each}
  </div>
{/snippet}

{#snippet sectionHeader(title: string, description: string)}
  <div class="grid gap-1">
    <h2 class="text-base font-bold tracking-normal text-foreground">{title}</h2>
    <p class="max-w-3xl text-xs leading-relaxed text-muted-foreground">{description}</p>
  </div>
{/snippet}

{#snippet arraySection(title: string, description: string, actionLabel: string, onAdd: () => void)}
  <div class="flex flex-wrap items-start justify-between gap-3">
    {@render sectionHeader(title, description)}
    <button type="button" class={buttonClass('outline', 'sm', 'bg-background')} onclick={onAdd}>
      <Plus class="size-3.5" />
      {actionLabel}
    </button>
  </div>
{/snippet}

<svelte:head>
  <title>Anda config.yaml</title>
</svelte:head>

<div class="min-h-screen bg-background text-foreground">
  <header class="border-b bg-muted/25">
    <div
      class="mx-auto flex max-w-7xl flex-col gap-4 px-4 py-4 sm:px-5 lg:flex-row lg:items-center lg:justify-between"
    >
      <div class="grid min-w-0 gap-1">
        <div class="flex min-w-0 items-center gap-2">
          <FileCode2 class="size-5 shrink-0 text-emerald-800" />
          <h1 class="truncate text-lg font-bold">config.yaml</h1>
        </div>
        <p class="truncate text-xs text-muted-foreground">
          {configPath || 'Connect to the local Anda daemon to load the runtime configuration.'}
        </p>
      </div>

      <div class="grid gap-2 sm:grid-cols-[14rem_16rem_auto]">
        <input
          class={inputClass('h-8 text-xs')}
          value={settings.baseUrl}
          spellcheck={false}
          aria-label="Gateway URL"
          oninput={(event) => updateSettingsString('baseUrl', event)}
        />
        <input
          class={inputClass('h-8 text-xs')}
          value={settings.token}
          type="password"
          autocomplete="off"
          spellcheck={false}
          aria-label="Bearer token"
          placeholder="Bearer token"
          oninput={(event) => updateSettingsString('token', event)}
        />
        <button
          type="button"
          class={buttonClass('outline', 'sm', 'bg-background')}
          onclick={reconnect}
          disabled={loading}
        >
          {#if loading}
            <LoaderCircle class="size-3.5 animate-spin" />
          {:else}
            <RefreshCw class="size-3.5" />
          {/if}
          Load
        </button>
      </div>
    </div>
  </header>

  <main
    class="mx-auto grid max-w-7xl gap-4 px-4 py-4 sm:px-5 lg:h-[calc(100vh-6rem)] lg:min-h-0 lg:grid-cols-[15rem_minmax(0,1fr)_minmax(24rem,0.8fr)]"
  >
    <aside class="min-w-0 lg:sticky lg:top-4 lg:self-start">
      <nav class="grid gap-1 rounded-lg border bg-background p-1 shadow-xs">
        {#each sections as section}
          <button
            type="button"
            class={`grid min-w-0 gap-0.5 rounded-md px-3 py-2 text-left transition ${
              activeSection === section.id
                ? 'bg-muted text-foreground shadow-xs'
                : 'text-muted-foreground hover:bg-muted/55 hover:text-foreground'
            }`}
            onclick={() => (activeSection = section.id)}
          >
            <span class="truncate text-sm font-bold">{section.label}</span>
            <span class="truncate text-[10px] font-medium">{section.detail}</span>
          </button>
        {/each}
      </nav>
    </aside>

    <section
      class="flex min-h-0 min-w-0 flex-col overflow-hidden rounded-lg border bg-background shadow-xs lg:h-full"
    >
      {#if loading}
        <div class="grid min-h-80 place-items-center gap-2 p-8 text-muted-foreground">
          <LoaderCircle class="size-5 animate-spin" />
          <span class="text-sm font-medium">Loading config.yaml</span>
        </div>
      {:else}
        <div
          class="scrollbar-slim grid min-h-0 flex-1 auto-rows-max content-start gap-6 overflow-y-auto p-4 sm:p-5"
          bind:this={formPanel}
        >
          {#if activeSection === 'runtime'}
            {@render sectionHeader(
              'Runtime',
              'Core daemon settings. Keep the side panel settings focused on browser connection details; use this page for the full config file.'
            )}
            {@render objectEditor(draft, runtimeFields)}
          {:else if activeSection === 'models'}
            <div class="grid gap-5">
              {@render sectionHeader(
                'Models',
                'Choose the active model and maintain every provider entry available to the daemon.'
              )}
              <div class="grid gap-3 sm:grid-cols-2">
                {@render fieldControl(model, {
                  key: 'active',
                  label: 'Active model',
                  kind: 'text'
                })}
              </div>
              {@render arraySection(
                'Providers',
                'Labels route memory, flash, image, audio, and video requests.',
                'Add provider',
                addModelProvider
              )}
              <div class="grid gap-3">
                {#each objectArray(model, 'providers') as provider, index}
                  <div
                    class="grid scroll-mt-5 gap-3 rounded-lg border bg-muted/15 p-3"
                    data-provider-index={index}
                  >
                    <div class="flex items-center justify-between gap-3">
                      <div class="min-w-0">
                        <h3 class="truncate text-sm font-bold">
                          {stringValue(provider, 'model') || `Provider ${index + 1}`}
                        </h3>
                        <p class="truncate text-xs text-muted-foreground">
                          {stringValue(provider, 'family') || 'provider'}
                        </p>
                      </div>
                      <button
                        type="button"
                        class={buttonClass('ghost', 'icon-sm')}
                        title="Remove provider"
                        aria-label="Remove provider"
                        onclick={() => removeObjectItem(model, 'providers', index)}
                      >
                        <Trash2 class="size-4" />
                      </button>
                    </div>
                    {@render objectEditor(provider, modelProviderFields)}
                  </div>
                {/each}
              </div>
            </div>
          {:else if activeSection === 'tts'}
            <div class="grid gap-5">
              {@render sectionHeader(
                'Text to speech',
                'Enable speech synthesis and configure any provider block you want saved under tts.'
              )}
              {@render objectEditor(tts, ttsFields)}
              <div class="grid gap-3">
                {#each Object.entries(ttsProviderSchemas) as [provider, fields]}
                  <div class="grid gap-3 rounded-lg border bg-muted/15 p-3">
                    <label class="flex items-center justify-between gap-3">
                      <span class="grid gap-0.5">
                        <span class="text-sm font-bold">{provider}</span>
                        <span class="text-xs text-muted-foreground">Provider block</span>
                      </span>
                      <input
                        type="checkbox"
                        class="size-4 accent-foreground"
                        checked={Boolean(optionalObject(tts, provider))}
                        onchange={(event) =>
                          setOptionalProvider(
                            tts,
                            provider,
                            (event.currentTarget as HTMLInputElement).checked,
                            createTtsProvider(provider)
                          )}
                      />
                    </label>
                    {#if optionalObject(tts, provider)}
                      {@render objectEditor(asObject(tts[provider]), fields)}
                    {/if}
                  </div>
                {/each}
              </div>
            </div>
          {:else if activeSection === 'transcription'}
            <div class="grid gap-5">
              {@render sectionHeader(
                'Transcription',
                'Configure voice transcription defaults and provider-specific options.'
              )}
              {@render objectEditor(transcription, transcriptionFields)}
              <div class="grid gap-3">
                {#each Object.entries(transcriptionProviderSchemas) as [provider, fields]}
                  <div class="grid gap-3 rounded-lg border bg-muted/15 p-3">
                    <label class="flex items-center justify-between gap-3">
                      <span class="grid gap-0.5">
                        <span class="text-sm font-bold">{provider}</span>
                        <span class="text-xs text-muted-foreground">Provider block</span>
                      </span>
                      <input
                        type="checkbox"
                        class="size-4 accent-foreground"
                        checked={Boolean(optionalObject(transcription, provider))}
                        onchange={(event) =>
                          setOptionalProvider(
                            transcription,
                            provider,
                            (event.currentTarget as HTMLInputElement).checked,
                            createTranscriptionProvider(provider)
                          )}
                      />
                    </label>
                    {#if optionalObject(transcription, provider)}
                      {@render objectEditor(asObject(transcription[provider]), fields)}
                    {/if}
                  </div>
                {/each}
              </div>
            </div>
          {:else if activeSection === 'channels'}
            <div class="grid gap-5">
              {@render sectionHeader(
                'Channels',
                'Configure Telegram, WeChat, Discord, and Lark/Feishu runtimes. Replies stay routed by each platform route.'
              )}
              {#each Object.entries(channelSchemas) as [channel, fields]}
                <div class="grid gap-3 rounded-lg border bg-muted/15 p-3">
                  {@render arraySection(
                    channel,
                    `${objectArray(channels, channel).length} configured`,
                    `Add ${channel}`,
                    () => addObjectItem(channels, channel, createChannel(channel))
                  )}
                  <div class="grid gap-3">
                    {#each objectArray(channels, channel) as item, index}
                      <div class="grid gap-3 rounded-md border bg-background p-3">
                        <div class="flex items-center justify-between gap-3">
                          <div class="min-w-0">
                            <h3 class="truncate text-sm font-bold">
                              {stringValue(item, 'id') || `${channel} ${index + 1}`}
                            </h3>
                            <p class="truncate text-xs text-muted-foreground">
                              {stringValue(item, 'username') ||
                                stringValue(item, 'user') ||
                                channel}
                            </p>
                          </div>
                          <button
                            type="button"
                            class={buttonClass('ghost', 'icon-sm')}
                            title={`Remove ${channel}`}
                            aria-label={`Remove ${channel}`}
                            onclick={() => removeObjectItem(channels, channel, index)}
                          >
                            <Trash2 class="size-4" />
                          </button>
                        </div>
                        {@render objectEditor(item, fields)}
                      </div>
                    {/each}
                  </div>
                </div>
              {/each}
            </div>
          {:else if activeSection === 'users'}
            <div class="grid gap-5">
              {@render arraySection(
                'Users',
                'Trusted users that can share this daemon and the same Anda agent.',
                'Add user',
                () => addObjectItem(draft, 'users', createUser())
              )}
              <div class="grid gap-3">
                {#each objectArray(draft, 'users') as user, index}
                  <div class="grid gap-3 rounded-lg border bg-muted/15 p-3">
                    <div class="flex items-center justify-between gap-3">
                      <h3 class="truncate text-sm font-bold">
                        {stringValue(user, 'id') || `User ${index + 1}`}
                      </h3>
                      <button
                        type="button"
                        class={buttonClass('ghost', 'icon-sm')}
                        title="Remove user"
                        aria-label="Remove user"
                        onclick={() => removeObjectItem(draft, 'users', index)}
                      >
                        <Trash2 class="size-4" />
                      </button>
                    </div>
                    {@render objectEditor(user, userFields)}
                  </div>
                {/each}
              </div>
            </div>
          {/if}
        </div>
      {/if}
    </section>

    <aside class="min-w-0 lg:sticky lg:top-4 lg:self-start">
      <div class="grid overflow-hidden rounded-lg border bg-background shadow-xs">
        <div class="flex items-center justify-between gap-3 border-b bg-muted/25 px-3 py-2">
          <div class="flex min-w-0 items-center gap-2">
            <SlidersHorizontal class="size-4 shrink-0 text-emerald-800" />
            <span class="truncate text-sm font-bold">YAML source</span>
          </div>
          <div class="flex shrink-0 items-center gap-1">
            <button
              type="button"
              class={buttonClass('ghost', 'xs')}
              onclick={formatFromForm}
              disabled={loading}
            >
              Format form
            </button>
            <button
              type="button"
              class={buttonClass('default', 'xs')}
              onclick={saveConfig}
              disabled={saving || loading || !dirty}
            >
              {#if saving}
                <LoaderCircle class="size-3 animate-spin" />
              {:else}
                <Save class="size-3" />
              {/if}
              Save
            </button>
          </div>
        </div>
        <textarea
          class="min-h-[34rem] w-full resize-y bg-transparent p-3 font-mono text-xs leading-relaxed outline-none"
          spellcheck={false}
          value={source}
          oninput={markSourceDirty}
        ></textarea>
      </div>

      {#if statusMessage}
        <div
          class="mt-3 grid grid-cols-[auto_1fr] gap-2 rounded-lg border border-emerald-900/10 bg-emerald-50 px-3 py-2 text-xs text-emerald-900 dark:bg-emerald-950/35 dark:text-emerald-100"
        >
          <Check class="mt-0.5 size-3.5" />
          <span>{statusMessage}</span>
        </div>
      {/if}
      {#if errorMessage}
        <div
          class="mt-3 grid grid-cols-[auto_1fr] gap-2 rounded-lg border border-destructive/20 bg-destructive/10 px-3 py-2 text-xs text-destructive"
        >
          <AlertCircle class="mt-0.5 size-3.5" />
          <span>{errorMessage}</span>
        </div>
      {/if}
    </aside>
  </main>
</div>
