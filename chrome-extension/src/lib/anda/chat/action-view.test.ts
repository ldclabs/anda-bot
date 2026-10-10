import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import {
  actionApproveLabel,
  actionChoiceId,
  actionChoiceInputKey,
  actionChoiceText,
  actionDefaultChoiceId,
  actionDenyLabel,
  actionDetailIsBlock,
  actionDetailLabel,
  actionDetailUrl,
  actionDetailText,
  actionKindLabel,
  actionMessage,
  actionPending,
  actionRememberLabel,
  actionSelectedChoice,
  actionStatusLabel,
  actionTitle,
  actionToolLabel,
  choiceInputPlaceholder,
  safeLink,
  choiceInputRequired,
  countdownLabel,
  isApprovalAction,
  isPaymentApproval,
  isShellApproval
} from './action-view'
import type { ChatAction, ChatActionDetail } from '../client/types'

// Echo the message key back so assertions can name the string that was chosen.
beforeEach(() => {
  vi.stubGlobal('chrome', {
    i18n: {
      getMessage: (key: string, substitutions?: string | string[]) => {
        const subs = Array.isArray(substitutions) ? substitutions : [substitutions]
        return substitutions ? `${key}:${subs.join(',')}` : key
      }
    }
  })
})

afterEach(() => {
  vi.unstubAllGlobals()
})

function action(overrides: Partial<ChatAction> = {}): ChatAction {
  return {
    id: 'a-1',
    name: 'approve',
    status: 'pending',
    payload: {},
    ...overrides
  }
}

describe('action classification', () => {
  it('recognizes both approval kinds', () => {
    expect(isApprovalAction(action({ kind: 'tool_approval' }))).toBe(true)
    expect(isApprovalAction(action({ kind: 'shell_command' }))).toBe(true)
    expect(isApprovalAction(action({ kind: 'choice' }))).toBe(false)
  })

  it('detects a shell approval by kind or by tool name', () => {
    expect(isShellApproval(action({ kind: 'shell_command' }))).toBe(true)
    expect(isShellApproval(action({ tool: { name: 'Shell' } }))).toBe(true)
    expect(isShellApproval(action({ tool: { name: 'run_shell_command' } }))).toBe(true)
    expect(isShellApproval(action({ tool: { name: 'read_file' } }))).toBe(false)
  })

  it('detects a payment approval by tool name', () => {
    expect(isPaymentApproval(action({ tool: { name: 'send_payment' } }))).toBe(true)
    expect(isPaymentApproval(action({ tool: { name: 'PayInvoice' } }))).toBe(true)
    expect(isPaymentApproval(action({ tool: { name: 'read_file' } }))).toBe(false)
  })

  it('reports pending only for the pending status', () => {
    expect(actionPending(action({ status: 'pending' }))).toBe(true)
    expect(actionPending(action({ status: 'approved' }))).toBe(false)
  })
})

