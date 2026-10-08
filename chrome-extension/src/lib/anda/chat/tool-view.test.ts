import { describe, expect, it } from 'vitest'
import {
  firstLine,
  runtimeNotices,
  toolCallStatus,
  toolCallSummary,
  toolDetailSections,
  toolKind
} from './tool-view'

describe('tool-view', () => {
  it('summarizes a call by its identifying argument', () => {
    expect(
      toolCallSummary({
        name: 'shell',
        args: { background: false, command: 'pwd &&\n  ls -la', cwd: '/repo' }
      })
    ).toBe('pwd && ls -la')
    expect(
      toolCallSummary({
        name: 'read_file',
        args: { limit: 0, path: '/Users/me/git/github.com/acme/verisit/docs/product-plan.md' }
      })
    ).toBe('…/verisit/docs/product-plan.md')
    expect(
      toolCallSummary({
        name: 'recall_memory',
        args: { budget: null, context: { agent: '$self' }, query: 'verisit history' }
      })
    ).toBe('verisit history')
    expect(toolCallSummary({ name: 'custom', args: '{"answer": "yes"}' })).toBe('yes')
    expect(toolCallSummary({ name: 'custom', args: { count: 3 } })).toBe('{"count":3}')
    expect(toolCallSummary({ name: 'shell', output: 'x' })).toBe('')
  })

  it('derives status from the result envelope', () => {
    expect(toolCallStatus({ name: 'shell', args: {} })).toBe('running')
    expect(toolCallStatus({ name: 'shell', output: { exit_code: 0, stdout: 'ok' } })).toBe('ok')
    expect(toolCallStatus({ name: 'shell', output: { exit_code: 2, stdout: '' } })).toBe('error')
    expect(toolCallStatus({ name: 'read_file', output: { error: 'denied' } })).toBe('error')
    expect(toolCallStatus({ name: 'mcp', output: '{"is_error": true}' })).toBe('error')
    expect(toolCallStatus({ name: 'shell', output: null })).toBe('ok')
  })

  it('shows shell streams instead of the result envelope', () => {
    expect(
      toolDetailSections({
        name: 'shell',
        args: { command: 'make test', cwd: '/repo', timeout_ms: 1000 },
        output: {
          exit_code: 1,
          state: 'exited',
          stdout: 'running\n',
          stderr: 'boom\n',
          process_id: 9,
          raw_output_path: '/tmp/x.log'
        }
      })
    ).toEqual([
      { kind: 'input', text: 'make test', meta: '/repo' },
      { kind: 'output', text: 'running', meta: 'exit 1' },
      { kind: 'stderr', text: 'boom', meta: undefined }
    ])
  })

  it('shows errors as text and other values as JSON', () => {
    expect(
      toolDetailSections({ name: 'read_file', args: { path: 'a' }, output: { error: 'denied' } })
    ).toEqual([
      { kind: 'input', text: '{\n  "path": "a"\n}' },
      { kind: 'output', text: 'denied' }
    ])
    expect(toolDetailSections({ name: 'recall', args: {}, output: '{"items":[]}' })).toEqual([
      { kind: 'output', text: '{\n  "items": []\n}' }
    ])
    expect(toolDetailSections({ name: 'shell', args: { command: 'ls' } })).toEqual([
      { kind: 'input', text: 'ls', meta: undefined }
    ])
  })

  it('splits runtime notices into kind and body', () => {
    const text =
      '[$system: kind="background shell"]\nThis message is from the Anda runtime.\n\n"done\\nexit 0"\n\n---\n\n[$system: kind="notice"]\nPreamble.\n\n"second"'
    expect(runtimeNotices(text)).toEqual([
      { kind: 'background shell', body: 'done\nexit 0' },
      { kind: 'notice', body: 'second' }
    ])
    expect(runtimeNotices('plain runtime text')).toEqual([{ kind: '', body: 'plain runtime text' }])
  })

  it('classifies tools and takes a readable first line', () => {
    expect(toolKind('shell')).toBe('shell')
    expect(toolKind('read_file')).toBe('file')
    expect(toolKind('recall_memory')).toBe('memory')
    expect(toolKind('web_fetch')).toBe('web')
    expect(toolKind('subagent')).toBe('agent')
    expect(toolKind('todo')).toBe('tool')
    expect(firstLine('## Plan\nstep one')).toBe('Plan')
  })
})
