import { getMessage } from '$lib/i18n'
import type {
  PageAudioResult,
  VoiceCapabilities,
  VoiceProvider,
  VoiceRecordingInput
} from '../client/types'
import {
  audioCaptureErrorMessage,
  audioExtensionForMime,
  blobToBase64,
  chromeSpeechErrorMessage,
  isPermissionError,
  preferredRecordingMimeType,
  speechRecognitionConstructor,
  speechRecognitionErrorMessage,
  speechRecognitionSupported,
  type BrowserSpeechRecognition,
  type BrowserSpeechRecognitionError,
  type BrowserSpeechRecognitionEvent
} from './voice'

export type VoiceStage = 'idle' | 'recording' | 'processing'

/**
 * The side panel's page-context capture, when the composer is wired to it.
 * Each pair is present or absent together; absent means "use the local path".
 */
export interface PageCaptureBridge {
  startSpeech?: (language: string) => Promise<void>
  stopSpeech?: () => Promise<string>
  cancelSpeech?: () => Promise<void>
  startAudio?: (mimeType?: string) => Promise<void>
  stopAudio?: () => Promise<PageAudioResult>
  cancelAudio?: () => Promise<void>
}

/** A microphone stream plus the level meter driven from it. */
export interface LocalRecording {
  /** Resolves with the recorded audio once `stop()` completes. */
  readonly blob: Promise<Blob>
  readonly mimeType: string
  stop(): void
  dispose(): void
}

/**
 * The browser APIs the local capture paths need. `browserVoicePlatform()` is
 * the production adapter; tests pass a fake, which is why this module has no
 * direct `window`, `navigator`, or `MediaRecorder` reference.
 */
export interface VoicePlatform {
  language(): string
  /** Whether the browser exposes SpeechRecognition, without allocating one. */
  speechRecognitionSupported(): boolean
  /** null when the browser has no SpeechRecognition. */
  createSpeechRecognition(): BrowserSpeechRecognition | null
  /** null when the browser cannot record; rejects when the mic is refused. */
  startRecording(acceptedFormats: string[]): Promise<LocalRecording> | null
  /** Drives `onLevel` until the returned handle is disposed. */
  meter(recording: LocalRecording | null, onLevel: (level: number) => void): () => void
  /** Encodes recorded audio for the daemon. */
  toBase64(blob: Blob): Promise<string>
  now(): number
}

export interface VoiceRecorderOptions {
  /** Formats the daemon accepts, used to pick a recording MIME type. */
  capabilities(): VoiceCapabilities
  /** Whether the reply should be spoken back. */
  ttsEnabled(): boolean
  /** Delivers the finished turn; rejecting surfaces as `error`. */
  send(input: VoiceRecordingInput): Promise<void>
  page?: () => PageCaptureBridge
  platform?: VoicePlatform
}

/**
 * The composer's push-to-talk recorder across all four capture paths: browser
 * speech recognition or raw audio, each either in the page (through the side
 * panel's service worker) or locally in the panel.
 *
 * The view reads `stage`, `transcript`, `error`, `level`, and `provider`, and
 * drives the whole machine with `toggle()` / `cancel()`. Path selection is
 * internal and falls back: Chrome speech degrades to Anda audio when
 * recognition will not start, and page capture degrades to local capture unless
 * the failure was a denied permission (retrying locally would only prompt
 * again).
 *
 * `stage` always returns to 'idle', and every failure lands in `error` rather
 * than rejecting, so a view can call these verbs without a try/catch.
 */
export class VoiceRecorder {
  #options: VoiceRecorderOptions
  #platform: VoicePlatform

  stage = $state<VoiceStage>('idle')
  transcript = $state('')
  error = $state('')
  level = $state(0)
  provider = $state<VoiceProvider>('anda')
  /** True once the user picked a provider, so auto-selection stops overriding. */
  providerSelected = $state(false)

  #speechRecognition: BrowserSpeechRecognition | null = null
  #speechMode: 'local' | 'page' | null = null
  #finalTranscript = ''
  #stopRequested = false
  #fatalError = ''
  #discardRecognition = false

