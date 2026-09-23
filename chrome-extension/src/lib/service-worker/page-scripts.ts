import type { BrowserActionArgs, BrowserActionResult } from './types'

/**
 * Functions that run inside the page, not in the service worker.
 *
 * Chrome serializes these to source and evaluates them in the tab through
 * `chrome.scripting.executeScript({ func })`, so they close over nothing: every
 * helper they use is nested inside them, even where that repeats a helper the
 * service worker also has. Keep it that way — a reference to module scope here
 * becomes a `ReferenceError` in the page, not a build error.
 *
 * For the same reason these cannot import from `browser-actions.ts`; only the
 * `BrowserActionArgs` / `BrowserActionResult` types cross, and types are erased.
 */

export function resolveInputTarget(args: BrowserActionArgs): Record<string, unknown> {
  function visible(element: Element): boolean {
    const style = window.getComputedStyle(element)
    const rect = element.getBoundingClientRect()
    return (
      style.visibility !== 'hidden' && style.display !== 'none' && rect.width > 0 && rect.height > 0
    )
  }

  function interactable(element: Element): boolean {
    if (!visible(element)) {
      return false
    }
    const rect = element.getBoundingClientRect()
    const hit = element.ownerDocument.elementFromPoint(
      rect.left + Math.max(1, rect.width) / 2,
      rect.top + Math.max(1, rect.height) / 2
    )
    if (!hit) {
      return false
    }
    if (hit === element || element.contains(hit)) {
      return true
    }
    const label = hit.closest('label')
    return (
      label instanceof HTMLLabelElement && (label.control === element || label.contains(element))
    )
  }

  function editableTextInput(element: Element): boolean {
    if (element.tagName === 'TEXTAREA') {
      return !(element as HTMLInputElement).readOnly && !(element as HTMLInputElement).disabled
    }
    if (element.tagName === 'INPUT') {
      const type = (element.getAttribute('type') || 'text').toLowerCase()
      return (
        !(element as HTMLInputElement).readOnly &&
        !(element as HTMLInputElement).disabled &&
        ['email', 'number', 'password', 'search', 'tel', 'text', 'url'].includes(type)
      )
    }
    return (element as HTMLElement).isContentEditable === true
  }

  function preferredMatch(elements: Element[]): Element | null {
    if (args.action === 'type_text') {
      return (
        elements.find((element) => editableTextInput(element) && visible(element)) ||
        elements.find((element) => visible(element)) ||
        elements[0] ||
        null
      )
    }
    return (
      elements.find((element) => interactable(element)) ||
      elements.find((element) => visible(element)) ||
      elements[0] ||
      null
    )
  }

  function deepQuerySelector(
    root: Document | ShadowRoot | Element,
    selector: string
  ): Element | null {
    const direct = preferredMatch(Array.from(root.querySelectorAll(selector)))
    if (direct) {
      return direct
    }
    for (const element of Array.from(root.querySelectorAll('*'))) {
      const shadowRoot = element.shadowRoot
      if (shadowRoot) {
        const found = deepQuerySelector(shadowRoot, selector)
        if (found) {
          return found
        }
      }
      const frameDocument = childFrameDocument(element)
      if (frameDocument) {
        const frameFound = deepQuerySelector(frameDocument, selector)
        if (frameFound) {
          return frameFound
        }
      }
    }
    return null
  }

  function childFrameDocument(element: Element): Document | null {
    if (element.tagName !== 'IFRAME') {
      return null
    }
    try {
      const frame = element as HTMLIFrameElement
      return frame.contentDocument || frame.contentWindow?.document || null
    } catch (_error) {
      return null
    }
  }

  function box(element: Element): Record<string, number> {
    const rect = element.getBoundingClientRect()
    let { x, y, width, height } = rect
    let owner = element.ownerDocument
    let frame = owner.defaultView?.frameElement as HTMLElement | null
    while (frame) {
      const bounds = frame.getBoundingClientRect()
      const scaleX = frame.offsetWidth ? bounds.width / frame.offsetWidth : 1
      const scaleY = frame.offsetHeight ? bounds.height / frame.offsetHeight : 1
      x = bounds.left + (frame.clientLeft + x) * scaleX
      y = bounds.top + (frame.clientTop + y) * scaleY
      width *= scaleX
      height *= scaleY
      owner = frame.ownerDocument
      frame = owner.defaultView?.frameElement as HTMLElement | null
    }
    return { x, y, width, height, left: x, top: y, right: x + width, bottom: y + height }
  }

  function label(element: Element): string {
    return String(
      element.getAttribute('aria-label') ||
        element.getAttribute('title') ||
        element.getAttribute('placeholder') ||
        (element instanceof HTMLElement ? element.innerText : '') ||
        element.textContent ||
        element.tagName
    ).slice(0, 240)
  }

  let element: Element | null = null
  if (args.selector) {
    element = deepQuerySelector(document, args.selector)
    if (!element) {
      throw new Error(`selector not found: ${args.selector}`)
    }
  } else if (
    typeof args.x === 'number' &&
    typeof args.y === 'number' &&
    Number.isFinite(args.x) &&
    Number.isFinite(args.y)
  ) {
    element = document.elementFromPoint(args.x, args.y)
  } else if (args.action === 'type_text') {
    const active = document.activeElement
    const frameActive = active ? childFrameDocument(active)?.activeElement : null
    element =
      frameActive && frameActive !== frameActive.ownerDocument.body
        ? frameActive
        : active && active !== document.body && active !== document.documentElement
          ? active
          : null
  }

  if (!element) {
    if (args.action === 'type_text') {
      throw new Error('type_text requires selector or an active editable element')
    }
    throw new Error('selector or x/y coordinates are required')
  }

  element.scrollIntoView({ block: 'center', inline: 'center' })
  const rect = box(element)
  const useExplicitPoint = !args.selector
  const x =
    useExplicitPoint && typeof args.x === 'number' && Number.isFinite(args.x)
      ? args.x
      : rect.left + Math.max(1, rect.width) / 2
  const y =
    useExplicitPoint && typeof args.y === 'number' && Number.isFinite(args.y)
      ? args.y
      : rect.top + Math.max(1, rect.height) / 2

  if (args.action === 'type_text') {
    if (!editableTextInput(element)) {
      return { native_text_input: false, reason: 'target is not a native text input' }
    }
    return {
      native_text_input: true,
      x,
      y,
      selector: args.selector || null,
      label: label(element),
      bounding_box: box(element)
    }
  }

  return { x, y, label: label(element), bounding_box: box(element) }
}

