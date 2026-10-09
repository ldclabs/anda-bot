/**
 * The desktop shell's tooltip: one shared bubble in <body>, shown under (or
 * above) the hovered or keyboard-focused control after a short delay, with an
 * optional shortcut. Moving between controls while one is showing swaps it at
 * once, as native tooltips do. The control keeps its own `aria-label`; the
 * bubble is linked as its description.
 *
 * Use it in place of `title`, which would show a second, native tooltip.
 */
export interface TipOptions {
  text: string
  shortcut?: string
}

const SHOW_DELAY = 450
const SWAP_WINDOW = 300
let bubble: HTMLDivElement | null = null
let owner: HTMLElement | null = null
let timer: ReturnType<typeof setTimeout> | undefined
let hiddenAt = 0

function element(): HTMLDivElement {
  if (!bubble) {
    bubble = document.createElement('div')
    bubble.id = 'desktop-tooltip'
    bubble.className = 'desktop-tooltip'
    bubble.setAttribute('role', 'tooltip')
    bubble.hidden = true
    document.body.append(bubble)
  }
  return bubble
}

function place(target: HTMLElement) {
  const tip = element()
  const rect = target.getBoundingClientRect()
  const width = tip.offsetWidth
  const height = tip.offsetHeight
  const margin = 6
  let top = rect.bottom + margin
  if (top + height > window.innerHeight - 4) top = rect.top - height - margin
  const left = Math.min(
    window.innerWidth - width - 4,
    Math.max(4, rect.left + rect.width / 2 - width / 2)
  )
  tip.style.transform = `translate(${Math.round(left)}px, ${Math.round(top)}px)`
}

function show(target: HTMLElement, options: TipOptions) {
  const tip = element()
  tip.replaceChildren(document.createTextNode(options.text))
  if (options.shortcut) {
    const keys = document.createElement('kbd')
    keys.textContent = options.shortcut
    tip.append(keys)
  }
  tip.hidden = false
  owner = target
  target.setAttribute('aria-describedby', tip.id)
  place(target)
}

function hide(target?: HTMLElement) {
  clearTimeout(timer)
  if (target && owner !== target) return
  if (owner) {
    owner.removeAttribute('aria-describedby')
    hiddenAt = Date.now()
  }
  owner = null
  if (bubble) bubble.hidden = true
}

export function tip(node: HTMLElement, initial: TipOptions | string) {
  let options = typeof initial === 'string' ? { text: initial } : initial
  const schedule = () => {
    clearTimeout(timer)
    if (!options.text) return
    const swap = owner !== null || Date.now() - hiddenAt < SWAP_WINDOW
    timer = setTimeout(() => show(node, options), swap ? 0 : SHOW_DELAY)
  }
  const leave = () => hide(node)
  const focus = () => {
    if (node.matches(':focus-visible')) schedule()
  }
  const key = (event: KeyboardEvent) => {
    if (event.key === 'Escape') hide(node)
  }
  node.addEventListener('pointerenter', schedule)
  node.addEventListener('pointerleave', leave)
  node.addEventListener('pointerdown', leave)
  node.addEventListener('focus', focus)
  node.addEventListener('blur', leave)
  node.addEventListener('keydown', key)
  return {
    update(next: TipOptions | string) {
      options = typeof next === 'string' ? { text: next } : next
      if (owner === node) show(node, options)
    },
    destroy() {
      hide(node)
      node.removeEventListener('pointerenter', schedule)
      node.removeEventListener('pointerleave', leave)
      node.removeEventListener('pointerdown', leave)
      node.removeEventListener('focus', focus)
      node.removeEventListener('blur', leave)
      node.removeEventListener('keydown', key)
    }
  }
}
