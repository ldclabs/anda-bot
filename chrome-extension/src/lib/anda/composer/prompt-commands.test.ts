import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import {
  buildPromptCommandSuggestions,
  readPromptCommandContext,
  type PromptCommandContext
} from './prompt-commands'

beforeEach(() => {
  vi.stubGlobal('chrome', {
    i18n: {
      getMessage: vi.fn((key: string) => key)
    }
  })
})

afterEach(() => {
  vi.restoreAllMocks()
  vi.unstubAllGlobals()
})

describe('readPromptCommandContext', () => {
  it('opens skill completion for dollar-prefixed skill prompts', () => {
    expect(readPromptCommandContext('$front', '$front'.length)).toEqual({
      open: true,
      mode: 'skill',
      query: 'front',
      replaceStart: 1,
      replaceEnd: 6,
      key: 'skill:front:1:6'
    })
  })

  it('closes dollar skill completion once the prompt body starts', () => {
    expect(readPromptCommandContext('$frontend-design polish this', 17)).toEqual({
      open: false,
      mode: 'command',
      query: '',
      replaceStart: 0,
      replaceEnd: 0,
      key: 'closed'
    })
  })
})

function commandContext(query: string): PromptCommandContext {
  return {
    open: true,
    mode: 'command',
    query,
    replaceStart: 0,
    replaceEnd: query.length + 1,
    key: `command:${query}:0:${query.length + 1}`
  }
}

describe('buildPromptCommandSuggestions', () => {
  it('matches commands anywhere in the name with prefix matches first', () => {
    const labels = buildPromptCommandSuggestions(commandContext('an'), [], false, '').map(
      (suggestion) => suggestion.label
    )

    // /cancel matches by substring only; no prefix command matches "an".
    expect(labels).toContain('/cancel')
    expect(labels).not.toContain('/steer')

    const stLabels = buildPromptCommandSuggestions(commandContext('e'), [], false, '').map(
      (suggestion) => suggestion.label
    )
    // Substring matches: /new, /steer, /cancel … none starts with "e".
    expect(stLabels).toContain('/new')
    expect(stLabels).toContain('/steer')
  })

  it('ranks alias prefix matches at the top', () => {
    const suggestions = buildPromptCommandSuggestions(commandContext('clear'), [], false, '')
    expect(suggestions[0]).toMatchObject({ label: '/new', detail: 'alias: /clear' })
  })

  it('lists loop as its own command instead of a goal alias', () => {
    const suggestions = buildPromptCommandSuggestions(commandContext('loop'), [], false, '')
    expect(suggestions[0]).toMatchObject({
      label: '/loop',
      description: expect.stringContaining('self-pace')
    })
    expect(suggestions.find((suggestion) => suggestion.label === '/goal')).toBeUndefined()
  })

  it('surfaces matching skills in slash completion with the dollar shorthand', () => {
    const suggestions = buildPromptCommandSuggestions(
      commandContext('ste'),
      [
        { name: 'steel-thread', description: 'Plan a steel thread' },
        { name: 'frontend-design', description: 'Frontend design skill' }
      ],
      false,
      ''
    )

    // Command prefix first, then skill name prefix; unrelated skills hidden.
    expect(suggestions.map((suggestion) => suggestion.label)).toEqual(['/steer', '$steel-thread'])
    expect(suggestions[1]).toMatchObject({
      insertText: '$steel-thread ',
      kind: 'skill'
    })
  })

  it('lists commands before skills when the query is empty', () => {
    const suggestions = buildPromptCommandSuggestions(
      commandContext(''),
      [{ name: 'frontend-design', description: 'Frontend design skill' }],
      false,
      ''
    )
    expect(suggestions[0]?.kind).toBe('command')
    expect(suggestions.at(-1)).toMatchObject({ label: '$frontend-design', kind: 'skill' })
  })

  it('prefers skill name prefix over substring and description matches', () => {
    const context: PromptCommandContext = {
      open: true,
      mode: 'skill',
      query: 'design',
      replaceStart: 1,
      replaceEnd: 7,
      key: 'skill:design:1:7'
    }

    const labels = buildPromptCommandSuggestions(
      context,
      [
        { name: 'writing', description: 'Polish design documents' },
        { name: 'frontend-design', description: 'Frontend design skill' },
        { name: 'design-review', description: 'Review designs' }
      ],
      false,
      ''
    ).map((suggestion) => suggestion.label)

    expect(labels).toEqual(['design-review', 'frontend-design', 'writing'])
  })

  it('inserts only the skill token for dollar-prefixed skill prompts', () => {
    const context: PromptCommandContext = {
      open: true,
      mode: 'skill',
      query: 'front',
      replaceStart: 1,
      replaceEnd: 6,
      key: 'skill:front:1:6'
    }

    expect(
      buildPromptCommandSuggestions(
        context,
        [{ name: 'frontend-design', description: 'Frontend design skill' }],
        false,
        ''
      )[0]
    ).toMatchObject({
      label: 'frontend-design',
      insertText: 'frontend-design ',
      kind: 'skill'
    })
  })
})
