export type PromptCommand = {
	kind: 'new' | 'side'
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
		default:
			return null
	}
}
