/**
 * The New chat backdrop: a faint dot grid that a wave from Anda reveals. Dots
 * swell under the pointer and leave a short trail; a click draws a shape in
 * dots (a heart, a paw, a panda, bamboo, a sparkle), and its outline then spreads
 * out into a ring. The loop only runs while something moves.
 */

export const SPACING = 15
const DOT_RADIUS = 0.9
const BASE_ALPHA = 0.75
const LENS_RADIUS = 110
// The lens drifts after the pointer and its trail lingers.
const LENS_FOLLOW_MS = 140
const TRAIL_MS = 900
const MASK_SIZE = 72
const OUTLINE_STEPS = 96
const MAX_STAMPS = 8
// px per ms: the grid takes about two seconds to unfold from Anda.
const REVEAL_SPEED = 0.45
const REVEAL_EDGE = 220
// A stamp swells in, breathes once, then its outline drifts away and fades.
const RING_START = 1300
export const STAMP_MS = 4200

type Rgb = [number, number, number]
type Ink = 'rose' | 'ink' | 'leaf' | 'amber' | 'line' | 'pandaFace' | 'pandaEyes' | 'pandaRim'
/** A shape paints up to three tones; each takes one of its inks. */
type Tone = 0 | 1 | 2
/** Fills `path` in `tone`, or strokes it `stroke` units wide. */
type Paint = (path: Path2D, tone?: Tone, stroke?: number) => void

interface Shape {
  /** Grid cells across the shape's 100-unit box. */
  cells: number
  /** The ink of each tone the shape paints. */
  inks: Ink[]
  /** The ink of the outline once it spreads. */
  ring: Ink
  draw(paint: Paint): void
}

interface Mask {
  alpha: Float32Array
  /** Each tone's coverage, premultiplied by alpha. */
  tones: Float32Array[]
  radii: Float32Array
  mean: number
  max: number
}

interface Stamp {
  x: number
  y: number
  born: number
  shape: Shape
  mask: Mask
}

export interface StampPose {
  /** Scale of the filled shape. */
  scale: number
  /** Opacity of the filled shape. */
  fill: number
  /** Opacity of the spreading outline; 0 before it detaches. */
  ring: number
  /** How far the outline has grown, as a multiple of the shape. */
  spread: number
  /** 0 = the shape's own outline, 1 = a circle. */
  morph: number
  /** Half the outline's thickness in px. */
  width: number
}

const clamp = (value: number, min = 0, max = 1) => Math.min(max, Math.max(min, value))
const easeOutSine = (x: number) => Math.sin((Math.PI * x) / 2)
const easeInOutSine = (x: number) => (1 - Math.cos(Math.PI * x)) / 2
// Eases out with a 4% overshoot.
const easeOutBack = (x: number) => 1 + 2 * (x - 1) ** 3 + (x - 1) ** 2

/** A stamp's pose `t` ms after the click, or null once it has faded. */
export function stampPose(t: number): StampPose | null {
  if (t < 0 || t >= STAMP_MS) return null
  const breath = t > 800 && t < 1500 ? 1 + 0.06 * Math.sin((Math.PI * (t - 800)) / 700) : 1
  const fill = t < 1500 ? easeOutSine(clamp(t / 360)) : 1 - easeInOutSine(clamp((t - 1500) / 900))
  const p = clamp((t - RING_START) / (STAMP_MS - RING_START))
  const ring = t < RING_START ? 0 : easeInOutSine(clamp((t - RING_START) / 450)) * (1 - p) ** 1.3
  return {
    scale: (0.55 + 0.45 * easeOutBack(clamp(t / 700))) * breath,
    fill,
    ring,
    spread: 1 + 2.2 * easeOutSine(p),
    morph: easeInOutSine(clamp(p * 1.4)),
    width: SPACING * (0.8 + 0.8 * p)
  }
}

/** Bilinear lookup in a square mask; `u`/`v` run 0..1 across it. */
export function sampleMask(mask: Float32Array, size: number, u: number, v: number): number {
  const x = u * size - 0.5
  const y = v * size - 0.5
  if (x < -1 || y < -1 || x > size || y > size) return 0
  const x0 = Math.floor(x)
  const y0 = Math.floor(y)
  const fx = x - x0
  const fy = y - y0
  const at = (cx: number, cy: number) =>
    cx < 0 || cy < 0 || cx >= size || cy >= size ? 0 : mask[cy * size + cx]
  const top = at(x0, y0) * (1 - fx) + at(x0 + 1, y0) * fx
  const bottom = at(x0, y0 + 1) * (1 - fx) + at(x0 + 1, y0 + 1) * fx
  return top * (1 - fy) + bottom * fy
}

