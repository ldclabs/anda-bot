<script lang="ts">
  import ChatGptSettings from '$lib/anda/chatgpt/ChatGptSettings.svelte'
  import { getClientPlatform } from '$lib/anda/client/platform'
  import { getMessage } from '$lib/i18n'
  import { buttonClass, fieldClass, fieldLabelClass, inputClass, textareaClass } from '$lib/anda/ui'
  import DropdownMenu from '$lib/anda/DropdownMenu.svelte'
  import { defaultSettings, errorToMessage } from '$lib/service-worker/settings'
  import type { SettingsState } from '$lib/service-worker/types'
  import { AlertCircle, LoaderCircle, Plus, Save, SlidersHorizontal, Trash2 } from '@lucide/svelte'
  import { onMount, tick } from 'svelte'
  import { DaemonConfigApi, loadConfigSettings } from './lib/anda/config/api'
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
    ensureObject,
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

  let {
    onModelsChanged,
    dirty = $bindable(false)
  }: { onModelsChanged?: () => void | Promise<void>; dirty?: boolean } = $props()

  const sections: { id: SectionId; label: string; detail: string }[] = [
    {
      id: 'runtime',
      label: getMessage('configSectionRuntime'),
      detail: getMessage('configSectionRuntimeDetail')
    },
    {
      id: 'models',
      label: getMessage('configSectionModels'),
      detail: getMessage('configSectionModelsDetail')
    },
    { id: 'tts', label: 'TTS', detail: getMessage('configSectionTtsDetail') },
    {
      id: 'transcription',
      label: getMessage('configSectionTranscription'),
      detail: getMessage('configSectionTranscriptionDetail')
    },
    {
      id: 'channels',
      label: getMessage('channelsLabel'),
      detail: getMessage('configSectionChannelsDetail')
    },
    {
      id: 'users',
      label: getMessage('configSectionUsers'),
      detail: getMessage('configSectionUsersDetail')
    }
  ]

  let settings = $state<SettingsState>({ ...defaultSettings })
  let draft = $state<JsonObject>(normalizeConfigDraft({}))
  let source = $state('')
  let configRevision = $state<string | undefined>()
  let activeSection = $state<SectionId>('runtime')
  let loading = $state(true)
  let saving = $state(false)
  let savedSource = $state<string | null>(null)
  let invalidYaml = $state(false)
  let editVersion = 0
  let statusMessage = $state('')
  let errorMessage = $state('')
  let formPanel = $state<HTMLElement | null>(null)

  const model = $derived(getObject(draft, 'model'))
  const tts = $derived(getObject(draft, 'tts'))
  const transcription = $derived(getObject(draft, 'transcription'))
  const channels = $derived(getObject(draft, 'channels'))

  onMount(() => {
    void loadConfig()
  })

  $effect(() => {
    // Chrome provides a native confirmation. Electron hides the window on close.
    if (!dirty || getClientPlatform()) return
    const warn = (event: BeforeUnloadEvent) => event.preventDefault()
    window.addEventListener('beforeunload', warn)
    return () => window.removeEventListener('beforeunload', warn)
  })

  async function loadConfig() {
    loading = true
    errorMessage = ''
    statusMessage = ''
    try {
      settings = await loadConfigSettings()
      const response = await new DaemonConfigApi(settings).load()
      draft = parseConfigDraft(response.content) || normalizeConfigDraft(response.config)
      savedSource = source = response.content
      configRevision = response.revision
      invalidYaml = false
      dirty = false
    } catch (error) {
      errorMessage = errorToMessage(error)
    } finally {
      loading = false
    }
  }

  async function selectSection(section: SectionId) {
    activeSection = section
    await tick()
    if (formPanel) formPanel.scrollTop = 0
  }

  async function chatgptModelSelected() {
    const version = editVersion
    const response = await new DaemonConfigApi(settings).load()
    // A model selection updates config.yaml immediately. Preserve edits made
    // while the request was running and their original conflict-check revision.
    if (!dirty && version === editVersion) {
      draft = parseConfigDraft(response.content) || normalizeConfigDraft(response.config)
      savedSource = source = response.content
      configRevision = response.revision
    }
    await onModelsChanged?.()
  }

  function markFormDirty() {
    editVersion++
    source = renderConfigYaml(draft, source)
    dirty = source !== savedSource
    statusMessage = ''
    errorMessage = ''
  }

  function markSourceDirty(event: Event) {
    editVersion++
    source = (event.currentTarget as HTMLTextAreaElement).value
    // Keep the form in sync with hand-edited YAML so a later form edit does
    // not overwrite manual changes with stale draft values.
    const parsed = parseConfigDraft(source)
    invalidYaml = !parsed
    if (parsed) {
      draft = parsed
    }
    dirty = source !== savedSource
    statusMessage = ''
    errorMessage = ''
  }

  function updateString(target: JsonObject, field: FieldSchema, value: string) {
    setStringValue(target, field.key, value, field.nullable)
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
    editVersion++
    source = renderConfigYaml(draft, source)
    dirty = source !== savedSource
    statusMessage = getMessage('configFormatted')
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
    if (saving || loading || savedSource === null || invalidYaml || !dirty) return
    const version = editVersion
    const content = source
    saving = true
    errorMessage = ''
    statusMessage = ''
    try {
      const response = await new DaemonConfigApi(settings).save(content, configRevision)
      configRevision = response.revision
      savedSource = response.content
      if (version === editVersion) {
        draft = parseConfigDraft(response.content) || normalizeConfigDraft(response.config)
        source = response.content
        dirty = false
        statusMessage = getMessage('configSaved')
      } else {
        dirty = source !== savedSource
      }
      if (response.models_error) errorMessage = response.models_error
    } catch (error) {
      errorMessage = errorToMessage(error)
    } finally {
      saving = false
    }
  }
