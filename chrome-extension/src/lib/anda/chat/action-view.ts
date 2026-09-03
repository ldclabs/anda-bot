import { getMessage } from '$lib/i18n'
import type { ChatAction, ChatActionChoice, ChatActionDetail } from '../client/types'

/**
 * How an agent action reads in the transcript: its title, status, tool, and the
 * choice the user made.
 *
 * Every function here is pure, so the transcript component only renders. Two
 * concerns are folded in: localizing the daemon's English defaults (the daemon
 * ships fallback strings like "Approve shell command", which are replaced with
 * the viewer's locale, while a custom string from a skill is left alone), and
 * reading the loosely typed `action.response` payload safely.
 */

export function isApprovalAction(action: ChatAction): boolean {
  return action.kind === 'tool_approval' || action.kind === 'shell_command'
}

export function actionPending(action: ChatAction): boolean {
  return action.status === 'pending'
}

export function actionToolName(action: ChatAction): string {
  return (action.tool?.name || '').toLowerCase()
}

/** Shell approvals get their own copy: they are the highest-risk action. */
export function isShellApproval(action: ChatAction): boolean {
  const toolName = actionToolName(action)
  return action.kind === 'shell_command' || toolName === 'shell' || toolName.includes('shell')
}

export function isPaymentApproval(action: ChatAction): boolean {
  const toolName = actionToolName(action)
  return toolName.includes('pay') || toolName.includes('payment')
}

export function actionToolLabel(action: ChatAction): string {
  if (isShellApproval(action)) {
    return getMessage('shellCommandTool')
  }
  return action.tool?.label || action.tool?.name || getMessage('actionToolFallback')
}

export function actionTitle(action: ChatAction): string {
  if (isShellApproval(action) && (!action.title || action.title === 'Approve shell command')) {
    return getMessage('shellApprovalTitle')
  }
  return action.title || ''
}

export function actionMessage(action: ChatAction): string | null | undefined {
  if (
    isShellApproval(action) &&
    (!action.message || action.message === 'The agent wants to run a local shell command.')
  ) {
    return getMessage('shellApprovalMessage')
  }
  return action.message
}

export function actionKindLabel(action: ChatAction): string {
  if (isApprovalAction(action)) {
    return getMessage('actionApprovalKindLabel', actionToolLabel(action))
  }
  if (action.kind === 'choice') {
    return getMessage('actionChoiceKindLabel')
  }
  return actionTitle(action) || getMessage('actionFallbackTitle')
}

export function actionStatusLabel(action: ChatAction): string {
  switch (action.status) {
    case 'pending':
      return getMessage('actionStatusPending')
    case 'approved':
      return getMessage('actionStatusApproved')
    case 'denied':
      return getMessage('actionStatusDenied')
    case 'selected':
      return getMessage('actionStatusSelected')
    case 'expired':
      return getMessage('actionStatusExpired')
    default:
      return action.status || getMessage('actionStatusUnknown')
  }
}

export function actionApproveLabel(action: ChatAction): string {
  const label = action.approval?.approveLabel
  return label && label !== 'Approve' ? label : getMessage('actionApprove')
}

export function actionDenyLabel(action: ChatAction): string {
  const label = action.approval?.denyLabel
  return label && label !== 'Deny' ? label : getMessage('actionDeny')
}

/** The id of the choice the user picked, or '' when none was. */
export function actionChoiceId(action: ChatAction): string {
  const choiceId = actionResponse(action)?.choice_id
  return typeof choiceId === 'string' ? choiceId : ''
}

export function actionChoiceSelected(action: ChatAction, choiceId: string): boolean {
  return action.status === 'selected' && actionChoiceId(action) === choiceId
}

/** The label of the selected choice, for the answered summary line. */
export function actionResponseLabel(action: ChatAction): string {
  if (action.status !== 'selected') {
    return ''
  }
  const choiceId = actionChoiceId(action)
  return action.choices?.find((choice) => choice.id === choiceId)?.label || choiceId
}

/**
 * Free text the user typed alongside their choice, or '' when they only picked
 * one. A value equal to the choice's own label or value is the choice itself
 * echoed back, not typed input.
 */
export function actionChoiceText(action: ChatAction): string {
  const response = actionResponse(action)
  if (typeof response?.choice_text === 'string') {
    return response.choice_text
  }
  const value = response?.value
  const selected = action.choices?.find((choice) => choice.id === actionChoiceId(action))
  if (
    selected?.input &&
    typeof value === 'string' &&
    value &&
    value !== selected.label &&
    value !== selected.value
  ) {
    return value
  }
  return ''
}

export function actionChoiceInputKey(action: ChatAction, choiceId: string): string {
  return `${action.id}:${choiceId}`
}

export function choiceHasInput(choice: ChatActionChoice): boolean {
  return Boolean(choice.input)
}

export function choiceInputRequired(choice: ChatActionChoice): boolean {
  return Boolean(choice.input?.required)
}

export function choiceInputPlaceholder(choice: ChatActionChoice): string {
  return choice.input?.placeholder || getMessage('actionChoiceInputPlaceholder')
}

export function actionDetailLabel(detail: ChatActionDetail): string {
  switch (detail.label) {
    case 'Command':
      return getMessage('actionDetailCommand')
    case 'Workspace':
      return getMessage('actionDetailWorkspace')
    case 'Approval reason':
      return getMessage('actionDetailApprovalReason')
    case 'Mode':
      return getMessage('actionDetailMode')
    case 'Environment keys':
      return getMessage('actionDetailEnvironmentKeys')
    default:
      return detail.label
  }
}

/** Renders a detail value; non-string values are shown as pretty JSON. */
export function actionDetailText(detail: ChatActionDetail): string {
  const value = detail.value
  if (typeof value === 'string') {
    if (detail.label === 'Mode') {
      if (value === 'background') {
        return getMessage('actionBackground')
      }
      if (value === 'foreground') {
        return getMessage('actionForeground')
      }
    }
    return value
  }
  return value === null ? '' : JSON.stringify(value, null, 2)
}

/** True when the detail needs its own block rather than an inline run. */
export function actionDetailIsBlock(detail: ChatActionDetail): boolean {
  return detail.format === 'code' || detail.format === 'json' || detail.format === 'list'
}

function actionResponse(action: ChatAction): Record<string, unknown> | undefined {
  return action.response && typeof action.response === 'object' && !Array.isArray(action.response)
    ? (action.response as Record<string, unknown>)
    : undefined
}
