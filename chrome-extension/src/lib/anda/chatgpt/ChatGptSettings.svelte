<script lang="ts">
  import { onMount, onDestroy } from 'svelte'
  import { getMessage } from '$lib/i18n'
  import { buttonClass, inputClass } from '../ui'
  import type { SettingsState } from '../client/types'
  import {
    chatgptRequest,
    openChatGptUrl,
    usageUrl,
    type ChatGptAccounts,
    type ChatGptLogin,
    type ChatGptModel
  } from './api'
  let {
    settings,
    onModelSelected
  }: { settings: SettingsState; onModelSelected?: () => void | Promise<void> } = $props()
  let accounts = $state<ChatGptAccounts>({ accounts: [], needs_setup: false })
  let profile = $state('')
  let models = $state<ChatGptModel[]>([])
  let model = $state('')
  let busy = $state(false)
  let error = $state('')
  let notice = $state('')
  let flow = $state<ChatGptLogin | null>(null)
  let alive = true
  let generation = 0
  const selected = $derived(accounts.accounts.find((a) => a.id === profile))
  const message = (error: unknown) => (error instanceof Error ? error.message : String(error))
  async function refresh() {
    const result = await chatgptRequest<ChatGptAccounts>(settings, { method: 'accounts' })
    if (!Array.isArray(result.accounts))
      throw new Error('Update the Anda daemon to connect ChatGPT')
    accounts = result
    if (!accounts.accounts.some((a) => a.id === profile))
      profile = accounts.active || accounts.accounts[0]?.id || ''
    await loadModels()
  }
  async function loadModels() {
    models = []
    model = ''
    if (!profile || !accounts.accounts.find((a) => a.id === profile)?.plan_enabled) return
    const result = await chatgptRequest<{ models: ChatGptModel[] }>(settings, {
      method: 'models',
      params: { profile_id: profile }
    })
    models = result.models
    model = models[0]?.slug || ''
  }
  async function action(work: () => Promise<void>) {
    if (busy) return
    busy = true
    error = ''
    notice = ''
    try {
      await work()
    } catch (e) {
      error = message(e)
    } finally {
      busy = false
    }
  }
  async function signIn(reconnect = false) {
    await action(async () => {
      const attempt = ++generation
      flow = await chatgptRequest<ChatGptLogin>(settings, {
        method: 'login_start',
        params: reconnect ? { profile_id: profile, consent: !selected?.plan_enabled } : {}
      })
      if (flow.authorization_url) await openChatGptUrl(flow.authorization_url)
      while (alive && generation === attempt) {
        await new Promise((resolve) => setTimeout(resolve, 1500))
        if (!alive || generation !== attempt || !flow) return
        const current = await chatgptRequest<ChatGptLogin>(settings, {
          method: 'login_status',
          params: { flow_id: flow.flow_id }
        })
        if (current.status === 'completed') {
          profile = current.account_id || ''
          flow = null
          await refresh()
          notice = getMessage('chatgptConnectedNotice')
          return
        }
        if (!['pending', 'exchanging'].includes(current.status)) {
          flow = null
          throw new Error(current.error || current.status)
        }
      }
    })
  }
  async function cancel() {
    const pending = flow
    generation++
    flow = null
    if (pending)
      try {
        await chatgptRequest(settings, {
          method: 'login_cancel',
          params: { flow_id: pending.flow_id }
        })
      } catch (e) {
        error = message(e)
      }
    busy = false
  }
  async function useModel() {
    await action(async () => {
      await chatgptRequest(settings, {
        method: 'model_select',
        params: { profile_id: profile, model }
      })
      notice = getMessage('chatgptUsingPlan')
      await onModelSelected?.()
    })
  }
  onMount(() => {
    void action(refresh)
  })
  onDestroy(() => {
    alive = false
    generation++
    if (flow)
      void chatgptRequest(settings, {
        method: 'login_cancel',
        params: { flow_id: flow.flow_id }
      }).catch(() => {})
  })
</script>

<section
  class="grid min-w-0 gap-3 rounded-lg border bg-muted/15 p-4"
  aria-label={getMessage('chatgptTitle')}
>
  <div class="grid gap-1">
    <h2 class="text-sm font-semibold">{getMessage('chatgptTitle')}</h2>
    <p class="text-xs text-muted-foreground">{getMessage('chatgptDescription')}</p>
  </div>
  {#if accounts.accounts.length}
    <label class="grid gap-1 text-xs"
      >{getMessage('chatgptAccount')}
      <select
        class={inputClass('w-full')}
        bind:value={profile}
        disabled={busy}
        onchange={() =>
          void action(async () => {
            await loadModels()
          })}
      >
        {#each accounts.accounts as account}<option value={account.id}
            >{account.label} · {account.id.slice(0, 6)}{account.connected
              ? ''
              : ` · ${getMessage('chatgptReconnect')}`}</option
          >{/each}
      </select>
    </label>
    {#if selected?.connected && !selected.plan_enabled}<p class="text-xs text-muted-foreground">
        {getMessage('chatgptPermissionDisabled')}
      </p>{/if}
    {#if models.length}
      <label class="grid gap-1 text-xs"
        >{getMessage('activeModel')}
        <select class={inputClass('w-full')} bind:value={model} disabled={busy}
          >{#each models as option}<option value={option.slug}>{option.display_name}</option
            >{/each}</select
        >
      </label>
    {/if}
  {/if}
  <div class="flex flex-wrap gap-2">
    <button class={buttonClass('default', 'sm')} disabled={busy} onclick={() => void signIn()}
      >{getMessage('chatgptContinue')}</button
    >
    {#if selected}
      <button class={buttonClass('outline', 'sm')} disabled={busy} onclick={() => void signIn(true)}
        >{getMessage('chatgptReconnect')}</button
      >
      {#if models.length}<button
          class={buttonClass('outline', 'sm')}
          disabled={busy || !model}
          onclick={() => void useModel()}>{getMessage('chatgptUseModel')}</button
        >{/if}
      {#if selected.connected}<button
          class={buttonClass('ghost', 'sm')}
          disabled={busy}
          onclick={() =>
            void action(async () => {
              const result = await chatgptRequest<{ revocation_confirmed: boolean }>(settings, {
                method: 'logout',
                params: { profile_id: profile }
              })
              await refresh()
              if (!result.revocation_confirmed) notice = getMessage('chatgptRevocationUnconfirmed')
            })}>{getMessage('chatgptLogout')}</button
        >{/if}
    {/if}
    <button class={buttonClass('ghost', 'sm')} disabled={busy} onclick={() => void action(refresh)}
      >{getMessage('refreshModels')}</button
    >
    <button
      class={buttonClass('link', 'sm')}
      onclick={() => void openChatGptUrl(usageUrl).catch((e) => (error = message(e)))}
      >{getMessage('chatgptManageUsage')}</button
    >
  </div>
  {#if flow}<div class="flex flex-wrap items-center gap-2 text-xs">
      <span>{getMessage('chatgptWaiting')}</span><button
        class={buttonClass('ghost', 'sm')}
        onclick={() => void cancel()}>{getMessage('cancel')}</button
      >{#if flow.authorization_url}<button
          class={buttonClass('link', 'sm')}
          onclick={() =>
            void openChatGptUrl(flow!.authorization_url!).catch((e) => (error = message(e)))}
          >{getMessage('chatgptContinue')}</button
        >{/if}
    </div>{/if}
  {#if notice}<p class="text-xs text-muted-foreground" role="status">{notice}</p>{/if}
  {#if error}<p class="text-xs break-words text-destructive" role="alert">{error}</p>{/if}
</section>
