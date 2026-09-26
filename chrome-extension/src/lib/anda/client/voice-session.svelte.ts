import { getMessage } from '$lib/i18n'
import type { DaemonApi } from './daemon'
import type {
  DaemonVoiceCapabilities,
  PageAudioResult,
  PageSpeechResult,
  TranscriptionToolOutput,
  TtsToolOutput,
  VoiceCapabilities,
  VoiceProvider,
  VoiceRecordingInput
} from './types'
import {
  isAudioResource,
  normalizeCapabilityFormats,
  normalizeVoiceRecordingAudio,
  playAudioArtifact,
  playVoiceTtsPipeline,
  prepareVoiceTtsText,
  splitVoiceTtsText,
  voiceTtsChunkChars
} from './voice'

/**
 * The side panel's channel to the service worker, which owns page-context
 * capture and Chrome TTS. Rejects when the worker reports a failure.
 */
export interface ExtensionMessenger {
  send<Result = unknown>(
    type: string,
    message?: Record<string, unknown>
  ): Promise<{ result?: Result }>
}

/**
 * Speech in and speech out for one side panel: capability discovery,
 * transcription, playback, and the page-context capture bridge.
 *
 * `capabilities` is discovered lazily — every verb that needs a capability
 * refreshes once before giving up, so a panel opened before the daemon
 * connected still works on the first try. Playback answers with the provider
 * that actually spoke, or null when none could, rather than throwing: a failed
 * `speak` is a UI state, not an error.
 *
 * Capture verbs mirror the service worker's page-speech and page-audio
 * protocols. `stop*` rejects when the worker reports an error or returns
 * nothing usable; `cancel*` is best-effort and never rejects.
 */
export class VoiceSession {
  #daemon: DaemonApi
  #messenger: ExtensionMessenger
  #speech?: AbortController
  speaking = $state(false)

  capabilities = $state<VoiceCapabilities>({
    transcription: [],
    daemonTts: [],
    chromeTts: false
  })

  constructor(daemon: DaemonApi, messenger: ExtensionMessenger) {
    this.#daemon = daemon
    this.#messenger = messenger
  }

  /** Re-reads Chrome TTS availability and, with a token, the daemon's formats. */
  async refreshCapabilities(): Promise<VoiceCapabilities> {
    const chromeTts = await this.#chromeTtsAvailable().catch(() => false)
    let next: VoiceCapabilities = { transcription: [], daemonTts: [], chromeTts }
    if (this.#daemon.authorized) {
      const daemon = await this.#daemon.rpc<DaemonVoiceCapabilities>('capabilities', [])
      next = {
        transcription: normalizeCapabilityFormats(daemon.transcription, ['wav']),
        daemonTts: normalizeCapabilityFormats(daemon.tts, ['mp3']),
        chromeTts
      }
    }
    this.capabilities = next
    return next
  }

