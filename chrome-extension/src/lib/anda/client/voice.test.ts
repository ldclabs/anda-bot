import { afterEach, describe, expect, it, vi } from 'vitest'

import {
  isAudioResource,
  normalizeCapabilityFormats,
  normalizeVoiceRecordingAudio,
  normalTextForSpeech,
  playVoiceTtsPipeline,
  prepareVoiceTtsText,
  splitVoiceTtsText,
  voiceTtsChunkChars
} from './voice'
import { chromeSpeechErrorMessage, preferredRecordingMimeType } from '../composer/voice'

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

  it('adds an extension when the MIME type identifies an accepted recording', async () => {
    await expect(
      normalizeVoiceRecordingAudio(
        {
          audioBase64: 'Zm9v',
          fileName: 'speech',
          mimeType: 'audio/webm',
          ttsEnabled: false
        },
        ['webm']
      )
    ).resolves.toEqual({
      audioBase64: 'Zm9v',
      fileName: 'speech.webm'
    })
  })

  it('treats audio/mp4 and m4a as aliases for browser recordings', async () => {
    await expect(
      normalizeVoiceRecordingAudio(
        {
          audioBase64: 'Zm9v',
          fileName: 'speech.m4a',
          mimeType: 'audio/mp4',
          ttsEnabled: false
        },
        ['m4a']
      )
    ).resolves.toEqual({
      audioBase64: 'Zm9v',
      fileName: 'speech.m4a'
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

  it('rejects unknown audio formats before sending them to the daemon', async () => {
    vi.stubGlobal('chrome', {
      i18n: {
        getMessage: vi.fn((key: string, substitutions?: string[]) =>
          substitutions?.length ? `${key}:${substitutions.join(',')}` : key
        )
      }
    })

    await expect(
      normalizeVoiceRecordingAudio(
        {
          audioBase64: 'Zm9v',
          fileName: 'speech.bin',
          mimeType: 'application/octet-stream',
          ttsEnabled: false
        },
        ['wav']
      )
    ).rejects.toThrow('audioFormatNotSupported:unknown,wav')
  })
})

describe('preferredRecordingMimeType', () => {
  it('uses the browser mp4 recorder for m4a-compatible providers', () => {
    vi.stubGlobal('MediaRecorder', {
      isTypeSupported: vi.fn((type: string) => type === 'audio/mp4')
    })

    expect(preferredRecordingMimeType(['m4a'])).toBe('audio/mp4')
  })
})

describe('chromeSpeechErrorMessage', () => {
  it('uses browser-neutral wording for user-visible speech errors', () => {
    expect(chromeSpeechErrorMessage('permission was not accepted')).toBe(
      'Browser speech permission was not accepted.'
    )
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

  it('prefers shorter sentence chunks for long spoken replies', () => {
    const sentence = '这是一句用于语音播报的较长内容，包含足够上下文，也适合在这里自然停顿。'
    const chunks = splitVoiceTtsText(sentence.repeat(10), voiceTtsChunkChars)

    expect(chunks.length).toBeGreaterThan(1)
    expect(chunks.every((chunk) => Array.from(chunk).length <= voiceTtsChunkChars)).toBe(true)
    expect(Array.from(chunks[0]).length).toBeLessThanOrEqual(180)
  })
})

describe('playVoiceTtsPipeline', () => {
  it('starts synthesizing the next segment while the current segment is playing', async () => {
    const events: string[] = []
    let finishFirstPlay: (() => void) | undefined
    const synthesize = vi.fn(async (_chunk: string, index: number) => {
      events.push(`synthesize:${index}`)
      return `artifact:${index}`
    })
    const play = vi.fn((artifact: string, index: number) => {
      events.push(`play-start:${index}:${artifact}`)
      if (index === 0) {
        return new Promise<void>((resolve) => {
          finishFirstPlay = () => {
            events.push('play-end:0')
            resolve()
          }
        })
      }
      events.push(`play-end:${index}`)
      return Promise.resolve()
    })

    const pipeline = playVoiceTtsPipeline(['first', 'second'], synthesize, play)
    await Promise.resolve()
    await Promise.resolve()
    await Promise.resolve()

    expect(events).toEqual(['synthesize:0', 'synthesize:1', 'play-start:0:artifact:0'])

    const finish = finishFirstPlay
    if (!finish) {
      throw new Error('first playback did not start')
    }
    finish()
    await pipeline

    expect(events).toEqual([
      'synthesize:0',
      'synthesize:1',
      'play-start:0:artifact:0',
      'play-end:0',
      'play-start:1:artifact:1',
      'play-end:1'
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