  #recording: LocalRecording | null = null
  #audioMode: 'local' | 'page' | null = null
  #discardRecording = false
  #stopMeter: (() => void) | null = null

  constructor(options: VoiceRecorderOptions) {
    this.#options = options
    this.#platform = options.platform || browserVoicePlatform()
  }

  get recording(): boolean {
    return this.stage === 'recording'
  }

  selectProvider(provider: VoiceProvider): void {
    this.provider = provider
    this.providerSelected = true
    this.error = ''
  }

  /** Starts recording, or stops an in-progress recording and sends it. */
  async toggle(): Promise<void> {
    if (this.stage === 'recording') {
      await this.stop()
      return
    }
    await this.start()
  }

  async start(): Promise<void> {
    this.transcript = ''
    const { chromeSpeech, andaVoice } = this.#available()

    if (this.provider === 'chrome' && chromeSpeech) {
      if (await this.#startSpeech()) {
        return
      }
      const speechError = this.error
      if (andaVoice) {
        this.provider = 'anda'
        this.error = ''
        await this.#startAndaCapture()
        return
      }
      this.error = chromeSpeechErrorMessage(speechError)
      return
    }

    if (andaVoice) {
      this.provider = 'anda'
      await this.#startAndaCapture()
      return
    }
    this.error = 'Selected voice service is unavailable.'
  }

  /** Ends the recording and delivers the turn. */
  async stop(): Promise<void> {
    if (this.#speechMode === 'page') {
      await this.#finishPageSpeech()
      return
    }
    if (this.#audioMode === 'page') {
      await this.#finishPageAudio()
      return
    }
    if (this.#speechRecognition) {
      this.#stopRequested = true
      this.stage = 'processing'
      this.#speechRecognition.stop()
      return
    }
    if (this.#recording) {
      this.stage = 'processing'
      const recording = this.#recording
      recording.stop()
      await this.#finishLocalRecording(recording)
    }
  }

  /** Abandons the recording without sending anything. */
  async cancel(): Promise<void> {
    this.#discardRecognition = true
    this.#discardRecording = true
    this.#stopRequested = false
    this.#fatalError = ''

    const page = this.#options.page?.()
    if (this.#speechMode === 'page') {
      await page?.cancelSpeech?.().catch(() => undefined)
      this.#speechMode = null
    }
    if (this.#audioMode === 'page') {
      await page?.cancelAudio?.().catch(() => undefined)
      this.#audioMode = null
    }
    if (this.#speechRecognition) {
      const recognition = this.#speechRecognition
      recognition.onend = null
      try {
        recognition.abort?.()
      } catch (_error) {
        try {
          recognition.stop()
        } catch (_stopError) {
          // Already stopped; nothing left to abandon.
        }
      }
      this.#speechRecognition = null
    }
    this.#recording?.stop()
    this.#cleanup()
    this.stage = 'idle'
    this.level = 0
  }

  /** Releases the microphone and any timers. Call from the view's teardown. */
  dispose(): void {
    void this.cancel()
  }

