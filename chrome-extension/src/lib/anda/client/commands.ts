export type NewPromptCommand = {
	prompt: string | null
}

export function parseNewPromptCommand(prompt: string): NewPromptCommand | null {
	const trimmed = prompt.trim()
	if (!trimmed.startsWith('/')) {
		return null
	}

	const body = trimmed.slice(1)
	const commandEnd = body.search(/\s/)
	const command = (commandEnd === -1 ? body : body.slice(0, commandEnd)).toLowerCase()
	if (command !== 'new' && command !== 'clear') {
		return null
	}

	const rest = commandEnd === -1 ? '' : body.slice(commandEnd).trim()
	return { prompt: rest || null }
}
