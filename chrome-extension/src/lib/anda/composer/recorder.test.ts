import { describe, expect, it, vi } from 'vitest'
import {
  VoiceRecorder,
  type LocalRecording,
  type PageCaptureBridge,
  type VoicePlatform
} from './recorder.svelte'
import type { VoiceCapabilities, VoiceRecordingInput } from '../client/types'
import type { BrowserSpeechRecognition } from './voice'

/** A SpeechRecognition stand-in whose events the test drives by hand. */
function fakeRecognition() {
  const recognition: BrowserSpeechRecognition & { started: number; aborted: boolean } = {
    lang: '',
    continuous: false,
    interimResults: false,
    onresult: null,
    onerror: null,
    onend: null,
    started: 0,
    aborted: false,
    start() {
      recognition.started += 1
    },
    stop() {
      recognition.onend?.()
    },
    abort() {
      recognition.aborted = true
    }
  }
  return recognition
}

function fakeRecording(blob: Blob | Promise<Blob>, mimeType = 'audio/webm') {
  const disposed = { value: false }
  const recording: LocalRecording & { disposed: typeof disposed } = {
    blob: Promise.resolve(blob),
    mimeType,
    stop: vi.fn(),
    dispose: vi.fn(() => {
      disposed.value = true
    }),
    disposed
  }
  return recording
}

function createPlatform(overrides: Partial<VoicePlatform> = {}): VoicePlatform {
  const createSpeechRecognition = overrides.createSpeechRecognition || (() => null)
  return {
    language: () => 'en-US',
    speechRecognitionSupported: () => Boolean(createSpeechRecognition()),
    createSpeechRecognition,
    startRecording: () => null,
    meter: () => () => undefined,
    toBase64: async () => 'AAA',
    now: () => 1700000000000,
    ...overrides
  }
}

function createRecorder(
  options: {
    capabilities?: Partial<VoiceCapabilities>
    page?: PageCaptureBridge
    platform?: Partial<VoicePlatform>
    sendError?: Error
  } = {}
) {
  const sent: VoiceRecordingInput[] = []
  const send = vi.fn(async (input: VoiceRecordingInput) => {
    if (options.sendError) {
      throw options.sendError
    }
    sent.push(input)
  })
  const recorder = new VoiceRecorder({
    capabilities: () => ({
      transcription: [],
      daemonTts: [],
      chromeTts: false,
      ...options.capabilities
    }),
    ttsEnabled: () => true,
    send,
    page: options.page ? () => options.page as PageCaptureBridge : undefined,
    platform: createPlatform(options.platform)
  })
  return { recorder, send, sent }
}

describe('VoiceRecorder path selection', () => {
  it('reports no usable service when nothing is available', async () => {
    const { recorder, send } = createRecorder()

    await recorder.start()

    expect(recorder.stage).toBe('idle')
    expect(recorder.error).toBe('Selected voice service is unavailable.')
    expect(send).not.toHaveBeenCalled()
  })

  it('falls back from Chrome speech to Anda audio when recognition will not start', async () => {
    const startAudio = vi.fn(async () => undefined)
    const { recorder } = createRecorder({
      capabilities: { transcription: ['wav'] },
      page: {
        startSpeech: vi.fn(async () => {
          throw new Error('mic blocked by policy')
        }),
        stopSpeech: vi.fn(async () => ''),
        startAudio,
        stopAudio: vi.fn(async () => ({}))
      }
    })
    recorder.provider = 'chrome'

    await recorder.start()

    expect(recorder.provider).toBe('anda')
    expect(startAudio).toHaveBeenCalled()
    expect(recorder.stage).toBe('recording')
  })

  it('does not retry locally when page capture was denied the microphone', async () => {
    const startRecording = vi.fn(() => null)
    const { recorder } = createRecorder({
      capabilities: { transcription: ['wav'] },
      page: {
        startAudio: vi.fn(async () => {
          throw new Error('Microphone access was blocked for the current page.')
        }),
        stopAudio: vi.fn(async () => ({}))
      },
      platform: { startRecording }
    })

    await recorder.start()

    expect(startRecording).not.toHaveBeenCalled()
    expect(recorder.stage).toBe('idle')
  })

  it('falls back to local capture for a non-permission page failure', async () => {
    const recording = fakeRecording(new Blob(['x']))
    const startRecording = vi.fn(() => Promise.resolve(recording))
    const { recorder } = createRecorder({
      capabilities: { transcription: ['wav'] },
      page: {
        startAudio: vi.fn(async () => {
          throw new Error('service worker asleep')
        }),
        stopAudio: vi.fn(async () => ({}))
      },
      platform: { startRecording }
    })

    await recorder.start()

    expect(startRecording).toHaveBeenCalled()
    expect(recorder.stage).toBe('recording')
  })
})

