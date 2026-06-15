export type PromptCommand = {
  kind: 'new' | 'side' | 'steer' | 'stop' | 'cancel'
  prompt: string
}

export function parsePromptCommand(prompt: string): PromptCommand | null {
  const trimmed = prompt.trim()
  if (!trimmed.startsWith('/')) {
    return null
  }

  const body = trimmed.slice(1)
  const commandEnd = body.search(/\s/)
  const command = (commandEnd === -1 ? body : body.slice(0, commandEnd)).toLowerCase()

  const rest = commandEnd === -1 ? '' : body.slice(commandEnd).trim()
  switch (command) {
    case 'new':
    case 'clear':
      return { kind: 'new', prompt: rest ? trimmed : '' }
    case 'side':
    case 'btw':
      return { kind: 'side', prompt: rest ? trimmed : '' }
    case 'steer':
      return { kind: 'steer', prompt: trimmed }
    case 'stop':
      return { kind: 'stop', prompt: trimmed }
    case 'cancel':
      return { kind: 'cancel', prompt: trimmed }
    default:
      return null
  }
}

export function isImmediatePromptCommand(command: PromptCommand | null): boolean {
  return (
    command?.kind === 'new' ||
    command?.kind === 'steer' ||
    command?.kind === 'stop' ||
    command?.kind === 'cancel'
  )
}
