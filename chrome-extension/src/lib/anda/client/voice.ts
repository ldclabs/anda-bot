import { getPlainText } from '$lib/utils/markdown'
import type { Resource, VoiceRecordingInput } from './types'

export const voiceTtsChunkChars = 320
const voiceTtsShortChunkChars = 80
const voiceTtsPreferredChunkChars = 120
const voiceTtsMaxShortLines = 3

type NormalizedVoiceRecording = {
  audioBase64: string
  fileName: string
}

export type VoiceTtsSynthesizer<TArtifact> = (chunk: string, index: number) => Promise<TArtifact>

export type VoiceTtsPlayer<TArtifact> = (artifact: TArtifact, index: number) => Promise<void>

export function normalizeCapabilityFormats(
  value: boolean | string[] | undefined,
  legacyFallback: string[]
): string[] {
  if (Array.isArray(value)) {
    return normalizeAudioFormats(value)
  }
  return value ? normalizeAudioFormats(legacyFallback) : []
}

export async function normalizeVoiceRecordingAudio(
  recording: VoiceRecordingInput,
  acceptedFormats: string[]
): Promise<NormalizedVoiceRecording> {
  const audioBase64 = recording.audioBase64 || ''
  const fileName = recording.fileName?.trim() || 'chrome_voice.webm'
  if (!audioBase64.trim()) {
    throw new Error(chrome.i18n.getMessage('audioCaptureMissingData'))
  }
  const accepted = normalizeAudioFormats(acceptedFormats)
  const sourceFormat = recordingAudioFormat(fileName, recording.mimeType)
  if (!sourceFormat) {
    throw new Error(
      chrome.i18n.getMessage('audioFormatNotSupported', ['unknown', accepted.join(', ') || 'none'])
    )
  }

  const acceptedFormat = acceptedAudioFormat(sourceFormat, accepted)
  if (acceptedFormat) {
    return {
      audioBase64,
      fileName: ensureAudioFileExtension(fileName, acceptedFormat)
    }
  }
  if (!accepted.includes('wav')) {
    throw new Error(
      chrome.i18n.getMessage('audioFormatNotSupported', [
        sourceFormat,
        accepted.join(', ') || 'none'
      ])
    )
  }

  const mimeType = recording.mimeType || audioMimeFromName(fileName) || 'audio/webm'
  const sourceBytes = base64ToUint8Array(audioBase64)
  const sourceBlob = new Blob([sourceBytes], { type: mimeType })
  const wavBytes = await audioBlobToWavBytes(sourceBlob)
  return {
    audioBase64: uint8ArrayToBase64(wavBytes),
    fileName: `${fileStem(fileName) || 'chrome_voice'}.wav`
  }
}

function normalizeAudioFormats(formats: string[]): string[] {
  const seen = new Set<string>()
  const normalized: string[] = []
  for (const format of formats) {
    const next = normalizeAudioFormat(format)
    if (next && !seen.has(next)) {
      seen.add(next)
      normalized.push(next)
    }
  }
  return normalized
}

function acceptedAudioFormat(sourceFormat: string, acceptedFormats: string[]): string {
  for (const alias of audioFormatAliases(sourceFormat)) {
    if (acceptedFormats.includes(alias)) {
      return alias
    }
  }
  return ''
}

function audioFormatAliases(format: string): string[] {
  switch (format) {
    case 'm4a':
    case 'mp4':
      return ['mp4', 'm4a']
    case 'mp3':
      return ['mp3', 'mpeg', 'mpga']
    default:
      return [format]
  }
}

function recordingAudioFormat(fileName: string, mimeType?: string): string {
  return normalizeAudioFormat(mimeType || '') || normalizeAudioFormat(fileExtension(fileName))
}

function normalizeAudioFormat(format: string): string {
  const normalized = format.trim().toLowerCase().split(';', 1)[0]
  switch (normalized) {
    case 'audio/webm':
    case 'webm':
      return 'webm'
    case 'audio/ogg':
    case 'audio/oga':
    case 'oga':
    case 'ogg':
      return 'ogg'
    case 'audio/mp4':
    case 'mp4':
      return 'mp4'
    case 'audio/x-m4a':
    case 'm4a':
      return 'm4a'
    case 'audio/mpeg':
    case 'audio/mp3':
    case 'mpeg':
    case 'mpga':
    case 'mp3':
      return 'mp3'
    case 'audio/wav':
    case 'audio/x-wav':
    case 'wav':
      return 'wav'
    case 'audio/flac':
    case 'flac':
      return 'flac'
    case 'audio/opus':
    case 'opus':
      return 'opus'
    case 'audio/pcm':
    case 'audio/l16':
    case 'pcm':
      return 'pcm'
    default:
      return ''
  }
}

function ensureAudioFileExtension(fileName: string, format: string): string {
  const extension = normalizeAudioFormat(fileExtension(fileName))
  if (extension === format) {
    return fileName
  }

  return `${fileStem(fileName) || 'chrome_voice'}.${preferredAudioExtension(format)}`
}

