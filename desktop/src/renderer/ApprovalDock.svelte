<script lang="ts">
  /**
   * The chat's open question, docked above the composer so it cannot scroll
   * out of reach. Approvals are answered here (or with their shortcuts);
   * a choice jumps to its card in the transcript, where its options and any
   * free-text answer live. The card in the transcript stays the record.
   */
  import {
    CircleCheck,
    CircleX,
    ListChecks,
    LoaderCircle,
    ShieldCheck,
    Terminal
  } from '@lucide/svelte'
  import {
    actionApproveLabel,
    actionDenyLabel,
    actionKindLabel,
    actionMessage,
    actionTitle,
    isApprovalAction,
    isShellApproval
  } from '$lib/anda/chat/action-view'
  import type { ChatAction } from '$lib/anda/client/types'
  import { shortcutLabel } from '../shared/shortcuts'
  import type { Label } from './labels'
  import { tip } from './tooltip'

  let {
    pending,
    disabled,
    platform,
    t,
    onRespond,
    onJump
  }: {
    pending: Array<{ action: ChatAction; messageId: string }>
    disabled: boolean
    platform: string
    t: (key: Label, values?: Record<string, string>) => string
    onRespond: (action: ChatAction, approve: boolean) => Promise<void>
    onJump: (messageId: string) => void
  } = $props()

  const first = $derived(pending[0])
  const approval = $derived(first ? isApprovalAction(first.action) : false)
  let responding = $state('')

  /** Answers the docked approval; the shortcuts call this too. */
  export async function respond(approve: boolean) {
    const action = first?.action
    if (!action || !approval || disabled || responding) return
    responding = action.id
    try {
      await onRespond(action, approve)
    } finally {
      responding = ''
    }
  }
</script>

{#if first}
  {@const action = first.action}
  {@const detail = action.command || action.summary || actionMessage(action) || ''}
  <div class="approval-dock" role="region" aria-label={t('approvalWaiting')}>
    <span class="approval-dock-icon"
      >{#if isShellApproval(action)}<Terminal size={15} />{:else if approval}<ShieldCheck
          size={15}
        />{:else}<ListChecks size={15} />{/if}</span
    >
    <div class="approval-dock-text">
      <strong>{actionTitle(action) || actionKindLabel(action)}</strong>
      {#if detail}<code title={detail}>{detail}</code>{/if}
    </div>
    {#if pending.length > 1}<span class="approval-dock-more"
        >{t('moreWaiting', { count: String(pending.length - 1) })}</span
      >{/if}
    {#if approval}
      <button
        class="dock-deny"
        disabled={disabled || Boolean(responding)}
        use:tip={{ text: actionDenyLabel(action), shortcut: shortcutLabel('deny', platform) }}
        onclick={() => void respond(false)}><CircleX size={14} />{actionDenyLabel(action)}</button
      ><button
        class="primary dock-approve"
        disabled={disabled || Boolean(responding)}
        use:tip={{ text: actionApproveLabel(action), shortcut: shortcutLabel('approve', platform) }}
        onclick={() => void respond(true)}
        >{#if responding}<LoaderCircle size={14} class="animate-spin" />{:else}<CircleCheck
            size={14}
          />{/if}{actionApproveLabel(action)}</button
      >
    {:else}
      <button onclick={() => onJump(first.messageId)}>{t('answerInChat')}</button>
    {/if}
  </div>
{/if}