describe('VoiceRecorder page speech', () => {
  const page = (transcript: string): PageCaptureBridge => ({
    startSpeech: vi.fn(async () => undefined),
    stopSpeech: vi.fn(async () => transcript),
    cancelSpeech: vi.fn(async () => undefined)
  })

  it('sends the recognized transcript and returns to idle', async () => {
    const bridge = page('  ship it  ')
    const { recorder, sent } = createRecorder({ page: bridge })
    recorder.provider = 'chrome'

    await recorder.toggle()
    expect(recorder.stage).toBe('recording')
    expect(bridge.startSpeech).toHaveBeenCalledWith('en-US')

    await recorder.toggle()

    expect(sent).toEqual([{ transcript: 'ship it', ttsEnabled: true, voiceProvider: 'chrome' }])
    expect(recorder.stage).toBe('idle')
    expect(recorder.error).toBe('')
  })

  it('reports when nothing was recognized', async () => {
    const { recorder, send } = createRecorder({ page: page('   ') })
    recorder.provider = 'chrome'

    await recorder.start()
    await recorder.stop()

    expect(send).not.toHaveBeenCalled()
    expect(recorder.error).toBe('No speech was recognized.')
    expect(recorder.stage).toBe('idle')
  })

  it('cancels the page recognition without sending', async () => {
    const bridge = page('ship it')
    const { recorder, send } = createRecorder({ page: bridge })
    recorder.provider = 'chrome'

    await recorder.start()
    await recorder.cancel()

    expect(bridge.cancelSpeech).toHaveBeenCalled()
    expect(send).not.toHaveBeenCalled()
    expect(recorder.stage).toBe('idle')
    expect(recorder.level).toBe(0)
  })

  it('surfaces a send failure as an error and still returns to idle', async () => {
    const { recorder } = createRecorder({
      page: page('ship it'),
      sendError: new Error('daemon offline')
    })
    recorder.provider = 'chrome'

    await recorder.start()
    await recorder.stop()

    expect(recorder.error).toBe('daemon offline')
    expect(recorder.stage).toBe('idle')
  })
})

describe('VoiceRecorder local speech', () => {
  it('accumulates final results and restarts recognition across pauses', async () => {
    const recognition = fakeRecognition()
    const { recorder, sent } = createRecorder({
      platform: { createSpeechRecognition: () => recognition }
    })
    recorder.provider = 'chrome'

    await recorder.start()
    expect(recorder.stage).toBe('recording')

    recognition.onresult?.({
      resultIndex: 0,
      results: [{ isFinal: true, 0: { transcript: 'hello' } }]
    })
    expect(recorder.transcript).toBe('hello')

    // Chrome ends recognition on a pause; the recorder restarts it.
    recognition.onend?.()
    await Promise.resolve()
    expect(recognition.started).toBe(2)

    recognition.onresult?.({
      resultIndex: 0,
      results: [{ isFinal: true, 0: { transcript: 'there' } }]
    })
    expect(recorder.transcript).toBe('hello there')

    await recorder.stop()
    await Promise.resolve()

    expect(sent).toEqual([{ transcript: 'hello there', ttsEnabled: true, voiceProvider: 'chrome' }])
  })

  it('keeps recording through a no-speech error', async () => {
    const recognition = fakeRecognition()
    const { recorder } = createRecorder({
      platform: { createSpeechRecognition: () => recognition }
    })
    recorder.provider = 'chrome'
    await recorder.start()

    recognition.onerror?.({ error: 'no-speech' })

    expect(recorder.error).toBe('')
    expect(recorder.stage).toBe('recording')
  })

  it('stops on a fatal recognition error without sending', async () => {
    const recognition = fakeRecognition()
    const { recorder, send } = createRecorder({
      platform: { createSpeechRecognition: () => recognition }
    })
    recorder.provider = 'chrome'
    await recorder.start()

    recognition.onerror?.({ error: 'not-allowed' })
    expect(recorder.error).toBe('Microphone access was blocked.')

    recognition.onend?.()
    await Promise.resolve()

    expect(send).not.toHaveBeenCalled()
    expect(recorder.stage).toBe('idle')
  })

  it('aborts the recognition on cancel', async () => {
    const recognition = fakeRecognition()
    const { recorder, send } = createRecorder({
      platform: { createSpeechRecognition: () => recognition }
    })
    recorder.provider = 'chrome'
    await recorder.start()

    await recorder.cancel()

    expect(recognition.aborted).toBe(true)
    expect(send).not.toHaveBeenCalled()
    expect(recorder.stage).toBe('idle')
  })
})

