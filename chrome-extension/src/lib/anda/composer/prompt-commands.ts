import { getMessage } from '$lib/i18n'
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

// Built fresh on each call so descriptions and details follow the active UI
// language (the launcher can switch it after this module loads).
function promptCommandItems(): PromptCommandSuggestion[] {
  return [
    {
      id: 'command:new',
      label: '/new',
      insertText: '/new ',
      description: getMessage('promptCommandNewDescription'),
      detail: getMessage('promptCommandAliasDetail', '/clear'),
      kind: 'command'
    },
    {
      id: 'command:goal',
      label: '/goal',
      insertText: '/goal ',
      description: getMessage('promptCommandGoalDescription'),
      kind: 'command'
    },
    {
      id: 'command:loop',
      label: '/loop',
      insertText: '/loop ',
      description: getMessage('promptCommandLoopDescription'),
      kind: 'command'
    },
    {
      id: 'command:side',
      label: '/side',
      insertText: '/side ',
      description: getMessage('promptCommandSideDescription'),
      detail: getMessage('promptCommandAliasDetail', '/btw'),
      kind: 'command'
    },
    {
      id: 'command:steer',
      label: '/steer',
      insertText: '/steer ',
      description: getMessage('promptCommandSteerDescription'),
      kind: 'command'
    },
    {
      id: 'command:skill',
      label: '/skill',
      insertText: '/skill ',
      description: getMessage('promptCommandSkillDescription'),
      detail: getMessage('promptCommandShortcutDetail', '$skill-name'),
      kind: 'command'
    },
    {
      id: 'command:stop',
      label: '/stop',
      insertText: '/stop ',
      description: getMessage('promptCommandStopDescription'),
      kind: 'command'
    },
    {
      id: 'command:cancel',
      label: '/cancel',
      insertText: '/cancel ',
      description: getMessage('promptCommandCancelDescription'),
      kind: 'command'
    },
    {
      id: 'command:ping',
      label: '/ping',
      insertText: '/ping ',
      description: getMessage('promptCommandPingDescription'),
      kind: 'command'
    }
  ]
}

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

const maxSkillSuggestions = 20

// Tier of a command for the given query; lower sorts first, -1 means no match.
// Prefix matches (label or alias) outrank anywhere-in-the-name matches.
function commandMatchTier(item: PromptCommandSuggestion, query: string): number {
  if (!query) {
    return 0
  }
  const label = item.label.slice(1).toLowerCase()
  const aliases = (item.detail?.toLowerCase().match(/\/[a-z0-9_-]+/g) || []).map((alias) =>
    alias.slice(1)
  )
  if (label.startsWith(query) || aliases.some((alias) => alias.startsWith(query))) {
    return 0
  }
  if (label.includes(query) || aliases.some((alias) => alias.includes(query))) {
    return 2
  }
  return -1
}

// Rank of a skill for the given query: 0 name prefix, 1 name substring,
// 2 description substring, -1 no match.
function skillMatchRank(skill: PromptSkill, query: string): number {
  if (!query) {
    return 0
  }
  const name = skill.name.toLowerCase()
  if (name.startsWith(query)) {
    return 0
  }
  if (name.includes(query)) {
    return 1
  }
  if ((skill.description || '').toLowerCase().includes(query)) {
    return 2
  }
  return -1
}

function skillSuggestion(skill: PromptSkill, insertPrefix: string): PromptCommandSuggestion {
  return {
    id: `skill:${skill.name}`,
    label: `${insertPrefix}${skill.name}`,
    insertText: `${insertPrefix}${skill.name} `,
    description: skill.description || getMessage('promptSkillDescription'),
    detail: '/skill',
    kind: 'skill'
  }
}

// `/` mode mixes commands and skills. Tier order: command prefix (0), skill
// name prefix (1), command substring (2), skill name substring (3), skill
// description substring (4).
function buildCommandModeSuggestions(
  query: string,
  skills: PromptSkill[],
  skillsLoading: boolean
): PromptCommandSuggestion[] {
  const entries: Array<{ suggestion: PromptCommandSuggestion; tier: number }> = []
  for (const item of promptCommandItems()) {
    const tier = commandMatchTier(item, query)
    if (tier >= 0) {
      entries.push({ suggestion: item, tier })
    }
  }

  const skillTierByRank = [1, 3, 4]
  let skillCount = 0
  for (const skill of skills) {
    if (skillCount >= maxSkillSuggestions) {
      break
    }
    const rank = skillMatchRank(skill, query)
    if (rank < 0) {
      continue
    }
    skillCount += 1
    // The replace range spans the whole `/token`, so insert the `$name`
    // shorthand to produce a valid skill invocation.
    entries.push({ suggestion: skillSuggestion(skill, '$'), tier: skillTierByRank[rank]! })
  }

  if (!entries.length) {
    if (skillsLoading && skills.length === 0) {
      return [promptCommandStatus('skills-loading', getMessage('promptSkillsLoading'))]
    }
    return [promptCommandStatus('commands-empty', getMessage('promptCommandsEmpty'))]
  }

  entries.sort((a, b) => a.tier - b.tier)
  return entries.map((entry) => entry.suggestion)
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
    return buildCommandModeSuggestions(query, skills, skillsLoading)
  }

  if (skillsLoading && skills.length === 0) {
    return [promptCommandStatus('skills-loading', getMessage('promptSkillsLoading'))]
  }
  if (skillsError) {
    return [promptCommandStatus('skills-error', skillsError)]
  }

  const ranked = skills
    .map((skill, index) => ({ skill, rank: skillMatchRank(skill, query), index }))
    .filter((entry) => entry.rank >= 0)
  ranked.sort((a, b) => a.rank - b.rank || a.index - b.index)
  const matches = ranked.slice(0, maxSkillSuggestions)
  return matches.length
    ? matches.map((entry) => skillSuggestion(entry.skill, ''))
    : [promptCommandStatus('skills-empty', getMessage('promptSkillsEmpty'))]
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
