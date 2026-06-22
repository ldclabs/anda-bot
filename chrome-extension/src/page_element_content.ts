const pageElementMemoryKey = '__andaLastRightClickedElement'
const maxTextChars = 200_000
const maxOuterHtmlChars = 500_000
const maxAttributeValueChars = 2_000
const maxAttributes = 80
const listenerKey = '__andaPageElementContentScriptContextMenuListener'

installContextMenuListener()

function capturePageElement(event: MouseEvent) {
  const element = eventTargetElement(event)
  if (!element) {
    return
  }

  try {
    ;(globalThis as Record<string, unknown>)[pageElementMemoryKey] = serializeElement(element)
  } catch (_error) {
    ;(globalThis as Record<string, unknown>)[pageElementMemoryKey] = null
  }
}

function installContextMenuListener() {
  const registry = globalThis as unknown as Record<string, EventListener | undefined>
  const listener = capturePageElement as EventListener
  const previousListener = registry[listenerKey]
  if (previousListener) {
    document.removeEventListener('contextmenu', previousListener, true)
  }
  registry[listenerKey] = listener
  document.addEventListener('contextmenu', listener, true)
}

function eventTargetElement(event: MouseEvent): Element | null {
  const target = event.composedPath?.()[0] || event.target
  if (target instanceof Element) {
    return target
  }
  if (target instanceof Node) {
    return target.parentElement
  }
  return null
}

function serializeElement(element: Element) {
  const htmlElement = element as HTMLElement
  const rect = element.getBoundingClientRect()
  return {
    tagName: element.tagName,
    id: element.id || null,
    className: stringValue((element as HTMLElement).className) || null,
    role: element.getAttribute('role'),
    innerText: trimString(htmlElement.innerText || '', maxTextChars),
    textContent: trimString(element.textContent || '', maxTextChars),
    outerHTML: trimString(element.outerHTML || '', maxOuterHtmlChars),
    attributes: elementAttributes(element),
    xpath: elementXPath(element),
    cssPath: elementCssPath(element),
    pageUrl: location.href,
    pageTitle: document.title || '',
    frameUrl: location.href,
    selectedText: trimString(getSelection()?.toString() || '', maxTextChars),
    rect: {
      x: rect.x,
      y: rect.y,
      width: rect.width,
      height: rect.height,
      top: rect.top,
      right: rect.right,
      bottom: rect.bottom,
      left: rect.left
    },
    capturedAt: Date.now()
  }
}

function elementAttributes(element: Element): Record<string, string> {
  const attributes: Record<string, string> = {}
  for (const attr of Array.from(element.attributes).slice(0, maxAttributes)) {
    attributes[attr.name] = trimString(attr.value, maxAttributeValueChars)
  }
  return attributes
}

function elementXPath(element: Element): string {
  if (element.id) {
    return `//*[@id=${xpathStringLiteral(element.id)}]`
  }

  const segments: string[] = []
  let current: Element | null = element
  while (current && current.nodeType === Node.ELEMENT_NODE) {
    const tag = current.tagName.toLowerCase()
    let index = 1
    let sibling = current.previousElementSibling
    while (sibling) {
      if (sibling.tagName === current.tagName) {
        index += 1
      }
      sibling = sibling.previousElementSibling
    }
    segments.unshift(`${tag}[${index}]`)
    current = current.parentElement
  }
  return segments.length ? `/${segments.join('/')}` : '/'
}

function elementCssPath(element: Element): string {
  if (element.id) {
    return `#${cssEscape(element.id)}`
  }

  const segments: string[] = []
  let current: Element | null = element
  while (current && current.nodeType === Node.ELEMENT_NODE) {
    const tag = current.tagName.toLowerCase()
    let segment = tag
    if (current.classList.length) {
      segment += `.${Array.from(current.classList).slice(0, 3).map(cssEscape).join('.')}`
    }
    const siblings = Array.from(current.parentElement?.children || []).filter(
      (sibling) => sibling.tagName === current?.tagName
    )
    if (siblings.length > 1) {
      segment += `:nth-of-type(${siblings.indexOf(current) + 1})`
    }
    segments.unshift(segment)
    current = current.parentElement
  }
  return segments.join(' > ')
}

function xpathStringLiteral(value: string): string {
  if (!value.includes('"')) {
    return `"${value}"`
  }
  if (!value.includes("'")) {
    return `'${value}'`
  }
  return `concat("${value.replace(/"/g, '", \'"\', "')}")`
}

function cssEscape(value: string): string {
  return typeof CSS !== 'undefined' && CSS.escape
    ? CSS.escape(value)
    : value.replace(/[^a-zA-Z0-9_-]/g, '\\$&')
}

function trimString(value: string, maxChars: number): string {
  return value.length > maxChars ? `${value.slice(0, maxChars)}...` : value
}

function stringValue(value: unknown): string {
  if (typeof value === 'string') {
    return value
  }
  if (value && typeof value === 'object' && 'baseVal' in value) {
    const baseVal = (value as { baseVal?: unknown }).baseVal
    return typeof baseVal === 'string' ? baseVal : ''
  }
  return ''
}
