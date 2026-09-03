import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import {
  actionApproveLabel,
  actionChoiceId,
  actionChoiceInputKey,
  actionChoiceSelected,
  actionChoiceText,
  actionDenyLabel,
  actionDetailIsBlock,
  actionDetailLabel,
  actionDetailText,
  actionKindLabel,
  actionMessage,
  actionPending,
  actionResponseLabel,
  actionStatusLabel,
  actionTitle,
  actionToolLabel,
  choiceInputPlaceholder,
  choiceInputRequired,
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

  it('marks a choice selected only once the action is answered', () => {
    const answered = action({ status: 'selected', response: { choice_id: 'c1' } })
    expect(actionChoiceSelected(answered, 'c1')).toBe(true)
    expect(actionChoiceSelected(answered, 'c2')).toBe(false)
    expect(actionChoiceSelected(action({ response: { choice_id: 'c1' } }), 'c1')).toBe(false)
  })

  it('resolves the answered label, falling back to the id', () => {
    expect(
      actionResponseLabel(action({ status: 'selected', response: { choice_id: 'c1' }, choices }))
    ).toBe('Main')
    expect(
      actionResponseLabel(action({ status: 'selected', response: { choice_id: 'gone' }, choices }))
    ).toBe('gone')
    expect(actionResponseLabel(action({ response: { choice_id: 'c1' }, choices }))).toBe('')
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