describe('action labels', () => {
  it('localizes the daemon default titles but keeps a custom one', () => {
    expect(actionTitle(action({ kind: 'shell_command', title: 'Approve shell command' }))).toBe(
      'shellApprovalTitle'
    )
    expect(actionTitle(action({ kind: 'shell_command' }))).toBe('shellApprovalTitle')
    expect(actionTitle(action({ kind: 'shell_command', title: 'Run the deploy' }))).toBe(
      'Run the deploy'
    )
  })

  it('localizes the daemon default message but keeps a custom one', () => {
    const message = 'The agent wants to run a local shell command.'
    expect(actionMessage(action({ kind: 'shell_command', message }))).toBe('shellApprovalMessage')
    expect(actionMessage(action({ kind: 'shell_command', message: 'Deploy to prod?' }))).toBe(
      'Deploy to prod?'
    )
  })

  it('names the tool, preferring the shell label', () => {
    expect(actionToolLabel(action({ kind: 'shell_command' }))).toBe('shellCommandTool')
    expect(actionToolLabel(action({ tool: { name: 'read_file', label: 'Read file' } }))).toBe(
      'Read file'
    )
    expect(actionToolLabel(action({ tool: { name: 'read_file' } }))).toBe('read_file')
    expect(actionToolLabel(action())).toBe('actionToolFallback')
  })

  it('builds a kind label per action shape', () => {
    expect(actionKindLabel(action({ kind: 'tool_approval', tool: { name: 'read_file' } }))).toBe(
      'actionApprovalKindLabel:read_file'
    )
    expect(actionKindLabel(action({ kind: 'choice' }))).toBe('actionChoiceKindLabel')
    expect(actionKindLabel(action({ title: 'Pick a branch' }))).toBe('Pick a branch')
    expect(actionKindLabel(action())).toBe('actionFallbackTitle')
  })

  it('maps every status, falling through to the raw value', () => {
    expect(actionStatusLabel(action({ status: 'pending' }))).toBe('actionStatusPending')
    expect(actionStatusLabel(action({ status: 'approved' }))).toBe('actionStatusApproved')
    expect(actionStatusLabel(action({ status: 'denied' }))).toBe('actionStatusDenied')
    expect(actionStatusLabel(action({ status: 'selected' }))).toBe('actionStatusSelected')
    expect(actionStatusLabel(action({ status: 'expired' }))).toBe('actionStatusExpired')
    expect(
      actionStatusLabel(action({ status: 'selected', response: { auto_selected: true } }))
    ).toBe('actionStatusAutoSelected')
    expect(
      actionStatusLabel(action({ status: 'expired', response: { answered_in_chat: true } }))
    ).toBe('actionStatusAnsweredInChat')
    expect(actionStatusLabel(action({ status: 'queued' }))).toBe('queued')
    expect(actionStatusLabel(action({ status: '' }))).toBe('actionStatusUnknown')
  })

  it('keeps custom approve and deny labels but localizes the defaults', () => {
    expect(actionApproveLabel(action({ approval: { approveLabel: 'Approve' } }))).toBe(
      'actionApprove'
    )
    expect(actionApproveLabel(action({ approval: { approveLabel: 'Ship it' } }))).toBe('Ship it')
    expect(actionDenyLabel(action({ approval: { denyLabel: 'Deny' } }))).toBe('actionDeny')
    expect(actionDenyLabel(action({ approval: { denyLabel: 'Stop' } }))).toBe('Stop')
  })

  it('offers always allow only on cards that can remember an approval', () => {
    expect(actionRememberLabel(action({ approval: { approveLabel: 'Approve' } }))).toBe('')
    expect(actionRememberLabel(action({ approval: { rememberLabel: 'Always allow' } }))).toBe(
      'actionAlwaysAllow'
    )
    expect(actionRememberLabel(action({ approval: { rememberLabel: 'Trust it' } }))).toBe(
      'Trust it'
    )
  })

  it('localizes MCP tool cards', () => {
    expect(actionTitle(action({ title: 'Run MCP tool: docs · search' }))).toBe(
      'mcpToolApprovalTitle:docs · search'
    )
    expect(actionDetailLabel({ label: 'Arguments', value: '{}', format: 'code' })).toBe(
      'actionDetailArguments'
    )
  })
})

describe('action choices', () => {
  const choices = [
    { id: 'c1', label: 'Main', value: 'main' },
    { id: 'c2', label: 'Other', input: { required: true, placeholder: 'Branch' } }
  ]

  it('reads the selected choice id from the response payload', () => {
    expect(actionChoiceId(action({ response: { choice_id: 'c1' } }))).toBe('c1')
    expect(actionChoiceId(action({ response: ['c1'] }))).toBe('')
    expect(actionChoiceId(action({ response: 'c1' }))).toBe('')
    expect(actionChoiceId(action())).toBe('')
  })

  it('finds the selected choice only once the action is answered', () => {
    expect(
      actionSelectedChoice(action({ status: 'selected', response: { choice_id: 'c1' }, choices }))
    ).toBe(choices[0])
    expect(
      actionSelectedChoice(action({ status: 'selected', response: { choice_id: 'gone' }, choices }))
    ).toBeUndefined()
    expect(actionSelectedChoice(action({ response: { choice_id: 'c1' }, choices }))).toBeUndefined()
  })

  it('reads the default choice only when it names one of the choices', () => {
    expect(actionDefaultChoiceId(action({ choices, payload: { default_choice_id: 'c1' } }))).toBe(
      'c1'
    )
    expect(actionDefaultChoiceId(action({ choices, payload: { default_choice_id: 'gone' } }))).toBe(
      ''
    )
    expect(actionDefaultChoiceId(action({ choices }))).toBe('')
  })

  it('returns typed-in text but not the choice echoed back', () => {
    expect(actionChoiceText(action({ response: { choice_text: 'release/1.2' } }))).toBe(
      'release/1.2'
    )
    expect(
      actionChoiceText(action({ response: { choice_id: 'c2', value: 'release/1.2' }, choices }))
    ).toBe('release/1.2')
    expect(
      actionChoiceText(action({ response: { choice_id: 'c1', value: 'main' }, choices }))
    ).toBe('')
    expect(actionChoiceText(action())).toBe('')
  })

  it('keys per-choice input by action and choice', () => {
    expect(actionChoiceInputKey(action({ id: 'a-9' }), 'c2')).toBe('a-9:c2')
  })

  it('reads the input requirements of a choice', () => {
    expect(choiceInputRequired(choices[1])).toBe(true)
    expect(choiceInputRequired(choices[0])).toBe(false)
    expect(choiceInputPlaceholder(choices[1])).toBe('Branch')
    expect(choiceInputPlaceholder(choices[0])).toBe('actionChoiceInputPlaceholder')
  })
})