export function pageActionDispatcher(
  args: BrowserActionArgs
): BrowserActionResult | Promise<BrowserActionResult> {
  const maxText = 12000
  const defaultHtmlLimit = 200000

  function truncate(value: unknown, limit = maxChars(maxText)): string {
    const text = String(value || '')
    return text.length > limit
      ? `${text.slice(0, limit)}\n[truncated ${text.length - limit} chars]`
      : text
  }

  function maxChars(defaultLimit: number): number {
    const requested =
      typeof args.max_chars === 'number' && Number.isFinite(args.max_chars)
        ? args.max_chars
        : defaultLimit
    return Math.max(1000, Math.min(500000, Math.floor(requested)))
  }

  function timeoutMs(defaultTimeout: number): number {
    const requested =
      typeof args.timeout_ms === 'number' && Number.isFinite(args.timeout_ms)
        ? args.timeout_ms
        : defaultTimeout
    return Math.max(100, Math.min(120000, Math.floor(requested)))
  }

  function cssEscape(value: string): string {
    if (window.CSS && CSS.escape) {
      return CSS.escape(value)
    }
    return String(value).replace(/[^a-zA-Z0-9_-]/g, '\\$&')
  }

  function visible(element: Element): boolean {
    const style = window.getComputedStyle(element)
    const rect = element.getBoundingClientRect()
    return (
      style.visibility !== 'hidden' && style.display !== 'none' && rect.width > 0 && rect.height > 0
    )
  }

  function cssPath(element: Element | null): string {
    if (!element) {
      return ''
    }
    if (element.id) {
      return `#${cssEscape(element.id)}`
    }

    const parts: string[] = []
    let current: Element | null = element
    while (current && current.nodeType === Node.ELEMENT_NODE && parts.length < 5) {
      let part = current.nodeName.toLowerCase()
      if (current.classList.length) {
        part += `.${Array.from(current.classList).slice(0, 2).map(cssEscape).join('.')}`
      }
      const parent: Element | null = current.parentElement
      if (parent) {
        const currentNodeName = current.nodeName
        const siblings = Array.from(parent.children).filter(
          (sibling: Element) => sibling.nodeName === currentNodeName
        )
        if (siblings.length > 1) {
          part += `:nth-of-type(${siblings.indexOf(current) + 1})`
        }
      }
      parts.unshift(part)
      current = parent
    }
    return parts.join(' > ')
  }

  function deepQuerySelector(
    root: Document | ShadowRoot | Element,
    selector: string
  ): Element | null {
    const direct = preferredMatch(Array.from(root.querySelectorAll(selector)))
    if (direct) {
      return direct
    }

    for (const element of Array.from(root.querySelectorAll('*'))) {
      const shadowRoot = element.shadowRoot
      if (shadowRoot) {
        const found = deepQuerySelector(shadowRoot, selector)
        if (found) {
          return found
        }
      }
      const frameDocument = childFrameDocument(element)
      if (frameDocument) {
        const frameFound = deepQuerySelector(frameDocument, selector)
        if (frameFound) {
          return frameFound
        }
      }
    }
    return null
  }

  function childFrameDocument(element: Element): Document | null {
    if (element.tagName !== 'IFRAME') {
      return null
    }
    try {
      const frame = element as HTMLIFrameElement
      return frame.contentDocument || frame.contentWindow?.document || null
    } catch (_error) {
      return null
    }
  }

  function interactable(element: Element): boolean {
    if (!visible(element)) {
      return false
    }
    const rect = element.getBoundingClientRect()
    const hit = element.ownerDocument.elementFromPoint(
      rect.left + Math.max(1, rect.width) / 2,
      rect.top + Math.max(1, rect.height) / 2
    )
    if (!hit) {
      return false
    }
    if (hit === element || element.contains(hit)) {
      return true
    }
    const label = hit.closest('label')
    return (
      label instanceof HTMLLabelElement && (label.control === element || label.contains(element))
    )
  }

  function preferredMatch(elements: Element[]): Element | null {
    return (
      elements.find((element) => interactable(element)) ||
      elements.find((element) => visible(element)) ||
      elements[0] ||
      null
    )
  }

  function queryRequired(selector?: string): Element {
    if (!selector) {
      throw new Error('selector is required')
    }
    const element = deepQuerySelector(document, selector)
    if (!element) {
      throw new Error(`selector not found: ${selector}`)
    }
    return element
  }

  function elementLabel(element: Element): string {
    return truncate(
      element.getAttribute('aria-label') ||
        element.getAttribute('title') ||
        element.getAttribute('placeholder') ||
        (element instanceof HTMLElement ? element.innerText : '') ||
        ('value' in element ? String(element.value) : '') ||
        element.textContent ||
        element.tagName,
      240
    )
  }

  function elementAttributes(element: Element): Record<string, string> {
    return Object.fromEntries(Array.from(element.attributes).map((attr) => [attr.name, attr.value]))
  }

  function elementBox(element: Element): Record<string, number> {
    const rect = element.getBoundingClientRect()
    return {
      x: rect.x,
      y: rect.y,
      width: rect.width,
      height: rect.height,
      top: rect.top,
      right: rect.right,
      bottom: rect.bottom,
      left: rect.left
    }
  }

  function elementInfo(element: Element): Record<string, unknown> {
    return {
      selector: cssPath(element),
      tag: element.tagName.toLowerCase(),
      id: element.id || null,
      classes: Array.from(element.classList),
      role: element.getAttribute('role'),
      name: element.getAttribute('name'),
      type: element.getAttribute('type'),
      label: elementLabel(element),
      text: truncate(
        element instanceof HTMLElement ? element.innerText : element.textContent,
        2000
      ),
      value: 'value' in element ? String(element.value) : null,
      attributes: elementAttributes(element),
      bounding_box: elementBox(element),
      visible: visible(element),
      disabled: 'disabled' in element ? Boolean(element.disabled) : false
    }
  }

  function pointFromArgs(prefix = ''): { x: number; y: number } | null {
    const x = prefix === 'to_' ? args.to_x : args.x
    const y = prefix === 'to_' ? args.to_y : args.y
    return typeof x === 'number' &&
      typeof y === 'number' &&
      Number.isFinite(x) &&
      Number.isFinite(y)
      ? { x, y }
      : null
  }

  function elementFromSelectorOrPoint(selector = args.selector): Element {
    if (selector) {
      return queryRequired(selector)
    }
    const point = pointFromArgs()
    if (!point) {
      throw new Error('selector or x/y coordinates are required')
    }
    const element = document.elementFromPoint(point.x, point.y)
    if (!element) {
      throw new Error(`no element at coordinates: ${point.x},${point.y}`)
    }
    return element
  }

  function editableElement(selector = args.selector): Element {
    if (selector) {
      return queryRequired(selector)
    }
    const active = document.activeElement
    const frameActive = active ? childFrameDocument(active)?.activeElement : null
    if (frameActive && frameActive !== frameActive.ownerDocument.body) {
      return frameActive
    }
    if (active && active !== document.body && active !== document.documentElement) {
      return active
    }
    throw new Error('type_text requires selector or an active editable element')
  }

  function selectElement(selector = args.selector): HTMLSelectElement {
    const element = queryRequired(selector)
    if (element.tagName === 'SELECT') {
      return element as HTMLSelectElement
    }
    const frameDocument = childFrameDocument(element)
    const nestedSelect = frameDocument?.querySelector('select')
    if (nestedSelect?.tagName === 'SELECT') {
      return nestedSelect as HTMLSelectElement
    }
    throw new Error(`selector is not a select element: ${selector}`)
  }

  function centerOf(element: Element): { x: number; y: number } {
    const rect = element.getBoundingClientRect()
    return { x: rect.left + rect.width / 2, y: rect.top + rect.height / 2 }
  }

  function pointerPoint(element: Element): { x: number; y: number } {
    return args.selector ? centerOf(element) : pointFromArgs() || centerOf(element)
  }

  function dispatchMouse(element: Element, type: string, point: { x: number; y: number }): void {
    element.dispatchEvent(
      new MouseEvent(type, {
        bubbles: true,
        cancelable: true,
        clientX: point.x,
        clientY: point.y,
        view: window
      })
    )
  }

  function setElementValue(
    element: HTMLInputElement | HTMLTextAreaElement | HTMLSelectElement,
    value: string
  ): void {
    const prototype = Object.getPrototypeOf(element)
    const descriptor = Object.getOwnPropertyDescriptor(prototype, 'value')
    if (descriptor?.set) {
      descriptor.set.call(element, value)
    } else {
      element.value = value
    }
  }

  function scrollBehavior(): ScrollBehavior {
    return args.behavior === 'auto' || args.behavior === 'smooth' || args.behavior === 'instant'
      ? args.behavior
      : 'smooth'
  }

  function structuredData(): Record<string, unknown> {
    const jsonLd = Array.from(document.querySelectorAll('script[type="application/ld+json"]'))
      .slice(0, 20)
      .map((element) => {
        const text = element.textContent || ''
        try {
          return JSON.parse(text)
        } catch (_error) {
          return { parse_error: true, text: truncate(text, 2000) }
        }
      })
    const meta = Array.from(document.querySelectorAll('meta[name], meta[property]'))
      .slice(0, 80)
      .map((element) => ({
        name: element.getAttribute('name') || element.getAttribute('property'),
        content: truncate(element.getAttribute('content'), 1000)
      }))
    const tables = Array.from(document.querySelectorAll('table'))
      .filter(visible)
      .slice(0, 10)
      .map((table) => {
        const rows = Array.from(table.querySelectorAll('tr')).slice(0, 50)
        return {
          selector: cssPath(table),
          caption: truncate(table.querySelector('caption')?.textContent, 500),
          rows: rows.map((row) =>
            Array.from(row.querySelectorAll('th, td'))
              .slice(0, 20)
              .map((cell) => truncate(cell.textContent, 500))
          )
        }
      })
    const lists = Array.from(document.querySelectorAll('ul, ol'))
      .filter(visible)
      .slice(0, 20)
      .map((list) => ({
        selector: cssPath(list),
        ordered: list.tagName.toLowerCase() === 'ol',
        items: Array.from(list.children)
          .slice(0, 40)
          .map((item) => truncate(item.textContent, 500))
      }))

    return { url: location.href, title: document.title, json_ld: jsonLd, meta, tables, lists }
  }

  function findMatches(): Record<string, unknown> {
    const query = String(args.query || '')
      .trim()
      .toLowerCase()
    if (!query) {
      throw new Error('find_in_page requires query')
    }
    const matches = Array.from(document.body?.querySelectorAll('*') || [])
      .filter(
        (element) => visible(element) && (element.textContent || '').toLowerCase().includes(query)
      )
      .slice(0, 80)
      .map((element) => {
        if (args.highlight && element instanceof HTMLElement) {
          element.dataset.andaFindHighlight = 'true'
          element.style.outline = '2px solid #f59e0b'
          element.style.outlineOffset = '2px'
        }
        return {
          selector: cssPath(element),
          label: elementLabel(element),
          text: truncate(element.textContent, 800),
          bounding_box: elementBox(element)
        }
      })
    return {
      query: args.query,
      count: matches.length,
      highlighted: Boolean(args.highlight),
      matches
    }
  }

  function serializeForResult(value: unknown, depth = 0): unknown {
    if (
      value === null ||
      value === undefined ||
      typeof value === 'string' ||
      typeof value === 'number' ||
      typeof value === 'boolean'
    ) {
      return value
    }
    if (value instanceof Element) {
      return elementInfo(value)
    }
    if (value instanceof Error) {
      return { name: value.name, message: value.message, stack: value.stack }
    }
    if (depth > 4) {
      return String(value)
    }
    if (Array.isArray(value)) {
      return value.slice(0, 200).map((item) => serializeForResult(item, depth + 1))
    }
    if (typeof value === 'object') {
      return Object.fromEntries(
        Object.entries(value as Record<string, unknown>)
          .slice(0, 200)
          .map(([key, entry]) => [key, serializeForResult(entry, depth + 1)])
      )
    }
    return String(value)
  }

  function serializeScriptResult(value: unknown): unknown {
    const serialized = serializeForResult(value)
    return serialized === undefined ? null : serialized
  }

  function compileScriptExpression(code: string): ((args: BrowserActionArgs) => unknown) | null {
    const expression = code.trim().replace(/;+$/, '')
    if (!expression) {
      return null
    }

    try {
      return new Function('args', `"use strict"; return (${expression})`) as (
        args: BrowserActionArgs
      ) => unknown
    } catch (error) {
      if (error instanceof SyntaxError) {
        return null
      }
      throw error
    }
  }

  function scriptWithImplicitReturn(code: string): string | null {
    const body = code.trim().replace(/;+$/, '')
    if (!body) {
      return null
    }
    const splitAt = lastTopLevelSemicolon(body)
    if (splitAt < 0) {
      return null
    }
    const prefix = body.slice(0, splitAt + 1)
    const tail = body.slice(splitAt + 1).trim()
    if (!tail || !canImplicitlyReturn(tail)) {
      return null
    }
    return `${prefix}\nreturn (${tail});`
  }

  function canImplicitlyReturn(statement: string): boolean {
    return !/^(break|catch|class|const|continue|do|export|finally|for|function|if|import|let|return|switch|throw|try|var|while)\b/.test(
      statement
    )
  }

  function lastTopLevelSemicolon(code: string): number {
    let quote: string | null = null
    let escaped = false
    let lineComment = false
    let blockComment = false
    let parenDepth = 0
    let braceDepth = 0
    let bracketDepth = 0
    let last = -1

    for (let index = 0; index < code.length; index += 1) {
      const char = code[index]
      const next = code[index + 1]

      if (lineComment) {
        if (char === '\n' || char === '\r') {
          lineComment = false
        }
        continue
      }
      if (blockComment) {
        if (char === '*' && next === '/') {
          blockComment = false
          index += 1
        }
        continue
      }
      if (quote) {
        if (escaped) {
          escaped = false
        } else if (char === '\\') {
          escaped = true
        } else if (char === quote) {
          quote = null
        }
        continue
      }

      if (char === '/' && next === '/') {
        lineComment = true
        index += 1
        continue
      }
      if (char === '/' && next === '*') {
        blockComment = true
        index += 1
        continue
      }
      if (char === '"' || char === "'" || char === '`') {
        quote = char
        continue
      }
      if (char === '(') {
        parenDepth += 1
      } else if (char === ')') {
        parenDepth = Math.max(0, parenDepth - 1)
      } else if (char === '{') {
        braceDepth += 1
      } else if (char === '}') {
        braceDepth = Math.max(0, braceDepth - 1)
      } else if (char === '[') {
        bracketDepth += 1
      } else if (char === ']') {
        bracketDepth = Math.max(0, bracketDepth - 1)
      } else if (char === ';' && parenDepth === 0 && braceDepth === 0 && bracketDepth === 0) {
        last = index
      }
    }

    return last
  }

  function compileScriptBody(code: string): (args: BrowserActionArgs) => unknown {
    const implicitReturn = scriptWithImplicitReturn(code)
    return new Function('args', `"use strict";\n${implicitReturn || code}`) as (
      args: BrowserActionArgs
    ) => unknown
  }

  function isPromiseLike(value: unknown): value is PromiseLike<unknown> {
    return (
      value !== null &&
      (typeof value === 'object' || typeof value === 'function') &&
      typeof (value as { then?: unknown }).then === 'function'
    )
  }

  function scriptResult(value: unknown): Record<string, unknown> {
    return { executed: true, result: serializeScriptResult(value) }
  }

  function executeJavaScript(
    code: string
  ): Record<string, unknown> | Promise<Record<string, unknown>> {
    const execute = compileScriptExpression(code) || compileScriptBody(code)
    const result = execute(args)
    return isPromiseLike(result) ? Promise.resolve(result).then(scriptResult) : scriptResult(result)
  }

  function copyText(text: string): Record<string, unknown> | Promise<Record<string, unknown>> {
    const textarea = document.createElement('textarea')
    textarea.value = text
    textarea.style.position = 'fixed'
    textarea.style.opacity = '0'
    document.body.appendChild(textarea)
    textarea.focus()
    textarea.select()
    const copied = document.execCommand('copy')
    textarea.remove()
    if (copied) {
      return { copied: true, length: text.length, method: 'execCommand' }
    }
    if (navigator.clipboard?.writeText) {
      let timer: ReturnType<typeof setTimeout> | null = null
      const timeout = new Promise<never>((_resolve, reject) => {
        timer = setTimeout(() => reject(new Error('clipboard write timed out')), 2000)
      })
      const write = navigator.clipboard
        .writeText(text)
        .then(() => ({ copied: true, length: text.length, method: 'clipboard' }))
        .finally(() => {
          if (timer) {
            clearTimeout(timer)
          }
        })
      return Promise.race([write, timeout])
    }
    throw new Error('copy command failed')
  }

  switch (args.action) {
    case 'annotate_viewport': {
      document.getElementById('__anda_viewport_annotations')?.remove()
      const container = document.createElement('div')
      container.id = '__anda_viewport_annotations'
      container.style.position = 'fixed'
      container.style.inset = '0'
      container.style.pointerEvents = 'none'
      container.style.zIndex = '2147483647'
      const candidates = Array.from(
        document.querySelectorAll(
          "a[href], button, input, textarea, select, summary, [role='button'], [role='link'], [role='menuitem'], [tabindex]:not([tabindex='-1'])"
        )
      )
        .filter(visible)
        .filter((element) => {
          const rect = element.getBoundingClientRect()
          return (
            rect.bottom >= 0 &&
            rect.right >= 0 &&
            rect.top <= innerHeight &&
            rect.left <= innerWidth
          )
        })
        .slice(0, 120)
      const markers = candidates.map((element, index) => {
        const markerId = index + 1
        const rect = element.getBoundingClientRect()
        const badge = document.createElement('div')
        badge.textContent = String(markerId)
        badge.style.position = 'fixed'
        badge.style.left = `${Math.max(0, rect.left)}px`
        badge.style.top = `${Math.max(0, rect.top)}px`
        badge.style.minWidth = '18px'
        badge.style.height = '18px'
        badge.style.padding = '0 5px'
        badge.style.borderRadius = '9px'
        badge.style.background = '#f59e0b'
        badge.style.color = '#111827'
        badge.style.border = '1px solid #111827'
        badge.style.font = '700 12px/18px system-ui, sans-serif'
        badge.style.textAlign = 'center'
        badge.style.boxShadow = '0 1px 4px rgba(0,0,0,0.35)'
        container.appendChild(badge)
        return {
          marker: markerId,
          selector: cssPath(element),
          label: elementLabel(element),
          tag: element.tagName.toLowerCase(),
          bounding_box: elementBox(element)
        }
      })
      document.body.appendChild(container)
      return { annotated: true, count: markers.length, markers }
    }
    case 'clear_annotations': {
      const existing = document.getElementById('__anda_viewport_annotations')
      existing?.remove()
      const highlighted = Array.from(document.querySelectorAll('[data-anda-find-highlight]'))
      for (const element of highlighted) {
        if (element instanceof HTMLElement) {
          delete element.dataset.andaFindHighlight
          element.style.outline = ''
          element.style.outlineOffset = ''
        }
      }
      return { cleared: Boolean(existing) || highlighted.length > 0 }
    }
    case 'snapshot': {
      const links = args.include_links
        ? Array.from(document.querySelectorAll('a[href]'))
            .filter(visible)
            .slice(0, 80)
            .map((element) => ({
              text: elementLabel(element),
              href: element instanceof HTMLAnchorElement ? element.href : '',
              selector: cssPath(element)
            }))
        : []
      const forms = args.include_forms
        ? Array.from(document.querySelectorAll("input, textarea, select, button, [role='button']"))
            .filter(visible)
            .slice(0, 120)
            .map((element) => ({
              tag: element.tagName.toLowerCase(),
              type: element.getAttribute('type') || element.getAttribute('role') || null,
              name: element.getAttribute('name') || null,
              label: elementLabel(element),
              selector: cssPath(element)
            }))
        : []
      return {
        url: location.href,
        title: document.title,
        selection: String(window.getSelection ? window.getSelection() : ''),
        active_element: document.activeElement ? elementInfo(document.activeElement) : null,
        viewport: { width: window.innerWidth, height: window.innerHeight },
        scroll: {
          x: window.scrollX,
          y: window.scrollY,
          max_y: document.documentElement.scrollHeight
        },
        text: truncate(document.body ? document.body.innerText : ''),
        links,
        forms
      }
    }
    case 'extract_text': {
      const element = args.selector ? queryRequired(args.selector) : document.body
      return {
        selector: args.selector || 'body',
        text: truncate(
          element instanceof HTMLElement
            ? element.innerText || element.textContent
            : element?.textContent
        )
      }
    }
    case 'get_full_page_html': {
      return {
        url: location.href,
        title: document.title,
        html: truncate(document.documentElement?.outerHTML || '', maxChars(defaultHtmlLimit))
      }
    }
    case 'get_structured_data': {
      return structuredData()
    }
    case 'get_element_info': {
      const element = queryRequired(args.selector)
      return elementInfo(element)
    }
    case 'get_viewport_size': {
      return {
        viewport: { width: window.innerWidth, height: window.innerHeight },
        screen: { width: window.screen.width, height: window.screen.height },
        device_pixel_ratio: window.devicePixelRatio,
        scroll: {
          x: window.scrollX,
          y: window.scrollY,
          max_x: document.documentElement.scrollWidth,
          max_y: document.documentElement.scrollHeight
        }
      }
    }
    case 'find_in_page': {
      return findMatches()
    }
    case 'wait_for_element': {
      const timeout = timeoutMs(10000)
      const selector = args.selector
      if (!selector) {
        throw new Error('wait_for_element requires selector')
      }
      const existing = deepQuerySelector(document, selector)
      if (existing && visible(existing)) {
        return { found: true, selector, element: elementInfo(existing) }
      }
      return new Promise((resolve, reject) => {
        const observer = new MutationObserver(() => {
          const element = deepQuerySelector(document, selector)
          if (element && visible(element)) {
            clearTimeout(timer)
            observer.disconnect()
            resolve({ found: true, selector, element: elementInfo(element) })
          }
        })
        const timer = setTimeout(() => {
          observer.disconnect()
          reject(new Error(`selector not found before timeout: ${selector}`))
        }, timeout)
        observer.observe(document.documentElement, {
          childList: true,
          subtree: true,
          attributes: true
        })
      })
    }
    case 'read_selection': {
      return { selection: String(window.getSelection ? window.getSelection() : '') }
    }
    case 'click': {
      const element = elementFromSelectorOrPoint()
      element.scrollIntoView({ block: 'center', inline: 'center' })
      const point = pointerPoint(element)
      dispatchMouse(element, 'mouseover', point)
      dispatchMouse(element, 'mousemove', point)
      dispatchMouse(element, 'mousedown', point)
      dispatchMouse(element, 'mouseup', point)
      dispatchMouse(element, 'click', point)
      return { clicked: true, selector: args.selector, label: elementLabel(element) }
    }
    case 'hover': {
      const element = elementFromSelectorOrPoint()
      element.scrollIntoView({ block: 'center', inline: 'center' })
      const point = pointerPoint(element)
      dispatchMouse(element, 'mouseover', point)
      dispatchMouse(element, 'mouseenter', point)
      dispatchMouse(element, 'mousemove', point)
      return { hovered: true, selector: args.selector, label: elementLabel(element) }
    }
    case 'type_text': {
      const element = editableElement()
      element.scrollIntoView({ block: 'center', inline: 'center' })
      if (element instanceof HTMLElement) {
        element.focus()
      }
      if ((element as HTMLElement).isContentEditable === true) {
        element.textContent = args.text || ''
      } else if (
        element.tagName === 'INPUT' ||
        element.tagName === 'TEXTAREA' ||
        element.tagName === 'SELECT'
      ) {
        setElementValue(element as HTMLInputElement, args.text || '')
      } else {
        throw new Error(
          args.selector
            ? `selector is not editable: ${args.selector}`
            : 'active element is not editable'
        )
      }
      element.dispatchEvent(
        new InputEvent('input', {
          bubbles: true,
          inputType: 'insertText',
          data: args.text || ''
        })
      )
      element.dispatchEvent(new Event('change', { bubbles: true }))
      return {
        typed: true,
        selector: args.selector || cssPath(element),
        active_element: !args.selector,
        length: String(args.text || '').length
      }
    }
    case 'select_dropdown': {
      const element = selectElement()
      element.scrollIntoView({ block: 'center', inline: 'center' })
      const value = String(args.value || '')
      const option = Array.from(element.options).find(
        (option) => option.value === value || option.label === value || option.text === value
      )
      if (!option) {
        throw new Error(`select option not found: ${value}`)
      }
      setElementValue(element, option.value)
      element.dispatchEvent(new Event('input', { bubbles: true }))
      element.dispatchEvent(new Event('change', { bubbles: true }))
      return { selected: true, selector: args.selector, value: option.value, label: option.label }
    }
    case 'press_key': {
      const target = document.activeElement || document.body
      const key = args.key || 'Enter'
      target.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true }))
      target.dispatchEvent(new KeyboardEvent('keyup', { key, bubbles: true }))
      return { pressed: true, key }
    }
    case 'scroll': {
      const amount =
        typeof args.amount === 'number' && Number.isFinite(args.amount) ? args.amount : 700
      window.scrollBy({ top: amount, behavior: 'smooth' })
      return { scrolled: true, amount, scroll_y: window.scrollY }
    }
    case 'scroll_to': {
      if (args.selector) {
        const element = queryRequired(args.selector)
        element.scrollIntoView({ block: 'center', inline: 'center', behavior: scrollBehavior() })
        return { scrolled_to: true, selector: args.selector, label: elementLabel(element) }
      }
      const point = pointFromArgs()
      if (!point) {
        throw new Error('scroll_to requires selector or x/y coordinates')
      }
      window.scrollTo({ left: point.x, top: point.y, behavior: scrollBehavior() })
      return {
        scrolled_to: true,
        x: point.x,
        y: point.y,
        scroll_x: window.scrollX,
        scroll_y: window.scrollY
      }
    }
    case 'drag_and_drop': {
      const source = queryRequired(args.from_selector)
      const target = args.to_selector
        ? queryRequired(args.to_selector)
        : (() => {
            const point = pointFromArgs('to_')
            if (!point) {
              throw new Error('drag_and_drop requires to_selector or to_x/to_y')
            }
            const element = document.elementFromPoint(point.x, point.y)
            if (!element) {
              throw new Error(`no drop target at coordinates: ${point.x},${point.y}`)
            }
            return element
          })()
      source.scrollIntoView({ block: 'center', inline: 'center' })
      const sourcePoint = centerOf(source)
      const targetPoint = args.to_selector
        ? centerOf(target)
        : pointFromArgs('to_') || centerOf(target)
      const dataTransfer = new DataTransfer()
      source.dispatchEvent(
        new DragEvent('dragstart', { bubbles: true, cancelable: true, dataTransfer })
      )
      dispatchMouse(source, 'mousedown', sourcePoint)
      dispatchMouse(target, 'mousemove', targetPoint)
      target.dispatchEvent(
        new DragEvent('dragenter', { bubbles: true, cancelable: true, dataTransfer })
      )
      target.dispatchEvent(
        new DragEvent('dragover', { bubbles: true, cancelable: true, dataTransfer })
      )
      target.dispatchEvent(new DragEvent('drop', { bubbles: true, cancelable: true, dataTransfer }))
      dispatchMouse(target, 'mouseup', targetPoint)
      source.dispatchEvent(
        new DragEvent('dragend', { bubbles: true, cancelable: true, dataTransfer })
      )
      return {
        dragged: true,
        from_selector: args.from_selector,
        to_selector: args.to_selector || cssPath(target),
        from: elementLabel(source),
        to: elementLabel(target)
      }
    }
    case 'copy_to_clipboard': {
      return copyText(String(args.text || ''))
    }
    case 'go_back': {
      history.back()
      return { went_back: true, url: location.href }
    }
    case 'go_forward': {
      history.forward()
      return { went_forward: true, url: location.href }
    }
    case 'execute_javascript': {
      const code = String(args.code || '')
      if (!code.trim()) {
        throw new Error('execute_javascript requires code')
      }
      return executeJavaScript(code)
    }
    default:
      throw new Error(`unsupported browser action: ${args.action}`)
  }
}