  #available(): { chromeSpeech: boolean; andaVoice: boolean } {
    const page = this.#options.page?.()
    return {
      chromeSpeech: Boolean(
        (page?.startSpeech && page.stopSpeech) || this.#platform.speechRecognitionSupported()
      ),
      andaVoice: this.#options.capabilities().transcription.length > 0
    }
  }

  async #startSpeech(): Promise<boolean> {
    const page = this.#options.page?.()
    if (page?.startSpeech && page.stopSpeech) {
      return this.#startPageSpeech(page)
    }
    return this.#startLocalSpeech()
  }

  async #startPageSpeech(page: PageCaptureBridge): Promise<boolean> {
    this.#resetSpeechState()
    this.#speechMode = 'page'
    this.stage = 'recording'
    this.#startSyntheticPulse()
    try {
      await page.startSpeech?.(this.#platform.language())
      return true
    } catch (error) {
      this.#speechMode = null
      this.#cleanup()
      this.stage = 'idle'
      this.level = 0
      this.error = chromeSpeechErrorMessage(errorText(error))
      return false
    }
  }

  #startLocalSpeech(): boolean {
    const recognition = this.#platform.createSpeechRecognition()
    if (!recognition) {
      this.error = 'Browser speech recognition is unavailable.'
      return false
    }

    this.#resetSpeechState()
    try {
      recognition.lang = this.#platform.language()
      recognition.continuous = true
      recognition.interimResults = true
      recognition.onresult = (event) => this.#onSpeechResult(event)
      recognition.onerror = (event) => this.#onSpeechError(event)
      recognition.onend = () => void this.#onSpeechEnd(recognition)
      this.#speechRecognition = recognition
      this.#speechMode = 'local'
      recognition.start()
      this.stage = 'recording'
      this.#startSyntheticPulse()
      return true
    } catch (error) {
      this.#speechRecognition = null
      this.#speechMode = null
      this.stage = 'idle'
      this.error = chromeSpeechErrorMessage(errorText(error))
      return false
    }
  }

  async #startAndaCapture(): Promise<void> {
    const page = this.#options.page?.()
    if (page?.startAudio && page.stopAudio) {
      if (await this.#startPageAudio(page)) {
        return
      }
      // A denied microphone will only be denied again locally.
      if (isPermissionError(this.error)) {
        return
      }
    }
    await this.#startLocalAudio()
  }

  async #startPageAudio(page: PageCaptureBridge): Promise<boolean> {
    this.error = ''
    this.transcript = ''
    this.#discardRecording = false
    this.#audioMode = 'page'
    this.stage = 'recording'
    this.#startSyntheticPulse()
    try {
      await page.startAudio?.(
        preferredRecordingMimeType(this.#options.capabilities().transcription)
      )
      return true
    } catch (error) {
      this.#audioMode = null
      this.#cleanup()
      this.stage = 'idle'
      this.level = 0
      this.error = audioCaptureErrorMessage(errorText(error))
      return false
    }
  }

  async #startLocalAudio(): Promise<void> {
    const pending = this.#platform.startRecording(this.#options.capabilities().transcription)
    if (!pending) {
      this.error = 'Voice input is unavailable in this browser.'
      return
    }
    this.error = ''
    this.#discardRecording = false
    this.#audioMode = 'local'
    try {
      this.#recording = await pending
      // A failed Chrome-speech attempt may have left the synthetic pulse running.
      this.#stopMeter?.()
      this.#stopMeter = this.#platform.meter(this.#recording, (level) => {
        this.level = level
      })
      this.stage = 'recording'
    } catch (error) {
      this.#cleanup()
      this.stage = 'idle'
      this.error = audioCaptureErrorMessage(errorText(error))
    }
  }

  #onSpeechResult(event: BrowserSpeechRecognitionEvent): void {
    this.error = ''
    let interim = ''
    for (let index = event.resultIndex; index < event.results.length; index += 1) {
      const result = event.results[index]
      const transcript = result[0]?.transcript?.trim() || ''
      if (!transcript) {
        continue
      }
      if (result.isFinal) {
        this.#finalTranscript = `${this.#finalTranscript} ${transcript}`.trim()
      } else {
        interim = `${interim} ${transcript}`.trim()
      }
    }
    this.transcript = `${this.#finalTranscript} ${interim}`.trim()
    this.level = Math.min(1, Math.max(0.28, this.level + 0.18))
  }

  #onSpeechError(event: BrowserSpeechRecognitionError): void {
    const errorName = event.error || ''
    // Silence is normal mid-utterance, and an abort we asked for is not a fault.
    if (errorName === 'no-speech' || (errorName === 'aborted' && this.#discardRecognition)) {
      return
    }
    this.#fatalError = errorName || event.message || 'Browser speech recognition failed.'
    this.error = event.message || speechRecognitionErrorMessage(this.#fatalError)
  }

  async #onSpeechEnd(recognition: BrowserSpeechRecognition): Promise<void> {
    if (this.#discardRecognition || this.#fatalError) {
      await this.#finishSpeech()
      return
    }
    // Chrome ends recognition on every pause; restart until the user stops.
    if (!this.#stopRequested && this.stage === 'recording') {
      try {
        recognition.start()
        return
      } catch (error) {
        this.#fatalError = errorText(error)
        this.error = speechRecognitionErrorMessage(this.#fatalError)
      }
    }
    await this.#finishSpeech()
  }

  async #finishPageSpeech(): Promise<void> {
    const page = this.#options.page?.()
    if (!page?.stopSpeech) {
      this.#failNotConnected()
      return
    }
    this.#stopRequested = true
    this.stage = 'processing'
    try {
      const transcript = (await page.stopSpeech()).trim()
      this.#finalTranscript = transcript
      this.transcript = transcript
      await this.#finishSpeech()
    } catch (error) {
      this.#speechMode = null
      this.#cleanup()
      this.level = 0
      this.error = errorText(error)
      this.stage = 'idle'
    }
  }

  async #finishSpeech(): Promise<void> {
    const transcript = this.transcript.trim() || this.#finalTranscript.trim()
    this.#speechRecognition = null
    this.#speechMode = null
    this.#cleanup()
    this.level = 0
    this.#stopRequested = false

    if (this.#discardRecognition) {
      this.#discardRecognition = false
      this.stage = 'idle'
      return
    }
    if (this.#fatalError) {
      this.#fatalError = ''
      this.stage = 'idle'
      return
    }
    if (!transcript) {
      this.error = 'No speech was recognized.'
      this.stage = 'idle'
      return
    }
    await this.#deliver({
      transcript,
      ttsEnabled: this.#options.ttsEnabled(),
      voiceProvider: this.provider
    })
  }

  async #finishPageAudio(): Promise<void> {
    const page = this.#options.page?.()
    if (!page?.stopAudio) {
      this.#failNotConnected()
      return
    }
    this.stage = 'processing'
    let result: PageAudioResult | null = null
    try {
      result = await page.stopAudio()
    } catch (error) {
      this.error = audioCaptureErrorMessage(errorText(error))
    } finally {
      this.#cleanup()
      this.level = 0
    }

    if (this.#discardRecording) {
      this.#discardRecording = false
      this.stage = 'idle'
      return
    }
    if (!result) {
      this.stage = 'idle'
      return
    }
    if (!result.audioBase64 || !result.mimeType) {
      this.error = getMessage('noVoiceCaptured')
      this.stage = 'idle'
      return
    }
    await this.#deliver({
      voiceProvider: 'anda',
      audioBase64: result.audioBase64,
      fileName: this.#audioFileName(result.mimeType),
      mimeType: result.mimeType,
      size: result.size,
      ttsEnabled: this.#options.ttsEnabled()
    })
  }

  async #finishLocalRecording(recording: LocalRecording): Promise<void> {
    let blob: Blob | null = null
    try {
      blob = await recording.blob
    } catch (error) {
      this.error = audioCaptureErrorMessage(errorText(error))
    } finally {
      this.#cleanup()
      this.level = 0
    }

    if (this.#discardRecording) {
      this.#discardRecording = false
      this.stage = 'idle'
      return
    }
    if (!blob?.size) {
      if (!this.error) {
        this.error = getMessage('noVoiceCaptured')
      }
      this.stage = 'idle'
      return
    }
    await this.#deliver({
      voiceProvider: this.provider,
      audioBase64: await this.#platform.toBase64(blob),
      fileName: this.#audioFileName(recording.mimeType),
      mimeType: recording.mimeType,
      size: blob.size,
      ttsEnabled: this.#options.ttsEnabled()
    })
  }

  async #deliver(input: VoiceRecordingInput): Promise<void> {
    try {
      this.stage = 'processing'
      await this.#options.send(input)
      this.error = ''
    } catch (error) {
      this.error = errorText(error)
    } finally {
      this.stage = 'idle'
    }
  }

  #audioFileName(mimeType: string): string {
    return `chrome_voice_${this.#platform.now()}.${audioExtensionForMime(mimeType)}`
  }

  #failNotConnected(): void {
    this.error = getMessage('voiceNotConnected') || 'Voice mode is not connected.'
    this.stage = 'idle'
  }

  #resetSpeechState(): void {
    this.error = ''
    this.transcript = ''
    this.#finalTranscript = ''
    this.#discardRecognition = false
    this.#stopRequested = false
    this.#fatalError = ''
  }

  /** Page capture gives no waveform, so decay a pulse to keep the orb alive. */
  #startSyntheticPulse(): void {
    this.#stopMeter?.()
    this.#stopMeter = this.#platform.meter(null, (level) => {
      this.level = this.stage === 'recording' ? Math.max(0.12, this.level * 0.86 || level) : 0
    })
  }

  #cleanup(): void {
    this.#stopMeter?.()
    this.#stopMeter = null
    this.#recording?.dispose()
    this.#recording = null
    this.#audioMode = null
  }
}

