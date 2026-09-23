export function normalizeBase64(value: string): string {
  const normalized = value
    .trim()
    .replace(/^data:[^,]*,/i, '')
    .replace(/\s/g, '')
    .replace(/-/g, '+')
    .replace(/_/g, '/')
  return normalized + '='.repeat((4 - (normalized.length % 4)) % 4)
}

export function base64ToBytes(value: string): Uint8Array<ArrayBuffer> {
  const binary = atob(normalizeBase64(value))
  const bytes = new Uint8Array(binary.length)
  for (let index = 0; index < binary.length; index++) bytes[index] = binary.charCodeAt(index)
  return bytes
}

export function bytesToBase64(bytes: Uint8Array): string {
  let binary = ''
  for (let index = 0; index < bytes.length; index += 0x8000) {
    binary += String.fromCharCode(...bytes.subarray(index, index + 0x8000))
  }
  return btoa(binary)
}
