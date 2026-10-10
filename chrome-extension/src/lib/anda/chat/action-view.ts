import { getMessage } from '$lib/i18n'
import type { ChatAction, ChatActionChoice, ChatActionDetail } from '../client/types'

/**
 * How an agent action reads in the transcript and in the dock above the
 * composer: its title, status, tool, and the choice the user made.
 *
 * Every function here is pure, so both components only render. Two
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
  const mcpTool = /^Run MCP tool: (.+)$/.exec(action.title || '')
  if (mcpTool) {
    return getMessage('mcpToolApprovalTitle', mcpTool[1]) || action.title || ''
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
      return getMessage(
        actionResponse(action)?.auto_selected === true
          ? 'actionStatusAutoSelected'
          : 'actionStatusSelected'
      )
    case 'expired':
      return getMessage(
        actionResponse(action)?.answered_in_chat === true
          ? 'actionStatusAnsweredInChat'
          : 'actionStatusExpired'
      )
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

/**
 * The third answer of a card that can approve without asking again, or ''
 * when it offers none.
 */
export function actionRememberLabel(action: ChatAction): string {
  const label = action.approval?.rememberLabel
  if (!label) {
    return ''
  }
  return label === 'Always allow' ? getMessage('actionAlwaysAllow') || label : label
}

/** The id of the choice the user picked, or '' when none was. */
export function actionChoiceId(action: ChatAction): string {
  const choiceId = actionResponse(action)?.choice_id
  return typeof choiceId === 'string' ? choiceId : ''
}

/** The option that was picked, by the user or by default, once the action is answered. */
export function actionSelectedChoice(action: ChatAction): ChatActionChoice | undefined {
  if (action.status !== 'selected') {
    return undefined
  }
  const choiceId = actionChoiceId(action)
  return action.choices?.find((choice) => choice.id === choiceId)
}

/**
 * The option the agent recommends, or '' when it named none. The daemon picks
 * it on its own when nobody answers before the action expires.
 */
export function actionDefaultChoiceId(action: ChatAction): string {
  const choiceId = action.payload?.default_choice_id
  return typeof choiceId === 'string' && action.choices?.some((choice) => choice.id === choiceId)
    ? choiceId
    : ''
}

/** Time left until `deadline` (epoch ms) as `m:ss`, or '' once it has passed. */
export function countdownLabel(deadline: number | undefined, now: number): string {
  const seconds = deadline ? Math.ceil((deadline - now) / 1000) : 0
  if (seconds <= 0) {
    return ''
  }
  return `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, '0')}`
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
    case 'Server':
      return getMessage('actionDetailServer') || detail.label
    case 'Tool':
      return getMessage('actionDetailTool') || detail.label
    case 'Description':
      return getMessage('actionDetailDescription') || detail.label
    case 'Server hints':
      return getMessage('actionDetailServerHints') || detail.label
    case 'Review':
      return getMessage('actionDetailReview') || detail.label
    case 'Arguments':
      return getMessage('actionDetailArguments') || detail.label
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