function preferredAudioExtension(format: string): string {
  switch (format) {
    case 'mpeg':
    case 'mpga':
      return 'mp3'
    case 'mp4':
      return 'mp4'
    case 'm4a':
      return 'm4a'
    default:
      return format
  }
}

async function audioBlobToWavBytes(blob: Blob): Promise<Uint8Array> {
  const AudioContextCtor = globalThis.AudioContext
  if (!AudioContextCtor) {
    throw new Error(chrome.i18n.getMessage('audioConversionFailed'))
  }
  const context = new AudioContextCtor()
  try {
    const audioBuffer = await context.decodeAudioData(await blob.arrayBuffer())
    return audioBufferToWavBytes(audioBuffer)
  } catch (error) {
    throw new Error(
      chrome.i18n.getMessage('wavConversionFailed', [
        error instanceof Error ? error.message : String(error)
      ])
    )
  } finally {
    await context.close().catch(() => undefined)
  }
}

function audioBufferToWavBytes(audioBuffer: AudioBuffer): Uint8Array<ArrayBuffer> {
  const channelCount = Math.min(audioBuffer.numberOfChannels || 1, 2)
  const sampleRate = audioBuffer.sampleRate
  const bytesPerSample = 2
  const blockAlign = channelCount * bytesPerSample
  const dataSize = audioBuffer.length * blockAlign
  const bytes = new Uint8Array(44 + dataSize)
  const view = new DataView(bytes.buffer)
  let offset = 0

  const writeString = (value: string) => {
    for (let index = 0; index < value.length; index += 1) {
      view.setUint8(offset, value.charCodeAt(index))
      offset += 1
    }
  }

  writeString('RIFF')
  view.setUint32(offset, 36 + dataSize, true)
  offset += 4
  writeString('WAVE')
  writeString('fmt ')
  view.setUint32(offset, 16, true)
  offset += 4
  view.setUint16(offset, 1, true)
  offset += 2
  view.setUint16(offset, channelCount, true)
  offset += 2
  view.setUint32(offset, sampleRate, true)
  offset += 4
  view.setUint32(offset, sampleRate * blockAlign, true)
  offset += 4
  view.setUint16(offset, blockAlign, true)
  offset += 2
  view.setUint16(offset, bytesPerSample * 8, true)
  offset += 2
  writeString('data')
  view.setUint32(offset, dataSize, true)
  offset += 4

  const channels = Array.from({ length: channelCount }, (_unused, index) =>
    audioBuffer.getChannelData(index)
  )
  for (let sampleIndex = 0; sampleIndex < audioBuffer.length; sampleIndex += 1) {
    for (let channelIndex = 0; channelIndex < channelCount; channelIndex += 1) {
      const sample = Math.max(-1, Math.min(1, channels[channelIndex][sampleIndex] || 0))
      view.setInt16(offset, sample < 0 ? sample * 0x8000 : sample * 0x7fff, true)
      offset += bytesPerSample
    }
  }

  return bytes
}

function base64ToUint8Array(base64: string): Uint8Array<ArrayBuffer> {
  const binary = atob(base64)
  const bytes = new Uint8Array(binary.length)
  for (let index = 0; index < binary.length; index += 1) {
    bytes[index] = binary.charCodeAt(index)
  }
  return bytes
}

function uint8ArrayToBase64(bytes: Uint8Array): string {
  const chunkSize = 0x8000
  let binary = ''
  for (let offset = 0; offset < bytes.length; offset += chunkSize) {
    binary += String.fromCharCode(...bytes.subarray(offset, offset + chunkSize))
  }
  return btoa(binary)
}

function fileExtension(fileName: string): string {
  return fileName.includes('.') ? fileName.split('.').pop()?.toLowerCase() || '' : ''
}

function fileStem(fileName: string): string {
  return fileName.includes('.') ? fileName.slice(0, fileName.lastIndexOf('.')) : fileName
}

export function isAudioResource(resource: Resource): boolean {
  const tags = Array.isArray(resource.tags) ? resource.tags : []
  return (
    tags.some((tag) => tag.toLowerCase() === 'audio') ||
    Boolean(resource.mime_type?.toLowerCase().startsWith('audio/')) ||
    Boolean(audioMimeFromName(resource.name))
  )
}

export async function playAudioArtifact(resource: Resource): Promise<void> {
  if (!resource.blob) {
    throw new Error('Audio artifact is missing inline data.')
  }
  const mimeType = resource.mime_type || audioMimeFromName(resource.name) || 'audio/mpeg'
  const audio = new Audio(`data:${mimeType};base64,${resource.blob}`)
  await new Promise<void>((resolve, reject) => {
    let settled = false
    const settle = (error?: unknown) => {
      if (settled) {
        return
      }
      settled = true
      if (error) {
        reject(error instanceof Error ? error : new Error(String(error)))
      } else {
        resolve()
      }
    }
    audio.onended = () => settle()
    audio.onerror = () => settle(new Error(chrome.i18n.getMessage('audioPlaybackFailed')))
    void audio.play().catch(settle)
  })
}