describe('VoiceRecorder local audio', () => {
  it('sends the captured blob with a derived file name', async () => {
    const recording = fakeRecording(new Blob(['abcd']), 'audio/ogg')
    const { recorder, sent } = createRecorder({
      capabilities: { transcription: ['ogg'] },
      platform: { startRecording: () => Promise.resolve(recording) }
    })

    await recorder.toggle()
    expect(recorder.stage).toBe('recording')

    await recorder.toggle()

    expect(recording.stop).toHaveBeenCalled()
    expect(sent).toHaveLength(1)
    expect(sent[0]).toMatchObject({
      voiceProvider: 'anda',
      mimeType: 'audio/ogg',
      fileName: 'chrome_voice_1700000000000.ogg',
      ttsEnabled: true
    })
    expect(recording.disposed.value).toBe(true)
  })

  it('reports an empty capture instead of sending it', async () => {
    vi.stubGlobal('chrome', { i18n: { getMessage: (key: string) => key } })
    const recording = fakeRecording(new Blob([]))
    const { recorder, send } = createRecorder({
      capabilities: { transcription: ['webm'] },
      platform: { startRecording: () => Promise.resolve(recording) }
    })

    await recorder.start()
    await recorder.stop()

    expect(send).not.toHaveBeenCalled()
    expect(recorder.stage).toBe('idle')
    expect(recorder.error).toBe('noVoiceCaptured')
    vi.unstubAllGlobals()
  })

  it('reports a refused microphone', async () => {
    const { recorder } = createRecorder({
      capabilities: { transcription: ['webm'] },
      platform: {
        startRecording: () => Promise.reject(new Error('Permission dismissed'))
      }
    })

    await recorder.start()

    expect(recorder.stage).toBe('idle')
    expect(recorder.error).toContain('Microphone permission was dismissed')
  })

  it('releases the stream on cancel without sending', async () => {
    const recording = fakeRecording(new Blob(['abcd']))
    const { recorder, send } = createRecorder({
      capabilities: { transcription: ['webm'] },
      platform: { startRecording: () => Promise.resolve(recording) }
    })

    await recorder.start()
    await recorder.cancel()

    expect(send).not.toHaveBeenCalled()
    expect(recording.disposed.value).toBe(true)
    expect(recorder.stage).toBe('idle')
  })
})

describe('VoiceRecorder page audio', () => {
  it('sends the captured audio from the page', async () => {
    const { recorder, sent } = createRecorder({
      capabilities: { transcription: ['webm'] },
      page: {
        startAudio: vi.fn(async () => undefined),
        stopAudio: vi.fn(async () => ({
          audioBase64: 'AAA',
          mimeType: 'audio/mp4',
          size: 3
        })),
        cancelAudio: vi.fn(async () => undefined)
      }
    })

    await recorder.start()
    await recorder.stop()

    expect(sent[0]).toMatchObject({
      voiceProvider: 'anda',
      audioBase64: 'AAA',
      mimeType: 'audio/mp4',
      fileName: 'chrome_voice_1700000000000.m4a'
    })
  })

  it('reports when the page produced no audio', async () => {
    vi.stubGlobal('chrome', { i18n: { getMessage: (key: string) => key } })
    const { recorder, send } = createRecorder({
      capabilities: { transcription: ['webm'] },
      page: {
        startAudio: vi.fn(async () => undefined),
        stopAudio: vi.fn(async () => ({}))
      }
    })

    await recorder.start()
    await recorder.stop()

    expect(send).not.toHaveBeenCalled()
    expect(recorder.error).toBe('noVoiceCaptured')
    expect(recorder.stage).toBe('idle')
    vi.unstubAllGlobals()
  })
})

describe('VoiceRecorder.selectProvider', () => {
  it('marks the choice as explicit and clears the error', () => {
    const { recorder } = createRecorder()
    recorder.error = 'stale'

    recorder.selectProvider('chrome')

    expect(recorder.provider).toBe('chrome')
    expect(recorder.providerSelected).toBe(true)
    expect(recorder.error).toBe('')
  })
})
