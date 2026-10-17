import { describe, expect, it } from 'vitest'
import {
  STAMP_MS,
  outlineRadii,
  parseColor,
  sampleMask,
  stampPose
} from '../src/renderer/welcome-field'

function disc(size: number, radius: number, cx = 0.5, cy = 0.5): Float32Array {
  const mask = new Float32Array(size * size)
  for (let y = 0; y < size; y++) {
    for (let x = 0; x < size; x++) {
      const d = Math.hypot((x + 0.5) / size - cx, (y + 0.5) / size - cy)
      mask[y * size + x] = d <= radius ? 1 : 0
    }
  }
  return mask
}

describe('welcome field', () => {
  it('fills a stamp, then lets its outline spread into a ring and fade', () => {
    const start = stampPose(0)!
    expect(start.fill).toBe(0)
    expect(start.ring).toBe(0)

    const held = stampPose(1000)!
    expect(held.fill).toBe(1)
    expect(held.ring).toBe(0)
    expect(held.spread).toBe(1)

    const spreading = stampPose(2600)!
    expect(spreading.ring).toBeGreaterThan(0)
    expect(spreading.fill).toBe(0)
    expect(spreading.spread).toBeGreaterThan(1)

    const late = stampPose(STAMP_MS - 1)!
    expect(late.morph).toBe(1)
    expect(late.ring).toBeLessThan(0.01)
    expect(stampPose(STAMP_MS)).toBeNull()
    expect(stampPose(-1)).toBeNull()
  })

  it('keeps the swell gentle', () => {
    let largest = 0
    for (let t = 0; t < STAMP_MS; t += 10) largest = Math.max(largest, stampPose(t)!.scale)
    expect(largest).toBeLessThan(1.1)
  })

  it('samples a mask bilinearly and treats the outside as empty', () => {
    const mask = new Float32Array(4 * 4).fill(1)
    expect(sampleMask(mask, 4, 0.5, 0.5)).toBe(1)
    expect(sampleMask(mask, 4, -0.5, 0.5)).toBe(0)
    expect(sampleMask(mask, 4, 0, 0.5)).toBeCloseTo(0.5)
  })

  it('traces a shape outline from its centre', () => {
    const radii = outlineRadii(disc(72, 0.3), 72, 48)
    for (const radius of radii) expect(radius).toBeCloseTo(0.3, 1)
  })

  it('bridges rays that miss a shape made of separate parts', () => {
    const size = 72
    const left = disc(size, 0.12, 0.25, 0.5)
    const right = disc(size, 0.12, 0.75, 0.5)
    const mask = left.map((value, i) => Math.max(value, right[i]))
    const radii = outlineRadii(mask, size, 48)
    for (const radius of radii) expect(radius).toBeGreaterThan(0)
    // The rays along the axis reach the far edge of each part.
    expect(radii[0]).toBeCloseTo(0.36, 1)
  })

  it('reads the colours the theme declares', () => {
    expect(parseColor(' #c1c1bb')).toEqual([193, 193, 187])
    expect(parseColor('#abc')).toEqual([170, 187, 204])
    expect(parseColor('rgb(1, 2, 3)')).toEqual([1, 2, 3])
    expect(parseColor('oklch(0.7 0.1 140)')).toBeNull()
  })
})
