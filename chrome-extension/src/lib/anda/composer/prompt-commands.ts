import type { PromptSkill } from '../client/types'

export type PromptCommandContext = {
  open: boolean
  mode: 'command' | 'skill'
  query: string
  replaceStart: number
  replaceEnd: number
  key: string
}

export type PromptCommandSuggestion = {
  id: string
  label: string
  insertText: string
  description: string
  detail?: string
  disabled?: boolean
  kind: 'command' | 'skill' | 'status'
}

export const emptyPromptCommandContext: PromptCommandContext = {
  open: false,
  mode: 'command',
  query: '',
  replaceStart: 0,
  replaceEnd: 0,
  key: 'closed'
}

export const promptSkillsCacheMs = 60_000

const promptCommandItems: PromptCommandSuggestion[] = [
  {
    id: 'command:new',
    label: '/new',
    insertText: '/new ',
    description: 'Start a fresh conversation with an optional first prompt.',
    detail: 'alias: /clear',
    kind: 'command'
  },
  {
    id: 'command:goal',
    label: '/goal',
    insertText: '/goal ',
    description: 'Start a supervised long-running task.',
    detail: 'alias: /loop',
    kind: 'command'
  },
  {
    id: 'command:side',
    label: '/side',
    insertText: '/side ',
    description: 'Run a temporary side request in a subagent.',
    detail: 'alias: /btw',
    kind: 'command'
  },
  {
    id: 'command:steer',
    label: '/steer',
    insertText: '/steer ',
    description: 'Redirect the next model step with a new instruction.',
    kind: 'command'
  },
  {
    id: 'command:skill',
    label: '/skill',
    insertText: '/skill ',
    description: 'Route the prompt to a named skill subagent.',
    detail: 'shortcut: $skill-name',
    kind: 'command'
  },
  {
    id: 'command:stop',
    label: '/stop',
    insertText: '/stop ',
    description: 'Cancel the current task with an optional reason.',
    detail: 'alias: /cancel',
    kind: 'command'
  },
  {
    id: 'command:cancel',
    label: '/cancel',
    insertText: '/cancel ',
    description: 'Cancel the current task with an optional reason.',
    detail: 'alias: /stop',
    kind: 'command'
  },
  {
    id: 'command:ping',
    label: '/ping',
    insertText: '/ping ',
    description: 'Send a lightweight ping.',
    kind: 'command'
  }
]

export function readPromptCommandContext(value: string, caret: number): PromptCommandContext {
  const safeCaret = Math.max(0, Math.min(caret, value.length))
  const firstLineBreak = value.indexOf('\n')
  const commandLineEnd = firstLineBreak === -1 ? value.length : firstLineBreak
  if (safeCaret > commandLineEnd) {
    return emptyPromptCommandContext
  }

  const commandLine = value.slice(0, commandLineEnd)
  const leadingWhitespace = commandLine.match(/^\s*/)?.[0] || ''
  const slashIndex = leadingWhitespace.length
  if (commandLine[slashIndex] === '$') {
    return readDollarSkillContext(commandLine, safeCaret, slashIndex)
  }
  if (commandLine[slashIndex] !== '/' || safeCaret < slashIndex + 1) {
    return emptyPromptCommandContext
  }

  const commandBody = commandLine.slice(slashIndex + 1)
  const commandToken = commandBody.match(/^\S*/)?.[0] || ''
  const commandTokenEnd = slashIndex + 1 + commandToken.length
  const commandName = commandToken.toLowerCase()

  if (commandName === 'skill' && safeCaret >= commandTokenEnd) {
    const afterCommand = commandLine.slice(commandTokenEnd)
    const spacesAfterCommand = afterCommand.match(/^\s+/)?.[0] || ''
    if (spacesAfterCommand.length > 0) {
      const skillStart = commandTokenEnd + spacesAfterCommand.length
      const skillToken = commandLine.slice(skillStart).match(/^\S*/)?.[0] || ''
      const skillEnd = skillStart + skillToken.length
      if (safeCaret >= skillStart && safeCaret <= skillEnd) {
        const query = commandLine.slice(skillStart, safeCaret)
        return {
          open: true,
          mode: 'skill',
          query,
          replaceStart: skillStart,
          replaceEnd: skillEnd,
          key: `skill:${query}:${skillStart}:${skillEnd}`
        }
      }
      return emptyPromptCommandContext
    }
  }

  if (safeCaret > commandTokenEnd) {
    return emptyPromptCommandContext
  }

  let replaceEnd = commandTokenEnd
  while (replaceEnd < commandLineEnd && /[ \t]/.test(value[replaceEnd])) {
    replaceEnd += 1
  }
  const query = commandLine.slice(slashIndex + 1, Math.min(safeCaret, commandTokenEnd))
  return {
    open: true,
    mode: 'command',
    query,
    replaceStart: slashIndex,
    replaceEnd,
    key: `command:${query}:${slashIndex}:${replaceEnd}`
  }
}

