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

describe('buildPromptCommandSuggestions', () => {
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