function audioMimeFromName(name: string): string | null {
  const extension = name.includes('.') ? name.split('.').pop()?.toLowerCase() : ''
  switch (extension) {
    case 'flac':
      return 'audio/flac'
    case 'm4a':
    case 'mp4':
      return 'audio/mp4'
    case 'ogg':
    case 'oga':
      return 'audio/ogg'
    case 'opus':
      return 'audio/opus'
    case 'wav':
      return 'audio/wav'
    case 'webm':
      return 'audio/webm'
    case 'mp3':
    case 'mpeg':
    case 'mpga':
      return 'audio/mpeg'
    default:
      return null
  }
}

export function prepareVoiceTtsText(text: string): string {
  return text
    .split(/\r?\n/)
    .map(getPlainText)
    .map((line) =>
      Array.from(line)
        .map(normalizeVoiceTtsCharacter)
        .filter((character): character is string => Boolean(character))
        .join('')
        .replace(/\s+/g, ' ')
        .trim()
    )
    .filter(Boolean)
    .join('\n')
}

export function normalTextForSpeech(text: string | undefined): string {
  return text ? splitLegacyThoughtTextForSpeech(text).text.trim() : ''
}

function splitLegacyThoughtTextForSpeech(content: string): { text: string; thinkingText: string } {
  const thinkingParts: string[] = []
  const text = content
    .replace(/<think(?:ing)?\b[^>]*>([\s\S]*?)<\/think(?:ing)?>/gi, (_match, thinking) => {
      if (typeof thinking === 'string' && thinking.trim()) {
        thinkingParts.push(thinking.trim())
      }
      return ''
    })
    .trim()
  return { text, thinkingText: thinkingParts.join('\n\n').trim() }
}

function normalizeVoiceTtsCharacter(character: string): string | null {
  const codePoint = character.codePointAt(0) || 0
  if (
    codePoint === 0x200d ||
    codePoint === 0xfe0e ||
    codePoint === 0xfe0f ||
    (codePoint >= 0x2600 && codePoint <= 0x27bf) ||
    (codePoint >= 0x1f000 && codePoint <= 0x1faff) ||
    (codePoint >= 0xe0020 && codePoint <= 0xe007f)
  ) {
    return null
  }
  if (character === '\r' || character === '\u00a0') {
    return ' '
  }
  if (codePoint === 0x2013 || codePoint === 0x2014) {
    return ','
  }
  return character
}

export function splitVoiceTtsText(text: string, maxChars: number): string[] {
  if (maxChars <= 0) {
    return []
  }
  const chunks: string[] = []
  let currentLines: string[] = []
  let currentChars = 0
  for (const line of text
    .split(/\r?\n/)
    .map((value) => value.trim())
    .filter(Boolean)) {
    const lineChars = Array.from(line).length
    if (lineChars > maxChars) {
      pushVoiceTtsLines(chunks, currentLines)
      currentLines = []
      currentChars = 0
      chunks.push(...splitLongVoiceTtsLine(line, maxChars))
      continue
    }

    const separatorChars = currentLines.length ? 1 : 0
    const nextChars = currentChars + separatorChars + lineChars
    if (
      currentLines.length &&
      (nextChars > maxChars ||
        currentLines.length >= voiceTtsMaxShortLines ||
        (currentLines.length >= 2 && currentChars >= voiceTtsShortChunkChars))
    ) {
      pushVoiceTtsLines(chunks, currentLines)
      currentLines = []
      currentChars = 0
    }

    currentChars += (currentLines.length ? 1 : 0) + lineChars
    currentLines.push(line)
  }
  pushVoiceTtsLines(chunks, currentLines)
  return chunks
}

export async function playVoiceTtsPipeline<TArtifact>(
  chunks: string[],
  synthesize: VoiceTtsSynthesizer<TArtifact>,
  play: VoiceTtsPlayer<TArtifact>
): Promise<void> {
  if (!chunks.length) {
    return
  }

  let current = await synthesize(chunks[0], 0)
  for (let index = 1; index < chunks.length; index += 1) {
    const next = synthesize(chunks[index], index)
    next.catch(() => undefined)
    await play(current, index - 1)
    current = await next
  }
  await play(current, chunks.length - 1)
}

function pushVoiceTtsLines(chunks: string[], lines: string[]): void {
  if (lines.length) {
    chunks.push(lines.join('\n'))
  }
}

function splitLongVoiceTtsLine(line: string, maxChars: number): string[] {
  const chunks: string[] = []
  let current = ''
  const boundaryThreshold = Math.min(voiceTtsPreferredChunkChars, Math.ceil(maxChars / 2))
  for (const character of Array.from(line)) {
    if (Array.from(current).length >= maxChars) {
      chunks.push(current.trim())
      current = ''
    }
    current += character
    if (isTtsSentenceBoundary(character) && Array.from(current).length >= boundaryThreshold) {
      chunks.push(current.trim())
      current = ''
    }
  }
  if (current.trim()) {
    chunks.push(current.trim())
  }
  return chunks
}

function isTtsSentenceBoundary(character: string): boolean {
  return ['.', '!', '?', '。', '！', '？'].includes(character)
}
