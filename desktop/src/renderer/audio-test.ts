/** Deterministic mono PCM fixture. It never samples a physical microphone. */
export function testTone(seconds = 0.4, sampleRate = 16000): Uint8Array<ArrayBuffer> {
  if (
    !Number.isFinite(seconds) ||
    seconds <= 0 ||
    seconds > 30 ||
    ![16000, 24000, 48000].includes(sampleRate)
  )
    throw new Error('Invalid audio fixture')
  const samples = Math.floor(seconds * sampleRate)
  const bytes = new Uint8Array(44 + samples * 2)
  const view = new DataView(bytes.buffer)
  const text = (offset: number, value: string) =>
    [...value].forEach((c, i) => view.setUint8(offset + i, c.charCodeAt(0)))
  text(0, 'RIFF')
  view.setUint32(4, bytes.length - 8, true)
  text(8, 'WAVE')
  text(12, 'fmt ')
  view.setUint32(16, 16, true)
  view.setUint16(20, 1, true)
  view.setUint16(22, 1, true)
  view.setUint32(24, sampleRate, true)
  view.setUint32(28, sampleRate * 2, true)
  view.setUint16(32, 2, true)
  view.setUint16(34, 16, true)
  text(36, 'data')
  view.setUint32(40, samples * 2, true)
  for (let i = 0; i < samples; i++) {
    const fade = Math.min(1, i / 320, (samples - i) / 320)
    view.setInt16(44 + i * 2, Math.sin((2 * Math.PI * 440 * i) / sampleRate) * 7000 * fade, true)
  }
  return bytes
}
