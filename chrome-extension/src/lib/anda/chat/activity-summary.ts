import { getMessage, uiLocale } from '$lib/i18n'
import type { ActivityDetail } from './message-display'
import { firstLine, isRecord, parseJsonString, toolCallStatus, toolKind } from './tool-view'

type Notice = Extract<ActivityDetail, { kind: 'notice' }>

export function noticeName(notice: Notice): string {
  return notice.name === 'background shell'
    ? getMessage('backgroundShellNotice')
    : notice.name || getMessage('runtimeNotice')
}

/** Background updates repeat their command's output; other notices lead with their point. */
export function noticeSummary(notice: Notice): string {
  return notice.name.startsWith('background ') ? '' : firstLine(notice.text)
}

/**
 * What a run of work did, in the reader's words: "Ran 3 commands, read 2
 * files and updated the plan" instead of a bare count of tool calls.
 */
export interface ActivityCounts {
  commands: number
  /** Distinct files successfully read by read_file. */
  filesRead: number
  filesEdited: number
  memory: number
  web: number
  /** Subagents, skills and every other tool. */
  tools: number
  /** The plan shows separately; its updates are named, not counted. */
  plan: boolean
}

export function activityCounts(details: ActivityDetail[]): ActivityCounts {
  const counts: ActivityCounts = {
    commands: 0,
    filesRead: 0,
    filesEdited: 0,
    memory: 0,
    web: 0,
    tools: 0,
    plan: false
  }
  const read = new Set<string>()
  const edited = new Set<string>()
  for (const detail of details) {
    if (detail.kind !== 'tool') continue
    const { tool } = detail
    const name = tool.name.toLowerCase()
    switch (name) {
      case 'shell':
        counts.commands++
        break
      case 'update_plan':
        if (toolCallStatus(tool) === 'ok') counts.plan = true
        else counts.tools++
        break
      case 'read_file':
      case 'edit_file':
      case 'write_file': {
        const args = parseJsonString(tool.args)
        if (
          toolCallStatus(tool) === 'ok' &&
          isRecord(args) &&
          typeof args.path === 'string' &&
          args.path
        ) {
          const paths = name === 'read_file' ? read : edited
          paths.add(args.path)
        } else {
          counts.tools++
        }
        break
      }
      default: {
        const kind = toolKind(name)
        if (kind === 'memory') counts.memory++
        else if (kind === 'web') counts.web++
        else counts.tools++
      }
    }
  }
  counts.filesRead = read.size
  counts.filesEdited = edited.size
  return counts
}

/** The counts as one phrase, or '' when the run called no tools. */
export function activityLabel(counts: ActivityCounts): string {
  const { commands, filesRead, filesEdited, memory, web, tools } = counts
  const parts: string[] = []
  if (commands) {
    parts.push(
      getMessage(commands === 1 ? 'activityCommandOne' : 'activityCommands', String(commands))
    )
  }
  if (filesRead) {
    parts.push(
      getMessage(filesRead === 1 ? 'activityFileReadOne' : 'activityFilesRead', String(filesRead))
    )
  }
  if (filesEdited) {
    parts.push(
      getMessage(
        filesEdited === 1 ? 'activityFileEditedOne' : 'activityFilesEdited',
        String(filesEdited)
      )
    )
  }
  if (memory) {
    parts.push(getMessage(memory === 1 ? 'activityMemoryOne' : 'activityMemory', String(memory)))
  }
  if (web) {
    parts.push(getMessage(web === 1 ? 'activityWebOne' : 'activityWeb', String(web)))
  }
  if (tools) {
    parts.push(getMessage(tools === 1 ? 'activityToolOne' : 'activityTools', String(tools)))
  }
  if (counts.plan) parts.push(getMessage('activityPlanUpdated'))
  const phrases = parts.filter(Boolean)
  if (!phrases.length) return ''
  const locale = uiLocale()
  const text = new Intl.ListFormat(locale, { type: 'conjunction' }).format(phrases)
  return text.charAt(0).toLocaleUpperCase(locale) + text.slice(1)
}
