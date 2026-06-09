import { afterEach, describe, expect, it, vi } from 'vitest'
import { pageSpeechRecognitionDispatcher } from './page-speech'

class MockRecognition {
  static instances: MockRecognition[] = []
  static autoStart = true

  lang = ''
  continuous = false
  interimResults = false
  onstart: (() => void) | null = null
  onresult: ((event: unknown) => void) | null = null
  onerror: ((event: unknown) => void) | null = null
  onend: (() => void) | null = null
  start = vi.fn(() => {
    if (MockRecognition.autoStart) {
      this.onstart?.()
    }
  })
  stop = vi.fn()
  abort = vi.fn()

  constructor() {
    MockRecognition.instances.push(this)
  }
}

function installSpeechRecognitionMock() {
  MockRecognition.instances = []
  MockRecognition.autoStart = true
  vi.stubGlobal('webkitSpeechRecognition', MockRecognition)
  vi.stubGlobal('navigator', { language: 'en-US' })
  vi.stubGlobal('window', {
    setTimeout: globalThis.setTimeout,
    clearTimeout: globalThis.clearTimeout
  })
}

afterEach(() => {
  vi.useRealTimers()
  vi.restoreAllMocks()
  vi.unstubAllGlobals()
})

describe('pageSpeechRecognitionDispatcher', () => {
  it('does not restart recognition after a stop timeout and delayed onend', async () => {
    vi.useFakeTimers()
    installSpeechRecognitionMock()

    await expect(
      pageSpeechRecognitionDispatcher({ action: 'start', language: 'en-US' })
    ).resolves.toEqual({ available: true, started: true })
    const recognition = MockRecognition.instances[0]!

    const stopResult = pageSpeechRecognitionDispatcher({ action: 'stop' })
    expect(recognition.stop).toHaveBeenCalledTimes(1)

    await vi.advanceTimersByTimeAsync(3000)
    await expect(stopResult).resolves.toEqual({ available: true, transcript: '' })

    recognition.onend?.()
    expect(recognition.start).toHaveBeenCalledTimes(1)
  })

  it('cleans up recognition state when the start permission prompt times out', async () => {
    vi.useFakeTimers()
    installSpeechRecognitionMock()
    MockRecognition.autoStart = false

    const startResult = pageSpeechRecognitionDispatcher({ action: 'start', language: 'en-US' })
    const recognition = MockRecognition.instances[0]!

    await vi.advanceTimersByTimeAsync(8000)
    await expect(startResult).resolves.toEqual({
      available: true,
      started: false,
      error: 'Browser speech permission was not accepted.'
    })
    expect(recognition.abort).toHaveBeenCalledTimes(1)

    expect(pageSpeechRecognitionDispatcher({ action: 'stop' })).toEqual({
      available: true,
      started: false,
      error: 'Browser speech permission was not accepted.'
    })
  })
})
