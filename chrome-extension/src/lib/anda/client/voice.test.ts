import { afterEach, describe, expect, it, vi } from 'vitest'

import {
	isAudioResource,
	normalizeCapabilityFormats,
	normalizeVoiceRecordingAudio,
	normalTextForSpeech,
	prepareVoiceTtsText,
	splitVoiceTtsText
} from './voice'

afterEach(() => {
	vi.restoreAllMocks()
	vi.unstubAllGlobals()
})

describe('normalizeCapabilityFormats', () => {
	it('normalizes legacy boolean capabilities and de-duplicates formats', () => {
		expect(normalizeCapabilityFormats(true, ['audio/mp3', 'mp3', 'WAV'])).toEqual(['mp3', 'wav'])
		expect(normalizeCapabilityFormats(false, ['mp3'])).toEqual([])
	})
})

describe('normalizeVoiceRecordingAudio', () => {
	it('returns the original recording when its format is already accepted', async () => {
		await expect(
			normalizeVoiceRecordingAudio(
				{
					audioBase64: 'Zm9v',
					fileName: 'speech.mp3',
					mimeType: 'audio/mpeg',
					ttsEnabled: false
				},
				['mp3', 'wav']
			)
		).resolves.toEqual({
			audioBase64: 'Zm9v',
			fileName: 'speech.mp3'
		})
	})

	it('rejects missing audio data before attempting format conversion', async () => {
		vi.stubGlobal('chrome', {
			i18n: {
				getMessage: vi.fn((key: string) => key)
			}
		})

		await expect(
			normalizeVoiceRecordingAudio(
				{
					audioBase64: '   ',
					fileName: 'speech.webm',
					mimeType: 'audio/webm',
					ttsEnabled: false
				},
				['wav']
			)
		).rejects.toThrow('audioCaptureMissingData')
	})
})

describe('prepareVoiceTtsText', () => {
	it('preserves literal symbols after markdown is flattened to plain text', () => {
		expect(prepareVoiceTtsText('Use C# and snake_case with `quoted_name`.')).toBe(
			'Use C# and snake_case with quoted_name.'
		)
	})

	it('removes emoji and normalizes em dashes for speech', () => {
		expect(prepareVoiceTtsText('Hello🙂 — world')).toBe('Hello , world')
	})
})

describe('normalTextForSpeech', () => {
	it('strips legacy thinking tags from spoken text', () => {
		expect(normalTextForSpeech('Before<thinking>draft</thinking> after')).toBe('Before after')
	})
})

describe('splitVoiceTtsText', () => {
	it('splits long lines on sentence boundaries before the hard limit', () => {
		expect(splitVoiceTtsText('Alpha beta. Gamma delta? Final line.', 20)).toEqual([
			'Alpha beta.',
			'Gamma delta?',
			'Final line.'
		])
	})
})

describe('isAudioResource', () => {
	it('detects audio artifacts from the file extension when tags and mime type are missing', () => {
		expect(
			isAudioResource({
				name: 'speech.mp3',
				blob: 'Zm9v',
				tags: []
			})
		).toBe(true)
	})

	it('remains tolerant of malformed runtime payloads without tags', () => {
		expect(
			isAudioResource({
				name: 'speech.wav',
				blob: 'Zm9v'
			} as any)
		).toBe(true)
	})
})