/**
 * The shape's outline seen from the mask's centre: for each of `steps` angles,
 * the farthest covered point, as a fraction of the mask's width. Rays that miss
 * (between a paw's toes) take their neighbours' radius, and the result is
 * smoothed so the spreading ring stays round-edged.
 */
export function outlineRadii(alpha: Float32Array, size: number, steps: number): Float32Array {
  const raw = new Float32Array(steps).fill(-1)
  for (let i = 0; i < steps; i++) {
    const angle = (i / steps) * Math.PI * 2
    const dx = Math.cos(angle)
    const dy = Math.sin(angle)
    for (let r = 0.71; r > 0; r -= 0.25 / size) {
      if (sampleMask(alpha, size, 0.5 + dx * r, 0.5 + dy * r) >= 0.5) {
        raw[i] = r
        break
      }
    }
  }
  if (raw.every((r) => r < 0)) return raw.fill(0)
  for (let i = 0; i < steps; i++) {
    if (raw[i] >= 0) continue
    let before = 1
    while (raw[(i - before + steps) % steps] < 0) before++
    let after = 1
    while (raw[(i + after) % steps] < 0) after++
    const a = raw[(i - before + steps) % steps]
    const b = raw[(i + after) % steps]
    raw[i] = a + ((b - a) * before) / (before + after)
  }
  const radii = new Float32Array(steps)
  for (let i = 0; i < steps; i++) {
    let sum = 0
    for (let k = -2; k <= 2; k++) sum += raw[(i + k + steps) % steps]
    radii[i] = sum / 5
  }
  return radii
}

