import { errorToError } from './settings'
import type { ChromeApi } from './types'

export function chromeTtsAvailable(chromeApi: ChromeApi): boolean {
	return Boolean(chromeApi.tts?.speak)
}

export async function speakWithChromeTts(chromeApi: ChromeApi, text: string): Promise<void> {
	const utterance = text.trim()
	if (!utterance) {
		return
	}
	const tts = chromeApi.tts
	if (!tts?.speak) {
		throw new Error('Chrome TTS is unavailable')
	}

	tts.stop?.()
	await new Promise<void>((resolve, reject) => {
		let settled = false
		let timeout: ReturnType<typeof setTimeout>
		const settle = (ok: boolean, error?: Error) => {
			if (settled) {
				return
			}
			settled = true
			clearTimeout(timeout)
			if (ok) {
				resolve()
			} else {
				reject(error || new Error('Chrome TTS failed'))
			}
		}
		timeout = setTimeout(() => settle(true), chromeTtsTimeoutMs(utterance))

		try {
			tts.speak(
				utterance,
				{
					enqueue: false,
					rate: 1,
					pitch: 1,
					volume: 1,
					desiredEventTypes: ['end', 'error', 'interrupted', 'cancelled'],
					onEvent: (event) => {
						if (event.type === 'end') {
							settle(true)
						} else if (event.type === 'error') {
							settle(false, new Error(event.errorMessage || 'Chrome TTS failed'))
						} else if (event.type === 'interrupted' || event.type === 'cancelled') {
							settle(false, new Error(`Chrome TTS ${event.type}`))
						}
					}
				},
				() => {
					const error = chromeApi.runtime.lastError
					if (error?.message) {
						settle(false, new Error(error.message))
					}
				}
			)
		} catch (error) {
			settle(false, errorToError(error))
		}
	})
}

function chromeTtsTimeoutMs(text: string): number {
	return Math.min(120_000, Math.max(8_000, text.length * 180))
}