function readDollarSkillContext(
  commandLine: string,
  safeCaret: number,
  dollarIndex: number
): PromptCommandContext {
  if (safeCaret < dollarIndex + 1) {
    return emptyPromptCommandContext
  }

  const skillStart = dollarIndex + 1
  const skillToken = commandLine.slice(skillStart).match(/^\S*/)?.[0] || ''
  const skillEnd = skillStart + skillToken.length
  if (safeCaret > skillEnd) {
    return emptyPromptCommandContext
  }

  const query = commandLine.slice(skillStart, safeCaret)
  return {
    open: true,
    mode: 'skill',
    query,
    replaceStart: skillStart,
    replaceEnd: skillEnd,
    key: `skill:${query}:${skillStart}:${skillEnd}`
  }
}

export function buildPromptCommandSuggestions(
  context: PromptCommandContext,
  skills: PromptSkill[],
  skillsLoading: boolean,
  skillsError: string
): PromptCommandSuggestion[] {
  if (!context.open) {
    return []
  }

  const query = context.query.trim().toLowerCase()
  if (context.mode === 'command') {
    const matches = promptCommandItems.filter((item) => {
      const label = item.label.slice(1).toLowerCase()
      const detail = item.detail?.toLowerCase() || ''
      return !query || label.startsWith(query) || detail.includes(`/${query}`)
    })
    return matches.length
      ? matches
      : [promptCommandStatus('commands-empty', chrome.i18n.getMessage('promptCommandsEmpty'))]
  }

  if (skillsLoading && skills.length === 0) {
    return [promptCommandStatus('skills-loading', chrome.i18n.getMessage('promptSkillsLoading'))]
  }
  if (skillsError) {
    return [promptCommandStatus('skills-error', skillsError)]
  }

  const matches = skills
    .filter((skill) => {
      const name = skill.name.toLowerCase()
      const description = skill.description?.toLowerCase() || ''
      return !query || name.includes(query) || description.includes(query)
    })
    .slice(0, 20)
  return matches.length
    ? matches.map((skill) => ({
        id: `skill:${skill.name}`,
        label: skill.name,
        insertText: `${skill.name} `,
        description: skill.description || chrome.i18n.getMessage('promptSkillDescription'),
        detail: '/skill',
        kind: 'skill'
      }))
    : [promptCommandStatus('skills-empty', chrome.i18n.getMessage('promptSkillsEmpty'))]
}

function promptCommandStatus(id: string, description: string): PromptCommandSuggestion {
  return {
    id,
    label: '',
    insertText: '',
    description,
    kind: 'status',
    disabled: true
  }
}

export function firstEnabledPromptCommandIndex(suggestions: PromptCommandSuggestion[]): number {
  const index = suggestions.findIndex((suggestion) => !suggestion.disabled)
  return index === -1 ? 0 : index
}