/** `#rgb`, `#rrggbb` or `rgb(r, g, b)` as numbers. */
export function parseColor(value: string): Rgb | null {
  const text = value.trim()
  const hex = /^#([0-9a-f]{3}|[0-9a-f]{6})$/i.exec(text)?.[1]
  if (hex) {
    const full = hex.length === 3 ? [...hex].map((c) => c + c).join('') : hex
    return [0, 2, 4].map((i) => parseInt(full.slice(i, i + 2), 16)) as Rgb
  }
  const rgb = /^rgba?\(\s*([\d.]+)[\s,]+([\d.]+)[\s,]+([\d.]+)/i.exec(text)
  return rgb ? [Number(rgb[1]), Number(rgb[2]), Number(rgb[3])] : null
}

const css = ([r, g, b]: Rgb) => `rgb(${Math.round(r)} ${Math.round(g)} ${Math.round(b)})`
const mix = (a: Rgb, b: Rgb, t: number): Rgb => [
  a[0] + (b[0] - a[0]) * t,
  a[1] + (b[1] - a[1]) * t,
  a[2] + (b[2] - a[2]) * t
]

function ellipse(x: number, y: number, rx: number, ry: number, rotation: number): Path2D {
  const path = new Path2D()
  path.ellipse(x, y, rx, ry, rotation, 0, Math.PI * 2)
  return path
}

/** A leaf from its stem at (x0, y0) to its tip, `width` at its widest. */
function leaf(x0: number, y0: number, x1: number, y1: number, width: number): Path2D {
  const length = Math.hypot(x1 - x0, y1 - y0)
  const nx = (-(y1 - y0) / length) * width
  const ny = ((x1 - x0) / length) * width
  const mx = (x0 + x1) / 2
  const my = (y0 + y1) / 2
  const path = new Path2D()
  path.moveTo(x0, y0)
  path.quadraticCurveTo(mx + nx, my + ny, x1, y1)
  path.quadraticCurveTo(mx - nx, my - ny, x0, y0)
  return path
}

const heart: Shape = {
  cells: 13,
  inks: ['rose'],
  ring: 'rose',
  draw: (paint) =>
    paint(
      new Path2D(
        'M50 89C47 86 6 63 6 36C6 22 17 12 30 12C39 12 46 17 50 25C54 17 61 12 70 12C83 12 94 22 94 36C94 63 53 86 50 89Z'
      )
    )
}

const SHAPES: Shape[] = [
  heart,
  {
    cells: 14,
    inks: ['ink'],
    ring: 'ink',
    draw: (paint) => {
      paint(ellipse(50, 66, 21, 16, 0))
      paint(ellipse(21, 44, 7.5, 9.5, -0.45))
      paint(ellipse(37, 25, 8, 10, -0.15))
      paint(ellipse(63, 25, 8, 10, 0.15))
      paint(ellipse(79, 44, 7.5, 9.5, 0.45))
    }
  },
  {
    // A panda drawn for dots: round ears, an outlined face, slanted eye
    // patches and a nose big enough to read at this size.
    cells: 17,
    inks: ['pandaFace', 'pandaEyes', 'pandaRim'],
    ring: 'line',
    draw: (paint) => {
      paint(ellipse(21, 25, 14, 13, 0), 2)
      paint(ellipse(79, 25, 14, 13, 0), 2)
      const head = ellipse(50, 57, 41, 34, 0)
      paint(head, 0)
      paint(head, 2, 6)
      paint(ellipse(34, 56, 8.5, 12.5, 0.55), 1)
      paint(ellipse(66, 56, 8.5, 12.5, -0.55), 1)
      paint(ellipse(50, 74, 7, 5, 0), 1)
    }
  },
  {
    cells: 15,
    inks: ['leaf'],
    ring: 'leaf',
    draw: (paint) => {
      paint(leaf(50, 94, 8, 30, 16))
      paint(leaf(50, 94, 52, 4, 16))
      paint(leaf(50, 94, 93, 34, 16))
    }
  },
  {
    cells: 12,
    inks: ['amber'],
    ring: 'amber',
    draw: (paint) =>
      paint(
        new Path2D('M50 6C53 34 66 47 94 50C66 53 53 66 50 94C47 66 34 53 6 50C34 47 47 34 50 6Z')
      )
  }
]

const INKS: Ink[] = ['rose', 'ink', 'leaf', 'amber', 'line', 'pandaFace', 'pandaEyes', 'pandaRim']
const LENS_STEPS = 8

function rasterize(shape: Shape): Mask {
  const canvas = document.createElement('canvas')
  canvas.width = canvas.height = MASK_SIZE
  const context = canvas.getContext('2d', { willReadFrequently: true })!
  context.scale(MASK_SIZE / 100, MASK_SIZE / 100)
  // Each tone paints one colour channel, so one pass yields every mask.
  const colors = ['#f00', '#0f0', '#00f']
  shape.draw((path, tone = 0, stroke) => {
    if (stroke) {
      context.strokeStyle = colors[tone]
      context.lineWidth = stroke
      context.stroke(path)
    } else {
      context.fillStyle = colors[tone]
      context.fill(path)
    }
  })
  const data = context.getImageData(0, 0, MASK_SIZE, MASK_SIZE).data
  const alpha = new Float32Array(MASK_SIZE * MASK_SIZE)
  const tones = shape.inks.map(() => new Float32Array(MASK_SIZE * MASK_SIZE))
  for (let i = 0; i < alpha.length; i++) {
    alpha[i] = data[i * 4 + 3] / 255
    tones.forEach((tone, k) => (tone[i] = (data[i * 4 + k] / 255) * alpha[i]))
  }
  const radii = outlineRadii(alpha, MASK_SIZE, OUTLINE_STEPS)
  const mean = radii.reduce((sum, r) => sum + r, 0) / radii.length
  return { alpha, tones, radii, mean, max: Math.max(...radii) }
}

const NOT_BACKGROUND =
  'button, a, input, textarea, select, label, [contenteditable], [role="menu"], [role="dialog"], [role="listbox"], .composer-footer, .workspace-header, .status-banner, .uncertain-banner'

export interface WelcomeField {
  /** Re-reads the theme's colours. */
  restyle(): void
  destroy(): void
}

/**
 * Draws the field on `canvas`, which sits behind `host`'s content. The reveal
 * starts from `origin`, and a tap on it always sends a heart.
 */
export function createWelcomeField(
  canvas: HTMLCanvasElement,
  host: HTMLElement,
  origin: () => HTMLElement | null,
  options: { reducedMotion: boolean }
): WelcomeField {
  const context = canvas.getContext('2d')!
  const masks = new Map<Shape, Mask>()
  const base = document.createElement('canvas')
  const baseContext = base.getContext('2d')!
  let width = 0
  let height = 0
  let ratio = 1
  let cols = 0
  let rows = 0
  let gridX = 0
  let gridY = 0
  let xs = new Float32Array(0)
  let ys = new Float32Array(0)
  let fade = new Float32Array(0)
  let reveal = new Float32Array(0)
  let energy = new Float32Array(0)
  let stampLevel = new Float32Array(0)
  let stampInk = new Uint8Array(0)
  let baseDirty = true
  let palette = readPalette()
  let stamps: Stamp[] = []
  let shapeIndex = 0
  let revealStart = options.reducedMotion ? -Infinity : performance.now()
  let frame = 0
  let last = 0
  const pointer = { active: false, x: 0, y: 0, tx: 0, ty: 0 }

  function readPalette() {
    const style = getComputedStyle(canvas)
    const dark = document.documentElement.classList.contains('dark')
    const read = (name: string, fallback: Rgb) =>
      parseColor(style.getPropertyValue(name)) ?? fallback
    const faint = read('--desk-faint', [193, 193, 187])
    const accent = read('--desk-accent', [100, 132, 104])
    const text = read('--desk-text', [41, 41, 38])
    const background = read('--desk-bg', [252, 252, 251])
    // Painting the background colour hides the grid's own dots there.
    const clear = { color: css(background), alpha: 1 }
    const ink = (alpha: number) => ({ color: css(text), alpha })
    const inks: Record<Ink, { color: string; alpha: number }> = {
      rose: { color: dark ? '#fb7185' : '#e11d48', alpha: 0.9 },
      ink: ink(0.85),
      leaf: { color: dark ? '#34d399' : '#059669', alpha: 0.9 },
      amber: { color: dark ? '#fbbf24' : '#d97706', alpha: 0.95 },
      line: ink(0.5),
      // Like the mark, the panda keeps a light face and dark patches: on a
      // light page its face is left blank, on a dark one its eyes are.
      pandaFace: dark ? ink(0.6) : clear,
      pandaEyes: dark ? clear : ink(0.85),
      pandaRim: ink(dark ? 0.3 : 0.8)
    }
    return {
      dot: css(faint),
      lens: Array.from({ length: LENS_STEPS + 1 }, (_, i) =>
        css(mix(faint, accent, i / LENS_STEPS))
      ),
      inks: INKS.map((ink) => inks[ink])
    }
  }

  function originPoint(): [number, number] {
    const element = origin()
    if (!element) return [width / 2, height * 0.4]
    const box = element.getBoundingClientRect()
    const own = canvas.getBoundingClientRect()
    return [box.left + box.width / 2 - own.left, box.top + box.height / 2 - own.top]
  }

  function layout() {
    const box = canvas.getBoundingClientRect()
    width = box.width
    height = box.height
    ratio = window.devicePixelRatio || 1
    canvas.width = base.width = Math.max(1, Math.round(width * ratio))
    canvas.height = base.height = Math.max(1, Math.round(height * ratio))
    cols = Math.floor(width / SPACING) + 1
    rows = Math.floor(height / SPACING) + 1
    gridX = (width - (cols - 1) * SPACING) / 2
    gridY = (height - (rows - 1) * SPACING) / 2
    const count = cols * rows
    xs = new Float32Array(count)
    ys = new Float32Array(count)
    fade = new Float32Array(count)
    reveal = new Float32Array(count)
    energy = new Float32Array(count)
    stampLevel = new Float32Array(count)
    stampInk = new Uint8Array(count)
    const [originX, originY] = originPoint()
    // The grid gathers around Anda and thins out towards the edges.
    const rx = Math.max(width * 0.5, 300)
    const ry = Math.max(height * 0.55, 260)
    for (let row = 0; row < rows; row++) {
      for (let col = 0; col < cols; col++) {
        const i = row * cols + col
        const x = gridX + col * SPACING
        const y = gridY + row * SPACING
        xs[i] = x
        ys[i] = y
        const d = Math.hypot((x - originX) / rx, (y - originY) / ry)
        const t = clamp((d - 0.3) / 0.7)
        fade[i] = 1 - t * t * (3 - 2 * t)
        reveal[i] = Math.hypot(x - originX, y - originY)
      }
    }
    baseDirty = true
    kick()
  }

  function drawDots(target: CanvasRenderingContext2D, alphaOf: (i: number) => number) {
    // Few alpha levels, so the whole grid takes a handful of fills.
    const levels = 6
    const paths = Array.from({ length: levels }, () => new Path2D())
    for (let i = 0; i < xs.length; i++) {
      const level = Math.round(alphaOf(i) * levels)
      if (level <= 0) continue
      const path = paths[level - 1]
      path.moveTo(xs[i] + DOT_RADIUS, ys[i])
      path.arc(xs[i], ys[i], DOT_RADIUS, 0, Math.PI * 2)
    }
    target.fillStyle = palette.dot
    paths.forEach((path, i) => {
      target.globalAlpha = (BASE_ALPHA * (i + 1)) / levels
      target.fill(path)
    })
    target.globalAlpha = 1
  }

  function maskFor(shape: Shape): Mask {
    let mask = masks.get(shape)
    if (!mask) masks.set(shape, (mask = rasterize(shape)))
    return mask
  }

  function paintStamp(stamp: Stamp, pose: StampPose) {
    const size = stamp.shape.cells * SPACING
    const { mask } = stamp
    const fillHalf = (size * pose.scale) / 2
    const ringScale = size * pose.spread
    const reach = Math.max(
      pose.fill > 0 ? fillHalf * 1.42 : 0,
      pose.ring > 0 ? ringScale * mask.max + pose.width : 0
    )
    const col0 = Math.max(0, Math.floor((stamp.x - reach - gridX) / SPACING))
    const col1 = Math.min(cols - 1, Math.ceil((stamp.x + reach - gridX) / SPACING))
    const row0 = Math.max(0, Math.floor((stamp.y - reach - gridY) / SPACING))
    const row1 = Math.min(rows - 1, Math.ceil((stamp.y + reach - gridY) / SPACING))
    const inks = stamp.shape.inks.map((ink) => INKS.indexOf(ink))
    const ringInk = INKS.indexOf(stamp.shape.ring)
    for (let row = row0; row <= row1; row++) {
      for (let col = col0; col <= col1; col++) {
        const i = row * cols + col
        const dx = xs[i] - stamp.x
        const dy = ys[i] - stamp.y
        let level = 0
        let ink = inks[0]
        if (pose.fill > 0) {
          const u = dx / (size * pose.scale) + 0.5
          const v = dy / (size * pose.scale) + 0.5
          const covered = sampleMask(mask.alpha, MASK_SIZE, u, v)
          if (covered > 0.02) {
            level = covered * pose.fill
            let strongest = 0
            mask.tones.forEach((tone, k) => {
              const value = inks.length > 1 ? sampleMask(tone, MASK_SIZE, u, v) : 1
              if (value > strongest) {
                strongest = value
                ink = inks[k]
              }
            })
          }
        }
        if (pose.ring > 0) {
          const step =
            ((Math.atan2(dy, dx) / (Math.PI * 2)) * OUTLINE_STEPS + OUTLINE_STEPS) % OUTLINE_STEPS
          const k = Math.floor(step)
          const shapeRadius =
            mask.radii[k] + (mask.radii[(k + 1) % OUTLINE_STEPS] - mask.radii[k]) * (step - k)
          const radius = ringScale * (shapeRadius + (mask.mean - shapeRadius) * pose.morph)
          const ring = clamp(1 - Math.abs(Math.hypot(dx, dy) - radius) / pose.width) * pose.ring
          if (ring > level) {
            level = ring
            ink = ringInk
          }
        }
        if (level > stampLevel[i]) {
          stampLevel[i] = level
          stampInk[i] = ink
        }
      }
    }
  }

  function draw(now: number) {
    frame = 0
    const dt = last ? Math.min(64, now - last) : 16
    last = now
    if ((window.devicePixelRatio || 1) !== ratio) layout()
    let busy = false

    // The lens trails the pointer a little.
    if (pointer.active) {
      const follow = 1 - Math.exp(-dt / LENS_FOLLOW_MS)
      pointer.x += (pointer.tx - pointer.x) * follow
      pointer.y += (pointer.ty - pointer.y) * follow
      if (Math.hypot(pointer.tx - pointer.x, pointer.ty - pointer.y) > 0.3) busy = true
    }
    const decay = Math.exp(-dt / TRAIL_MS)
    for (let i = 0; i < energy.length; i++) {
      let value = energy[i] * decay
      if (pointer.active) {
        const d = Math.hypot(xs[i] - pointer.x, ys[i] - pointer.y)
        if (d < LENS_RADIUS) value = Math.max(value, (1 - d / LENS_RADIUS) ** 2)
      }
      if (value < 0.004) value = 0
      if (value !== energy[i]) busy = true
      energy[i] = value
    }

    context.setTransform(ratio, 0, 0, ratio, 0, 0)
    context.clearRect(0, 0, width, height)
    const front = (now - revealStart) * REVEAL_SPEED
    const revealing = front < Math.hypot(width, height) + REVEAL_EDGE
    if (revealing) {
      busy = true
      drawDots(context, (i) => fade[i] * clamp((front - reveal[i]) / REVEAL_EDGE))
    } else {
      if (baseDirty) {
        baseContext.setTransform(ratio, 0, 0, ratio, 0, 0)
        baseContext.clearRect(0, 0, width, height)
        drawDots(baseContext, (i) => fade[i])
        baseDirty = false
      }
      context.drawImage(base, 0, 0, width, height)
    }

    stampLevel.fill(0)
    stamps = stamps.filter((stamp) => {
      const pose = stampPose(now - stamp.born)
      if (pose) paintStamp(stamp, pose)
      return pose !== null
    })
    if (stamps.length) busy = true

    for (let i = 0; i < xs.length; i++) {
      const level = stampLevel[i]
      // The reveal's front glows as it passes.
      const wave = revealing ? Math.exp(-(((front - reveal[i] - 60) / 70) ** 2)) * 0.5 * fade[i] : 0
      const lens = Math.max(energy[i], wave)
      if (level > 0.03) {
        const ink = palette.inks[stampInk[i]]
        context.globalAlpha = clamp(0.2 + level * 0.8) * ink.alpha
        context.fillStyle = ink.color
        context.beginPath()
        context.arc(xs[i], ys[i], DOT_RADIUS + level * 1.5, 0, Math.PI * 2)
        context.fill()
      } else if (lens > 0.03) {
        context.globalAlpha = clamp(BASE_ALPHA * fade[i] * 0.6 + lens * 0.75)
        context.fillStyle = palette.lens[Math.round(lens * LENS_STEPS)]
        context.beginPath()
        context.arc(xs[i], ys[i], DOT_RADIUS + lens * 1.3, 0, Math.PI * 2)
        context.fill()
      }
    }
    context.globalAlpha = 1

    if (busy) kick()
    else last = 0
  }

  function kick() {
    frame ||= requestAnimationFrame(draw)
  }

  function local(event: PointerEvent): [number, number] {
    const box = canvas.getBoundingClientRect()
    return [event.clientX - box.left, event.clientY - box.top]
  }

  function move(event: PointerEvent) {
    const [x, y] = local(event)
    const inside = x >= 0 && y >= 0 && x <= width && y <= height
    if (inside && !pointer.active) {
      pointer.x = x
      pointer.y = y
    }
    pointer.active = inside
    pointer.tx = x
    pointer.ty = y
    kick()
  }

  function leave() {
    pointer.active = false
    kick()
  }

  function down(event: PointerEvent) {
    const target = event.target
    if (event.button !== 0 || !(target instanceof Element) || target.closest(NOT_BACKGROUND)) {
      return
    }
    const [x, y] = local(event)
    if (x < 0 || y < 0 || x > width || y > height) return
    // Tapping Anda always sends a heart; the background takes turns.
    const shape = origin()?.contains(target) ? heart : SHAPES[shapeIndex++ % SHAPES.length]
    stamps.push({ x, y, born: performance.now(), shape, mask: maskFor(shape) })
    if (stamps.length > MAX_STAMPS) stamps.shift()
    kick()
  }

  const observer = new ResizeObserver(layout)
  observer.observe(canvas)
  if (!options.reducedMotion) {
    host.addEventListener('pointermove', move, { passive: true })
    host.addEventListener('pointerleave', leave)
    host.addEventListener('pointerdown', down)
  }

  return {
    restyle() {
      palette = readPalette()
      baseDirty = true
      kick()
    },
    destroy() {
      cancelAnimationFrame(frame)
      observer.disconnect()
      host.removeEventListener('pointermove', move)
      host.removeEventListener('pointerleave', leave)
      host.removeEventListener('pointerdown', down)
    }
  }
}
