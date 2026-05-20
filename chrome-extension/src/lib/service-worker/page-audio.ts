import type { PageAudioArgs, PageAudioResult } from './types'

export async function pageAudioCaptureDispatcher(args: PageAudioArgs): Promise<PageAudioResult> {
  type PageAudioState = {
    stream: MediaStream | null
    recorder: MediaRecorder | null
    chunks: Blob[]
    mimeType: string
    canceled: boolean
  }
  const global = globalThis as typeof globalThis & { __andaAudioCapture?: PageAudioState }

  function audioCaptureAvailable(): boolean {
    return (
      typeof navigator.mediaDevices?.getUserMedia === 'function' &&
      typeof MediaRecorder !== 'undefined'
    )
  }

  function audioErrorMessage(error: unknown): string {
    const name =
      error && typeof error === 'object' && 'name' in error ? String(error.name || '') : ''
    const message = error instanceof Error ? error.message : String(error || '')
    const normalized = `${name} ${message}`.toLowerCase()
    if (normalized.includes('permission dismissed')) {
      return 'Microphone permission was dismissed for the current page.'
    }
    if (
      normalized.includes('notallowed') ||
      normalized.includes('not-allowed') ||
      normalized.includes('permission denied') ||
      normalized.includes('permission blocked')
    ) {
      return 'Microphone access was blocked for the current page.'
    }
    if (normalized.includes('notfound') || normalized.includes('devices not found')) {
      return 'No microphone was found.'
    }
    return message || name || 'Anda voice recording failed.'
  }

  function supportedMimeType(mimeType?: string): string {
    const requested = mimeType?.trim()
    if (requested && MediaRecorder.isTypeSupported(requested)) {
      return requested
    }
    return (
      ['audio/ogg;codecs=opus', 'audio/webm;codecs=opus', 'audio/webm', 'audio/mp4'].find((type) =>
        MediaRecorder.isTypeSupported(type)
      ) || ''
    )
  }

  function cleanup(state: PageAudioState | undefined): void {
    state?.stream?.getTracks().forEach((track) => track.stop())
    global.__andaAudioCapture = undefined
  }

  function blobToBase64(blob: Blob): Promise<string> {
    return new Promise((resolve, reject) => {
      const reader = new FileReader()
      reader.onload = () => resolve(String(reader.result || '').split(',', 2)[1] || '')
      reader.onerror = () => reject(reader.error || new Error('Failed to read voice audio.'))
      reader.readAsDataURL(blob)
    })
  }

  async function finish(state: PageAudioState): Promise<PageAudioResult> {
    const blob = new Blob(state.chunks, { type: state.mimeType })
    cleanup(state)
    if (state.canceled) {
      return { available: true, canceled: true }
    }
    if (!blob.size) {
      return { available: true, error: 'No voice audio was captured.' }
    }
    return {
      available: true,
      audioBase64: await blobToBase64(blob),
      mimeType: blob.type || state.mimeType || 'audio/webm',
      size: blob.size
    }
  }

  if (!audioCaptureAvailable()) {
    return { available: false, error: 'Browser audio recording is unavailable.' }
  }

  if (args.action === 'available') {
    return { available: true }
  }

  if (args.action === 'cancel') {
    const state = global.__andaAudioCapture
    if (!state?.recorder) {
      cleanup(state)
      return { available: true, canceled: true }
    }
    state.canceled = true
    return new Promise<PageAudioResult>((resolve) => {
      state.recorder!.onstop = () => {
        cleanup(state)
        resolve({ available: true, canceled: true })
      }
      try {
        if (state.recorder!.state === 'inactive') {
          cleanup(state)
          resolve({ available: true, canceled: true })
        } else {
          state.recorder!.stop()
        }
      } catch (_error) {
        cleanup(state)
        resolve({ available: true, canceled: true })
      }
    })
  }

  if (args.action === 'stop') {
    const state = global.__andaAudioCapture
    if (!state?.recorder) {
      return { available: true, error: 'Anda voice recording is not active.' }
    }
    return new Promise<PageAudioResult>((resolve) => {
      state.recorder!.ondataavailable = (event) => {
        if (event.data.size > 0) {
          state.chunks.push(event.data)
        }
      }
      state.recorder!.onstop = () => {
        void finish(state)
          .then(resolve)
          .catch((error) => {
            cleanup(state)
            resolve({ available: true, error: audioErrorMessage(error) })
          })
      }
      try {
        state.recorder!.stop()
      } catch (error) {
        cleanup(state)
        resolve({ available: true, error: audioErrorMessage(error) })
      }
    })
  }

  if (args.action !== 'start') {
    return { available: true, error: 'Unknown Anda voice recording action.' }
  }

  cleanup(global.__andaAudioCapture)
  const mimeType = supportedMimeType(args.mimeType)
  try {
    const stream = await navigator.mediaDevices.getUserMedia({
      audio: {
        echoCancellation: true,
        noiseSuppression: true,
        autoGainControl: true
      }
    })
    const recorder = new MediaRecorder(stream, mimeType ? { mimeType } : undefined)
    const state: PageAudioState = {
      stream,
      recorder,
      chunks: [],
      mimeType: recorder.mimeType || mimeType || 'audio/webm',
      canceled: false
    }
    recorder.ondataavailable = (event) => {
      if (event.data.size > 0) {
        state.chunks.push(event.data)
      }
    }
    global.__andaAudioCapture = state
    recorder.start()
    return { available: true, started: true, mimeType: state.mimeType }
  } catch (error) {
    cleanup(global.__andaAudioCapture)
    return { available: true, started: false, error: audioErrorMessage(error) }
  }
}