</script>

{#snippet fieldControl(
  target: JsonObject,
  field: FieldSchema,
  editableTarget: () => JsonObject = () => target
)}
  <div data-slot="field" class={fieldClass('gap-1.5')}>
    <!-- Controls are named by aria-label: the same key repeats across providers,
         so there is no unique id for a <label for> to point at. -->
    <span class={fieldLabelClass('text-xs font-bold text-muted-foreground')}>{field.label}</span>

    {#if field.kind === 'boolean'}
      <label
        class="flex h-9 items-center gap-2 rounded-md border bg-background px-2.5 text-sm shadow-xs"
      >
        <input
          type="checkbox"
          class="size-4 accent-foreground"
          aria-label={field.label}
          checked={booleanValue(target, field.key)}
          onchange={(event) => updateBoolean(editableTarget(), field, event)}
        />
        <span class="text-muted-foreground"
          >{booleanValue(target, field.key)
            ? getMessage('configEnabled')
            : getMessage('configDisabled')}</span
        >
      </label>
    {:else if field.kind === 'select'}
      <DropdownMenu
        items={(field.options || []).map((option) => ({ value: option, label: option }))}
        value={stringValue(target, field.key)}
        onSelect={(value) => updateString(editableTarget(), field, value)}
        ariaLabel={field.label}
      />
    {:else if field.kind === 'number'}
      <input
        class={inputClass('h-9 text-sm')}
        type="number"
        aria-label={field.label}
        value={numberValue(target, field.key)}
        placeholder={field.nullable ? getMessage('configOptionalPlaceholder') : undefined}
        oninput={(event) => updateNumber(editableTarget(), field, event)}
      />
    {:else if field.kind === 'string-list'}
      <textarea
        class={textareaClass('min-h-20 resize-y font-mono text-xs')}
        spellcheck={false}
        aria-label={field.label}
        placeholder={getMessage('configOneItemPerLine')}
        value={stringListValue(target, field.key)}
        oninput={(event) => updateStringList(editableTarget(), field, event)}></textarea>
    {:else if field.kind === 'object'}
      <div
        role="group"
        aria-label={field.label}
        class="grid gap-3 rounded-md border bg-muted/20 p-3"
      >
        {#each field.fields || [] as child}
          {@render fieldControl(getObject(target, field.key, field.initialValue), child, () =>
            ensureObject(editableTarget(), field.key, field.initialValue)
          )}
        {/each}
      </div>
    {:else}
      <input
        class={inputClass('h-9 text-sm')}
        type={field.kind === 'secret' ? 'password' : 'text'}
        aria-label={field.label}
        autocomplete="off"
        spellcheck={false}
        value={stringValue(target, field.key)}
        placeholder={field.placeholder ||
          (field.nullable ? getMessage('configOptionalPlaceholder') : undefined)}
        oninput={(event) => updateString(editableTarget(), field, event.currentTarget.value)}
      />
    {/if}
  </div>
{/snippet}

{#snippet objectEditor(target: JsonObject, fields: FieldSchema[])}
  <div class="grid gap-3 @min-[36rem]/config:grid-cols-2 @min-[70rem]/config:grid-cols-3">
    {#each fields as field}
      <div
        class={field.kind === 'string-list' || field.kind === 'object'
          ? '@min-[36rem]/config:col-span-2 @min-[70rem]/config:col-span-3'
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

<div class="@container/config flex h-full min-h-0 flex-col bg-background text-foreground">
  {#if savedSource === null}
    <div
      class="grid flex-1 place-content-center justify-items-center gap-3 overflow-y-auto px-6 py-4 text-center"
    >
      {#if loading}
        <LoaderCircle class="size-5 animate-spin text-muted-foreground" />
        <p role="status" class="text-sm">{getMessage('configLoading')}</p>
      {:else}
        <AlertCircle class="size-5 text-destructive" />
        <h2 class="text-base font-semibold">{getMessage('configLoadFailed')}</h2>
        <p role="alert" class="max-w-md text-sm break-words text-muted-foreground">
          {errorMessage}
        </p>
        <button type="button" class={buttonClass('outline', 'sm')} onclick={loadConfig}
          >{getMessage('retryAction')}</button
        >
      {/if}
    </div>
  {:else}
    <main
      class="grid min-h-0 flex-1 content-start items-start gap-4 overflow-y-auto p-3 @min-[56rem]/config:grid-cols-[12rem_minmax(0,1fr)] @min-[70rem]/config:grid-cols-[12rem_minmax(0,1fr)_minmax(20rem,0.8fr)] @min-[70rem]/config:content-stretch @min-[70rem]/config:items-stretch @min-[70rem]/config:overflow-hidden"
    >
      <aside
        class="min-w-0 @min-[56rem]/config:sticky @min-[56rem]/config:top-4 @min-[56rem]/config:self-start"
      >
        <nav
          class="grid grid-cols-2 gap-1 rounded-lg border bg-background p-1 shadow-xs @min-[36rem]/config:grid-cols-3 @min-[56rem]/config:grid-cols-1"
        >
          {#each sections as section}
            <button
              type="button"
              class={`grid min-w-0 gap-0.5 rounded-md px-3 py-2 text-left transition ${
                activeSection === section.id
                  ? 'bg-muted text-foreground shadow-xs'
                  : 'text-muted-foreground hover:bg-muted/55 hover:text-foreground'
              }`}
              aria-current={activeSection === section.id ? 'page' : undefined}
              aria-controls="config-panel"
              onclick={() => void selectSection(section.id)}
            >
              <span class="truncate text-sm font-bold">{section.label}</span>
              <span class="truncate text-[10px] font-medium">{section.detail}</span>
            </button>
          {/each}
        </nav>
      </aside>

      <section
        id="config-panel"
        aria-label={sections.find((section) => section.id === activeSection)?.label}
        class="flex min-w-0 flex-col rounded-lg border bg-background shadow-xs @min-[70rem]/config:h-full @min-[70rem]/config:min-h-0 @min-[70rem]/config:overflow-hidden"
      >
        <div
          class="scrollbar-slim grid auto-rows-max content-start gap-6 p-4 @min-[36rem]/config:p-5 @min-[70rem]/config:min-h-0 @min-[70rem]/config:flex-1 @min-[70rem]/config:overflow-y-auto"
          inert={invalidYaml || loading}
          bind:this={formPanel}
        >
          {#if activeSection === 'runtime'}
            {@render sectionHeader(
              getMessage('configSectionRuntime'),
              getMessage('configRuntimeDescription')
            )}
            {@render objectEditor(draft, runtimeFields)}
          {:else if activeSection === 'models'}
            <div class="grid gap-5">
              {@render sectionHeader(
                getMessage('configSectionModels'),
                getMessage('configModelsDescription')
              )}
              <div class="grid gap-3 @min-[36rem]/config:grid-cols-2">
                {@render fieldControl(model, {
                  key: 'active',
                  label: getMessage('activeModel'),
                  kind: 'text'
                })}
              </div>
              <ChatGptSettings
                {settings}
                modelSelectionDisabled={dirty || saving}
                onModelSelected={chatgptModelSelected}
              />
              {@render arraySection(
                getMessage('configProviders'),
                getMessage('configProvidersDescription'),
                getMessage('configAddProvider'),
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
                          {stringValue(provider, 'model') ||
                            getMessage('configProviderFallback', String(index + 1))}
                        </h3>
                        <p class="truncate text-xs text-muted-foreground">
                          {stringValue(provider, 'family') || 'provider'}
                        </p>
                      </div>
                      <button
                        type="button"
                        class={buttonClass('ghost', 'icon-sm')}
                        title={getMessage('configRemoveProvider')}
                        aria-label={getMessage('configRemoveProvider')}
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
                getMessage('configTtsTitle'),
                getMessage('configTtsDescription')
              )}
              {@render objectEditor(tts, ttsFields)}
              <div class="grid gap-3">
                {#each Object.entries(ttsProviderSchemas) as [provider, fields]}
                  <div class="grid gap-3 rounded-lg border bg-muted/15 p-3">
                    <label class="flex items-center justify-between gap-3">
                      <span class="grid gap-0.5">
                        <span class="text-sm font-bold">{provider}</span>
                        <span class="text-xs text-muted-foreground"
                          >{getMessage('configProviderBlock')}</span
                        >
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
                getMessage('configSectionTranscription'),
                getMessage('configTranscriptionDescription')
              )}
              {@render objectEditor(transcription, transcriptionFields)}
              <div class="grid gap-3">
                {#each Object.entries(transcriptionProviderSchemas) as [provider, fields]}
                  <div class="grid gap-3 rounded-lg border bg-muted/15 p-3">
                    <label class="flex items-center justify-between gap-3">
                      <span class="grid gap-0.5">
                        <span class="text-sm font-bold">{provider}</span>
                        <span class="text-xs text-muted-foreground"
                          >{getMessage('configProviderBlock')}</span
                        >
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
                getMessage('channelsLabel'),
                getMessage('configChannelsDescription')
              )}
              {#each Object.entries(channelSchemas) as [channel, fields]}
                <div class="grid gap-3 rounded-lg border bg-muted/15 p-3">
                  {@render arraySection(
                    channel,
                    getMessage(
                      'configCountConfigured',
                      String(objectArray(channels, channel).length)
                    ),
                    getMessage('configAddChannel', channel),
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
                            title={getMessage('configRemoveChannel', channel)}
                            aria-label={getMessage('configRemoveChannel', channel)}
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
                getMessage('configSectionUsers'),
                getMessage('configUsersDescription'),
                getMessage('configAddUser'),
                () => addObjectItem(draft, 'users', createUser())
              )}
              <div class="grid gap-3">
                {#each objectArray(draft, 'users') as user, index}
                  <div class="grid gap-3 rounded-lg border bg-muted/15 p-3">
                    <div class="flex items-center justify-between gap-3">
                      <h3 class="truncate text-sm font-bold">
                        {stringValue(user, 'id') ||
                          getMessage('configUserFallback', String(index + 1))}
                      </h3>
                      <button
                        type="button"
                        class={buttonClass('ghost', 'icon-sm')}
                        title={getMessage('configRemoveUser')}
                        aria-label={getMessage('configRemoveUser')}
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
      </section>

      <aside
        class="min-h-0 min-w-0 @min-[56rem]/config:col-start-2 @min-[70rem]/config:col-start-auto"
      >
        <div
          class="flex min-h-0 flex-col overflow-hidden rounded-lg border bg-background shadow-xs @min-[70rem]/config:h-full"
        >
          <div
            class="flex shrink-0 flex-wrap items-center justify-between gap-3 border-b bg-muted/25 px-3 py-2"
          >
            <div class="flex min-w-0 items-center gap-2">
              <SlidersHorizontal class="size-4 shrink-0 text-emerald-800" />
              <span class="truncate text-sm font-bold">{getMessage('configYamlSource')}</span>
            </div>
            <div class="flex shrink-0 items-center gap-1">
              <button
                type="button"
                class={buttonClass('ghost', 'xs')}
                onclick={formatFromForm}
                disabled={loading || invalidYaml}
              >
                {getMessage('configFormatForm')}
              </button>
            </div>
          </div>
          <textarea
            class="min-h-52 w-full resize-y bg-transparent p-3 font-mono text-xs leading-relaxed focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-ring @min-[70rem]/config:flex-1 @min-[70rem]/config:resize-none"
            aria-invalid={invalidYaml}
            aria-describedby={invalidYaml ? 'yaml-problem' : undefined}
            disabled={loading}
            spellcheck={false}
            aria-label={getMessage('configYamlSource')}
            value={source}
            oninput={markSourceDirty}></textarea>
        </div>
      </aside>
    </main>
    <footer
      class="flex shrink-0 flex-wrap items-center justify-end gap-2 border-t bg-background px-3 py-2"
    >
      <div class="me-auto min-w-0 text-xs">
        {#if invalidYaml}<p id="yaml-problem" role="alert" class="text-destructive">
            {getMessage('configInvalidYaml')}
          </p>
        {:else if errorMessage}<p
            role="alert"
            class="max-h-16 overflow-y-auto break-words text-destructive"
          >
            {errorMessage}
          </p>
        {:else}<p role="status" class="text-muted-foreground">
            {statusMessage || (dirty ? getMessage('configUnsaved') : '')}
          </p>{/if}
      </div>
      {#if dirty}<button
          type="button"
          class={buttonClass('ghost', 'sm')}
          disabled={loading || saving}
          onclick={loadConfig}>{getMessage('configDiscard')}</button
        >{/if}
      <button
        type="button"
        class={buttonClass('default', 'sm')}
        disabled={!dirty || loading || saving || invalidYaml}
        onclick={saveConfig}
      >
        {#if saving}<LoaderCircle class="size-3.5 animate-spin" />{:else}<Save
            class="size-3.5"
          />{/if}{getMessage('save')}
      </button>
    </footer>
  {/if}
</div>