  /** Transcribes a recording, transcoding it to a format the daemon accepts. */
  async transcribe(recording: VoiceRecordingInput): Promise<TranscriptionToolOutput> {
    if (this.capabilities.transcription.length === 0) {
      await this.refreshCapabilities()
    }
    if (this.capabilities.transcription.length === 0) {
      throw new Error(getMessage('voiceTranscriptionNotConfigured'))
    }
    if (!recording.audioBase64 || !recording.fileName) {
      throw new Error(getMessage('audioCaptureMissingData'))
    }
    const normalized = await normalizeVoiceRecordingAudio(
      recording,
      this.capabilities.transcription
    )
    const { output } = await this.#daemon.toolCall<TranscriptionToolOutput>('transcribe_audio', {
      file_name: normalized.fileName,
      audio_base64: normalized.audioBase64
    })
    return output
  }

  /**
   * Speaks `text` through the preferred provider. Returns the provider that
   * spoke, or null when it was unavailable or playback failed.
   */
  async speak(text: string, preferredProvider: VoiceProvider): Promise<VoiceProvider | null> {
    this.stopSpeaking()
    const chunks = splitVoiceTtsText(prepareVoiceTtsText(text), voiceTtsChunkChars)
    if (!chunks.length) {
      return null
    }
    const speech = new AbortController()
    this.#speech = speech
    this.speaking = true
    try {
      if (preferredProvider === 'anda')
        return (await this.#speakWithAndaTts(chunks, speech.signal)) ? 'anda' : null
      return (await this.#speakWithChromeTts(chunks, speech.signal)) ? 'chrome' : null
    } finally {
      if (this.#speech === speech) {
        this.#speech = undefined
        this.speaking = false
      }
    }
  }

  stopSpeaking(): void {
    if (!this.#speech) return
    this.#speech.abort()
    this.#speech = undefined
    this.speaking = false
    void this.#messenger.send('anda_chrome_tts_stop').catch(() => {})
  }

  async startSpeechRecognition(language: string): Promise<void> {
    const result = await this.#capture<PageSpeechResult>('anda_page_speech_start', { language })
    if (result.error || result.started === false) {
      throw new Error(result.error || getMessage('browserSpeechStartFailed'))
    }
  }

  /** Returns the recognized transcript, or '' when nothing was heard. */
  async stopSpeechRecognition(): Promise<string> {
    const result = await this.#capture<PageSpeechResult>('anda_page_speech_stop')
    if (result.error) {
      throw new Error(result.error)
    }
    return result.transcript?.trim() || ''
  }

  async cancelSpeechRecognition(): Promise<void> {
    await this.#messenger.send('anda_page_speech_cancel').catch(() => undefined)
  }

  async startAudioCapture(mimeType?: string): Promise<void> {
    const result = await this.#capture<PageAudioResult>('anda_page_audio_start', { mimeType })
    if (result.error || result.started === false) {
      throw new Error(result.error || getMessage('andaVoiceStartFailed'))
    }
  }

  /** Returns captured audio; rejects when the page produced none. */
  async stopAudioCapture(): Promise<PageAudioResult> {
    const result = await this.#capture<PageAudioResult>('anda_page_audio_stop')
    if (result.error) {
      throw new Error(result.error)
    }
    if (!result.audioBase64 || !result.mimeType) {
      throw new Error(getMessage('noVoiceCaptured'))
    }
    return result
  }

  async cancelAudioCapture(): Promise<void> {
    await this.#messenger.send('anda_page_audio_cancel').catch(() => undefined)
  }

  async #capture<Result extends object>(
    type: string,
    message?: Record<string, unknown>
  ): Promise<Result> {
    const response = await this.#messenger.send<Result>(type, message)
    return response.result || ({} as Result)
  }

  async #speakWithChromeTts(chunks: string[], signal: AbortSignal): Promise<boolean> {
    if (!this.capabilities.chromeTts) {
      await this.refreshCapabilities().catch(() => undefined)
    }
    if (!this.capabilities.chromeTts) {
      return false
    }
    try {
      for (const chunk of chunks) {
        signal.throwIfAborted()
        await this.#messenger.send('anda_chrome_tts_speak', { text: chunk })
      }
      return true
    } catch (_error) {
      await this.#messenger.send('anda_chrome_tts_stop').catch(() => undefined)
      return false
    }
  }

  async #speakWithAndaTts(chunks: string[], signal: AbortSignal): Promise<boolean> {
    if (this.capabilities.daemonTts.length === 0) {
      await this.refreshCapabilities().catch(() => undefined)
    }
    if (this.capabilities.daemonTts.length === 0) {
      return false
    }
    try {
      // Synthesis runs one chunk ahead of playback so speech stays continuous.
      await playVoiceTtsPipeline(
        chunks,
        async (chunk, index) => {
          signal.throwIfAborted()
          const result = await this.#daemon.toolCall<TtsToolOutput>('synthesize_speech', {
            text: chunk,
            artifact_name: `anda_chrome_voice_${Date.now()}_${index + 1}`
          })
          const artifact = result.artifacts?.find(isAudioResource)
          signal.throwIfAborted()
          if (!artifact?.blob) {
            throw new Error('Anda TTS did not return playable audio.')
          }
          return artifact
        },
        (artifact) => playAudioArtifact(artifact, signal)
      )
      return true
    } catch (_error) {
      return false
    }
  }

  async #chromeTtsAvailable(): Promise<boolean> {
    const response = await this.#messenger.send<{ available?: boolean }>(
      'anda_chrome_tts_available'
    )
    return Boolean(response.result?.available)
  }
}
