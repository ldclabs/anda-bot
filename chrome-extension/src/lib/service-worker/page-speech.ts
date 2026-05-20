import type { PageSpeechArgs, PageSpeechResult } from './types'

export function pageSpeechRecognitionDispatcher(
  args: PageSpeechArgs
): PageSpeechResult | Promise<PageSpeechResult> {
  type PageRecognitionEvent = {
    resultIndex: number
    results: ArrayLike<{
      isFinal: boolean
      [index: number]: { transcript: string }
    }>
  }
  type PageRecognitionError = {
    error?: string
    message?: string
  }
  type PageRecognition = {
    lang: string
    continuous: boolean
    interimResults: boolean
    onstart: (() => void) | null
    onresult: ((event: PageRecognitionEvent) => void) | null
    onerror: ((event: PageRecognitionError) => void) | null
    onend: (() => void) | null
    start(): void
    stop(): void
    abort?: () => void
  }
  type PageRecognitionConstructor = new () => PageRecognition
  type PageRecognitionState = {
    recognition: PageRecognition | null
    finalTranscript: string
    interimTranscript: string
    error: string
    stopRequested: boolean
    canceled: boolean
    active: boolean
    lastResult: PageSpeechResult | null
    stopResolver: ((result: PageSpeechResult) => void) | null
    stopTimer: number | null
  }

  const scope = globalThis as typeof globalThis & {
    SpeechRecognition?: PageRecognitionConstructor
    webkitSpeechRecognition?: PageRecognitionConstructor
    __andaSpeechRecognition?: PageRecognitionState
  }
  const Recognition = scope.SpeechRecognition || scope.webkitSpeechRecognition
  if (args.action === 'available') {
    return { available: Boolean(Recognition) }
  }
  if (!Recognition) {
    return { available: false, error: 'Browser speech recognition is unavailable on this page.' }
  }

  const state =
    scope.__andaSpeechRecognition ||
    (scope.__andaSpeechRecognition = {
      recognition: null,
      finalTranscript: '',
      interimTranscript: '',
      error: '',
      stopRequested: false,
      canceled: false,
      active: false,
      lastResult: null,
      stopResolver: null,
      stopTimer: null
    })

  function transcript(): string {
    return `${state.finalTranscript} ${state.interimTranscript}`.trim()
  }

  function resetForStart(): void {
    if (state.stopTimer !== null) {
      clearTimeout(state.stopTimer)
    }
    state.finalTranscript = ''
    state.interimTranscript = ''
    state.error = ''
    state.stopRequested = false
    state.canceled = false
    state.active = false
    state.lastResult = null
    state.stopResolver = null
    state.stopTimer = null
  }

  function deactivate(result?: PageSpeechResult): PageSpeechResult {
    const output =
      result ||
      (state.error
        ? { available: true, error: pageSpeechErrorMessage(state.error) }
        : { available: true, transcript: transcript() })
    if (state.stopTimer !== null) {
      clearTimeout(state.stopTimer)
    }
    const resolver = state.stopResolver
    state.recognition = null
    state.finalTranscript = ''
    state.interimTranscript = ''
    state.error = ''
    state.stopRequested = false
    state.canceled = false
    state.active = false
    state.stopResolver = null
    state.stopTimer = null
    state.lastResult = output
    resolver?.(output)
    return output
  }

  function detachRecognition(recognition: PageRecognition): void {
    recognition.onstart = null
    recognition.onresult = null
    recognition.onerror = null
    recognition.onend = null
  }

  function cancelRecognition(): PageSpeechResult {
    const recognition = state.recognition
    state.canceled = true
    state.stopRequested = true
    if (recognition) {
      detachRecognition(recognition)
      try {
        recognition.abort?.()
      } catch (_error) {
        try {
          recognition.stop()
        } catch (_stopError) {}
      }
    }
    return deactivate({ available: true, canceled: true })
  }

  if (args.action === 'cancel') {
    return cancelRecognition()
  }

  if (args.action === 'stop') {
    if (!state.recognition) {
      const result = state.lastResult || { available: true, transcript: transcript() }
      state.lastResult = null
      return result
    }

    state.stopRequested = true
    return new Promise<PageSpeechResult>((resolve) => {
      state.stopResolver = resolve
      state.stopTimer = window.setTimeout(() => {
        deactivate()
      }, 3000)
      try {
        state.recognition?.stop()
      } catch (_error) {
        deactivate()
      }
    })
  }

  if (args.action !== 'start') {
    return { available: true, error: 'Unknown browser speech recognition action.' }
  }

  if (state.recognition) {
    cancelRecognition()
  }
  resetForStart()

  const recognition = new Recognition()
  state.recognition = recognition
  state.active = true
  recognition.lang = args.language || navigator.language || 'zh-CN'
  recognition.continuous = true
  recognition.interimResults = true

  return new Promise<PageSpeechResult>((resolve) => {
    let settled = false
    let startTimer: number | null = null

    function settle(result: PageSpeechResult): void {
      if (settled) {
        return
      }
      settled = true
      if (startTimer !== null) {
        clearTimeout(startTimer)
      }
      resolve(result)
    }

    recognition.onstart = () => {
      settle({ available: true, started: true })
    }
    recognition.onresult = (event) => {
      let interimTranscript = ''
      for (let index = event.resultIndex; index < event.results.length; index += 1) {
        const result = event.results[index]
        const recognized = result[0]?.transcript?.trim() || ''
        if (!recognized) {
          continue
        }
        if (result.isFinal) {
          state.finalTranscript = `${state.finalTranscript} ${recognized}`.trim()
        } else {
          interimTranscript = `${interimTranscript} ${recognized}`.trim()
        }
      }
      state.interimTranscript = interimTranscript
    }
    recognition.onerror = (event) => {
      const errorName = event.error || event.message || ''
      if (errorName === 'no-speech') {
        return
      }
      state.error = errorName || 'Browser speech recognition failed.'
      settle({ available: true, started: false, error: pageSpeechErrorMessage(state.error) })
    }
    recognition.onend = () => {
      if (state.stopRequested || state.canceled || state.error) {
        deactivate()
        return
      }
      try {
        recognition.start()
      } catch (error) {
        state.error = error instanceof Error ? error.message : String(error)
        deactivate()
      }
    }

    try {
      recognition.start()
      startTimer = window.setTimeout(() => {
        state.error = 'permission-timeout'
        try {
          recognition.abort?.()
        } catch (_error) {}
        settle({ available: true, started: false, error: pageSpeechErrorMessage(state.error) })
      }, 8000)
    } catch (error) {
      state.error = error instanceof Error ? error.message : String(error)
      deactivate()
      settle({ available: true, started: false, error: pageSpeechErrorMessage(state.error) })
    }
  })

  function pageSpeechErrorMessage(error: string): string {
    const normalized = error.toLowerCase()
    if (normalized.includes('permission dismissed')) {
      return 'Chrome speech permission was dismissed.'
    }
    switch (error) {
      case 'not-allowed':
      case 'service-not-allowed':
        return 'Microphone access was blocked for the current page.'
      case 'permission-timeout':
        return 'Chrome speech permission was not accepted.'
      case 'audio-capture':
        return 'No microphone was found.'
      case 'network':
        return 'Browser speech recognition is offline.'
      default:
        return error || 'Browser speech recognition failed.'
    }
  }
}
