import { describe, expect, it, vi } from 'vitest'
import type { DaemonApi } from './daemon'
import { VoiceSession, type ExtensionMessenger } from './voice-session.svelte'

/**
 * `VoiceSession` takes the daemon and the service-worker channel as ports, so
 * the whole speech pipeline is exercised without a `chrome` global.
 */
function createSession(
  options: {
    authorized?: boolean
    capabilities?: { transcription?: string[]; tts?: string[] }
    chromeTts?: boolean
    messengerResults?: Record<string, unknown>
    messengerError?: Error
    toolOutput?: unknown
  } = {}
) {
  const send = vi.fn(async (type: string) => {
    if (options.messengerError) {
      throw options.messengerError
    }
    if (type === 'anda_chrome_tts_available') {
      return { result: { available: options.chromeTts ?? false } }
    }
    return { result: options.messengerResults?.[type] }
  })
  const messenger: ExtensionMessenger = { send: send as ExtensionMessenger['send'] }

  const rpc = vi.fn(
    async () =>
      ({
        transcription: options.capabilities?.transcription,
        tts: options.capabilities?.tts
      }) as never
  )
  const toolCall = vi.fn(async () => (options.toolOutput ?? { output: {}, usage: {} }) as never)
  const daemon: DaemonApi = {
    authorized: options.authorized ?? true,
    rpc: rpc as DaemonApi['rpc'],
    toolCall
  }

  return { session: new VoiceSession(daemon, messenger), send, rpc, toolCall }
}

describe('VoiceSession.refreshCapabilities', () => {
  it('merges Chrome TTS availability with the daemon formats', async () => {
    const { session, rpc } = createSession({
      chromeTts: true,
      capabilities: { transcription: ['wav', 'mp3'], tts: ['mp3'] }
    })

    const capabilities = await session.refreshCapabilities()

    expect(capabilities.chromeTts).toBe(true)
    expect(capabilities.transcription).toEqual(['wav', 'mp3'])
    expect(capabilities.daemonTts).toEqual(['mp3'])
    expect(rpc).toHaveBeenCalledWith('capabilities', [])
    expect(session.capabilities).toEqual(capabilities)
  })

  it('reports Chrome TTS only when there is no token', async () => {
    const { session, rpc } = createSession({ authorized: false, chromeTts: true })

    const capabilities = await session.refreshCapabilities()

    expect(capabilities).toEqual({ transcription: [], daemonTts: [], chromeTts: true })
    expect(rpc).not.toHaveBeenCalled()
  })

  it('treats an unreachable service worker as no Chrome TTS', async () => {
    const { session } = createSession({
      authorized: false,
      messengerError: new Error('worker asleep')
    })

    expect((await session.refreshCapabilities()).chromeTts).toBe(false)
  })
})

describe('VoiceSession.transcribe', () => {
  const recording = {
    audioBase64: 'AAA',
    fileName: 'clip.wav',
    mimeType: 'audio/wav',
    ttsEnabled: false
  }

  it('discovers capabilities once before giving up', async () => {
    const { session, rpc } = createSession({ capabilities: { transcription: [], tts: [] } })

    await expect(session.transcribe(recording)).rejects.toThrow()
    expect(rpc).toHaveBeenCalledTimes(1)
  })

  it('rejects a recording with no audio', async () => {
    const { session } = createSession({ capabilities: { transcription: ['wav'] } })

    await expect(session.transcribe({ ...recording, audioBase64: '' })).rejects.toThrow()
  })

  it('sends the audio to the transcribe_audio tool', async () => {
    const { session, toolCall } = createSession({
      capabilities: { transcription: ['wav'] },
      toolOutput: { output: { text: ' hello there ' }, usage: {} }
    })

    const output = await session.transcribe(recording)

    expect(output.text).toBe(' hello there ')
    expect(toolCall).toHaveBeenCalledWith(
      'transcribe_audio',
      expect.objectContaining({ file_name: 'clip.wav' })
    )
  })
})

describe('VoiceSession.speak', () => {
  it('returns null for text that has nothing speakable', async () => {
    const { session, send } = createSession({ chromeTts: true })

    expect(await session.speak('   ', 'chrome')).toBeNull()
    expect(send).not.toHaveBeenCalled()
  })

  it('speaks through Chrome TTS when it is available', async () => {
    const { session, send } = createSession({ chromeTts: true })

    expect(await session.speak('hello there', 'chrome')).toBe('chrome')
    expect(send).toHaveBeenCalledWith('anda_chrome_tts_speak', { text: 'hello there' })
  })

  it('returns null when Chrome TTS is unavailable', async () => {
    const { session } = createSession({ authorized: false, chromeTts: false })

    expect(await session.speak('hello there', 'chrome')).toBeNull()
  })

  it('returns null when the daemon offers no TTS formats', async () => {
    const { session, toolCall } = createSession({ capabilities: { tts: [] } })

    expect(await session.speak('hello there', 'anda')).toBeNull()
    expect(toolCall).not.toHaveBeenCalled()
  })

  it('reports failure rather than throwing when daemon audio is unplayable', async () => {
    const { session } = createSession({
      capabilities: { tts: ['mp3'] },
      // No artifacts, so the synthesizer cannot produce playable audio.
      toolOutput: { output: {}, artifacts: [], usage: {} }
    })

    expect(await session.speak('hello there', 'anda')).toBeNull()
  })
})

describe('VoiceSession capture bridge', () => {
  it('starts and stops page speech recognition', async () => {
    const { session, send } = createSession({
      messengerResults: {
        anda_page_speech_start: { started: true },
        anda_page_speech_stop: { transcript: '  ship it  ' }
      }
    })

    await session.startSpeechRecognition('en-US')
    expect(send).toHaveBeenCalledWith('anda_page_speech_start', { language: 'en-US' })

    expect(await session.stopSpeechRecognition()).toBe('ship it')
  })

  it('rejects when the page refuses to start recognition', async () => {
    const { session } = createSession({
      messengerResults: { anda_page_speech_start: { started: false, error: 'mic blocked' } }
    })

    await expect(session.startSpeechRecognition('en-US')).rejects.toThrow('mic blocked')
  })

  it('returns empty text when nothing was recognized', async () => {
    const { session } = createSession({ messengerResults: { anda_page_speech_stop: {} } })

    expect(await session.stopSpeechRecognition()).toBe('')
  })

  it('returns captured audio and rejects when the page produced none', async () => {
    const { session } = createSession({
      messengerResults: {
        anda_page_audio_start: { started: true },
        anda_page_audio_stop: { audioBase64: 'AAA', mimeType: 'audio/webm', size: 3 }
      }
    })

    await session.startAudioCapture('audio/webm')
    expect(await session.stopAudioCapture()).toMatchObject({ mimeType: 'audio/webm' })

    const empty = createSession({ messengerResults: { anda_page_audio_stop: {} } })
    await expect(empty.session.stopAudioCapture()).rejects.toThrow()
  })

  it('never rejects from a cancel', async () => {
    const { session } = createSession({ messengerError: new Error('worker asleep') })

    await expect(session.cancelSpeechRecognition()).resolves.toBeUndefined()
    await expect(session.cancelAudioCapture()).resolves.toBeUndefined()
  })
})
