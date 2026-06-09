export type BrowserSpeechRecognitionEvent = {
  resultIndex: number
  results: ArrayLike<{
    isFinal: boolean
    [index: number]: { transcript: string }
  }>
}

export type BrowserSpeechRecognitionError = {
  error?: string
  message?: string
}

export type BrowserSpeechRecognition = {
  lang: string
  continuous: boolean
  interimResults: boolean
  onresult: ((event: BrowserSpeechRecognitionEvent) => void) | null
  onerror: ((event: BrowserSpeechRecognitionError) => void) | null
  onend: (() => void) | null
  start(): void
  stop(): void
  abort?: () => void
}

type BrowserSpeechRecognitionConstructor = new () => BrowserSpeechRecognition

export function chromeSpeechErrorMessage(error: string): string {
  const normalized = error.toLowerCase()
  if (normalized.includes('permission dismissed')) {
    return 'Browser speech permission was dismissed.'
  }
  if (normalized.includes('permission was not accepted')) {
    return 'Browser speech permission was not accepted.'
  }
  if (normalized.includes('microphone access was blocked') || normalized.includes('not-allowed')) {
    return 'Browser speech microphone access was blocked.'
  }
  return error || 'Browser speech recognition did not start.'
}

export function audioCaptureErrorMessage(error: string): string {
  const normalized = error.toLowerCase()
  if (normalized.includes('permission dismissed')) {
    return 'Microphone permission was dismissed for the current page.'
  }
  if (
    normalized.includes('microphone access was blocked') ||
    normalized.includes('notallowed') ||
    normalized.includes('not-allowed')
  ) {
    return 'Microphone access was blocked for the current page.'
  }
  return error || 'Anda voice recording did not start.'
}

export function isPermissionError(error: string): boolean {
  const normalized = error.toLowerCase()
  return (
    normalized.includes('permission') ||
    normalized.includes('microphone access was blocked') ||
    normalized.includes('notallowed') ||
    normalized.includes('not-allowed')
  )
}

export function speechRecognitionErrorMessage(error: string): string {
  switch (error) {
    case 'not-allowed':
    case 'service-not-allowed':
      return 'Microphone access was blocked.'
    case 'audio-capture':
      return 'No microphone was found.'
    case 'network':
      return 'Browser speech recognition is offline.'
    default:
      return error || 'Browser speech recognition failed.'
  }
}

export function preferredRecordingMimeType(acceptedFormats: string[] = []): string {
  if (typeof MediaRecorder === 'undefined') {
    return ''
  }
  const accepted = new Set(acceptedFormats.map((format) => format.toLowerCase()))
  const directTypes = [
    { format: 'webm', mimeType: 'audio/webm;codecs=opus' },
    { format: 'webm', mimeType: 'audio/webm' },
    { format: 'ogg', mimeType: 'audio/ogg;codecs=opus' },
    { format: 'mp4', mimeType: 'audio/mp4' },
    { format: 'm4a', mimeType: 'audio/mp4' }
  ]
  const direct = directTypes.find(
    ({ format, mimeType }) => accepted.has(format) && MediaRecorder.isTypeSupported(mimeType)
  )
  if (direct) {
    return direct.mimeType
  }
  const fallbackTypes = [
    'audio/webm;codecs=opus',
    'audio/webm',
    'audio/ogg;codecs=opus',
    'audio/mp4'
  ]
  return fallbackTypes.find((type) => MediaRecorder.isTypeSupported(type)) || ''
}

export function audioExtensionForMime(mimeType: string): string {
  const normalized = mimeType.toLowerCase()
  if (normalized.includes('ogg')) {
    return 'ogg'
  }
  if (normalized.includes('mp4')) {
    return 'm4a'
  }
  if (normalized.includes('wav')) {
    return 'wav'
  }
  return 'webm'
}

export async function blobToBase64(blob: Blob): Promise<string> {
  const dataUrl = await new Promise<string>((resolve, reject) => {
    const reader = new FileReader()
    reader.onload = () => resolve(String(reader.result || ''))
    reader.onerror = () => reject(reader.error || new Error('Failed to read voice audio.'))
    reader.readAsDataURL(blob)
  })
  return dataUrl.split(',', 2)[1] || ''
}

export function isMacPlatform(): boolean {
  if (typeof navigator === 'undefined') {
    return false
  }
  return /mac|iphone|ipad|ipod/i.test(navigator.platform || navigator.userAgent)
}

export function speechRecognitionSupported(): boolean {
  return Boolean(speechRecognitionConstructor())
}

export function speechRecognitionConstructor(): BrowserSpeechRecognitionConstructor | null {
  const scope = globalThis as typeof globalThis & {
    SpeechRecognition?: BrowserSpeechRecognitionConstructor
    webkitSpeechRecognition?: BrowserSpeechRecognitionConstructor
  }
  return scope.SpeechRecognition || scope.webkitSpeechRecognition || null
}