describe('countdownLabel', () => {
  it('formats the time left as m:ss, rounding up', () => {
    expect(countdownLabel(200_000, 20_000)).toBe('3:00')
    expect(countdownLabel(65_500, 0)).toBe('1:06')
    expect(countdownLabel(9_000, 0)).toBe('0:09')
  })

  it('is empty without a deadline or once it has passed', () => {
    expect(countdownLabel(undefined, 0)).toBe('')
    expect(countdownLabel(1_000, 1_000)).toBe('')
    expect(countdownLabel(1_000, 5_000)).toBe('')
  })
})

describe('action details', () => {
  function detail(overrides: Partial<ChatActionDetail> = {}): ChatActionDetail {
    return { label: 'Command', value: 'ls -la', ...overrides }
  }

  it('localizes the daemon detail labels and passes others through', () => {
    expect(actionDetailLabel(detail({ label: 'Command' }))).toBe('actionDetailCommand')
    expect(actionDetailLabel(detail({ label: 'Workspace' }))).toBe('actionDetailWorkspace')
    expect(actionDetailLabel(detail({ label: 'Approval reason' }))).toBe(
      'actionDetailApprovalReason'
    )
    expect(actionDetailLabel(detail({ label: 'Environment keys' }))).toBe(
      'actionDetailEnvironmentKeys'
    )
    expect(actionDetailLabel(detail({ label: 'Retries' }))).toBe('Retries')
  })

  it('localizes the two known Mode values only', () => {
    expect(actionDetailText(detail({ label: 'Mode', value: 'background' }))).toBe(
      'actionBackground'
    )
    expect(actionDetailText(detail({ label: 'Mode', value: 'foreground' }))).toBe(
      'actionForeground'
    )
    expect(actionDetailText(detail({ label: 'Mode', value: 'detached' }))).toBe('detached')
  })

  it('renders non-string values as pretty JSON, and null as empty', () => {
    expect(actionDetailText(detail({ value: { a: 1 } }))).toBe('{\n  "a": 1\n}')
    expect(actionDetailText(detail({ value: null }))).toBe('')
    expect(actionDetailText(detail({ value: 'ls -la' }))).toBe('ls -la')
  })

  it('renders code, json, and list details as blocks', () => {
    expect(actionDetailIsBlock(detail({ format: 'code' }))).toBe(true)
    expect(actionDetailIsBlock(detail({ format: 'json' }))).toBe(true)
    expect(actionDetailIsBlock(detail({ format: 'list' }))).toBe(true)
    expect(actionDetailIsBlock(detail({ format: 'text' }))).toBe(false)
    expect(actionDetailIsBlock(detail())).toBe(false)
  })
})

describe('action links', () => {
  it('opens only http(s) links', () => {
    expect(
      actionDetailUrl({ label: 'Link', value: 'https://example.com/a?b=1', format: 'url' })
    ).toBe('https://example.com/a?b=1')
    expect(actionDetailUrl({ label: 'Link', value: 'https://example.com', format: 'text' })).toBe(
      undefined
    )
    expect(safeLink('javascript:alert(1)')).toBe(undefined)
    expect(safeLink('file:///etc/passwd')).toBe(undefined)
    expect(safeLink('not a url')).toBe(undefined)
    expect(safeLink(null)).toBe(undefined)
  })
})