function errorText(error: unknown): string {
  return error instanceof Error ? error.message : String(error)
}

/** The production {@link VoicePlatform}, backed by the panel's own window. */
export function browserVoicePlatform(): VoicePlatform {
  return {
    language: () => navigator.language || 'zh-CN',

    speechRecognitionSupported: () => speechRecognitionSupported(),

    createSpeechRecognition() {
      const Recognition = speechRecognitionConstructor()
      return Recognition ? new Recognition() : null
    },

    startRecording(acceptedFormats) {
      if (!navigator.mediaDevices?.getUserMedia || typeof MediaRecorder === 'undefined') {
        return null
      }
      return (async () => {
        const stream = await navigator.mediaDevices.getUserMedia({
          audio: { echoCancellation: true, noiseSuppression: true, autoGainControl: true }
        })
        const preferred = preferredRecordingMimeType(acceptedFormats)
        const recorder = new MediaRecorder(stream, preferred ? { mimeType: preferred } : undefined)
        const chunks: Blob[] = []
        recorder.ondataavailable = (event) => {
          if (event.data.size > 0) {
            chunks.push(event.data)
          }
        }
        const mimeType = recorder.mimeType || preferred || 'audio/webm'
        const blob = new Promise<Blob>((resolve) => {
          recorder.onstop = () => resolve(new Blob(chunks, { type: mimeType }))
        })
        recorder.start()
        return {
          blob,
          mimeType,
          stream,
          stop: () => {
            if (recorder.state === 'recording') {
              recorder.stop()
            }
          },
          dispose: () => {
            stream.getTracks().forEach((track) => track.stop())
          }
        } satisfies LocalRecording & { stream: MediaStream }
      })()
    },

    toBase64: (blob) => blobToBase64(blob),

    meter(recording, onLevel) {
      let frame: number | null = null
      const stream = (recording as (LocalRecording & { stream?: MediaStream }) | null)?.stream
      if (!stream || typeof AudioContext === 'undefined') {
        // No waveform available: let the caller decay its own synthetic level.
        const pulse = () => {
          onLevel(0.12)
          frame = requestAnimationFrame(pulse)
        }
        pulse()
        return () => {
          if (frame !== null) {
            cancelAnimationFrame(frame)
          }
        }
      }

      const context = new AudioContext()
      const analyser = context.createAnalyser()
      analyser.fftSize = 256
      context.createMediaStreamSource(stream).connect(analyser)
      const samples = new Uint8Array(analyser.frequencyBinCount)
      const update = () => {
        analyser.getByteFrequencyData(samples)
        const total = samples.reduce((sum, sample) => sum + sample, 0)
        onLevel(Math.min(1, total / samples.length / 120))
        frame = requestAnimationFrame(update)
      }
      update()
      return () => {
        if (frame !== null) {
          cancelAnimationFrame(frame)
        }
        void context.close().catch(() => undefined)
      }
    },

    now: () => Date.now()
  }
}
