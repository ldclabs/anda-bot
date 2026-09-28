import { afterEach, describe, expect, it, vi } from 'vitest'

import {
  pageElementDomMemoryKey,
  pageElementListenerKey as listenerKey,
  pageElementSerializerKey
} from '$lib/anda/page-element'

type ContextMenuListener = (event: MouseEvent) => void

class TestNode {
  static ELEMENT_NODE = 1
  nodeType = TestNode.ELEMENT_NODE
  parentElement: TestElement | null = null
}

class TestElement extends TestNode {
  tagName = 'BUTTON'
  id = 'submit'
  textReads = 0
  get innerText() {
    this.textReads += 1
    return 'Submit'
  }
  textContent = 'Submit'
  attributes = [{ name: 'id', value: 'submit' }]
  previousElementSibling: TestElement | null = null
  classList = { length: 0 }

  getAttribute(name: string): string | null {
    return name === 'role' ? 'button' : null
  }

  getBoundingClientRect() {
    return {
      x: 0,
      y: 0,
      width: 120,
      height: 32,
      top: 0,
      right: 120,
      bottom: 32,
      left: 0
    }
  }
}

afterEach(() => {
  vi.restoreAllMocks()
  vi.unstubAllGlobals()
  vi.resetModules()
  delete (globalThis as Record<string, unknown>)[listenerKey]
  delete (globalThis as Record<string, unknown>)[pageElementSerializerKey]
  delete (globalThis as Record<string, unknown>)[pageElementDomMemoryKey]
})

describe('page element content script', () => {
  it('remembers the right-clicked element and serializes it only on request', async () => {
    const chromeApi = new Proxy(
      {},
      {
        get() {
          throw new Error('Extension context invalidated.')
        }
      }
    )
    const contextMenu = await importContentScript(chromeApi)

    const element = new TestElement()
    contextMenu(contextMenuEvent(element))

    expect((globalThis as Record<string, unknown>)[pageElementDomMemoryKey]).toBe(element)
    expect(element.textReads).toBe(0)

    const serialize = (globalThis as Record<string, unknown>)[pageElementSerializerKey] as () => {
      capturedAt: number
    }
    const captured = serialize()
    expect(captured).toMatchObject({
      tagName: 'BUTTON',
      innerText: 'Submit',
      pageUrl: 'https://example.com/form'
    })
    expect(captured.capturedAt).toBeGreaterThan(0)
    expect(element.textReads).toBe(1)
  })

  it('has nothing to serialize before a right-click', async () => {
    await importContentScript({})
    const serialize = (globalThis as Record<string, unknown>)[
      pageElementSerializerKey
    ] as () => unknown
    expect(serialize()).toBeNull()
  })

  it('replaces the previous listener when the script is injected again', async () => {
    const removedListeners: EventListener[] = []
    const contextMenu = await importContentScript({})
    const firstListener = (globalThis as unknown as Record<string, EventListener>)[listenerKey]

    await importContentScript(
      {},
      {
        removeEventListener: (_type: string, listener: EventListener) =>
          removedListeners.push(listener)
      }
    )

    expect(firstListener).toBe(contextMenu)
    expect(removedListeners).toContain(firstListener)
  })
})

async function importContentScript(
  chromeApi: unknown,
  overrides: Partial<Pick<Document, 'removeEventListener'>> = {}
): Promise<ContextMenuListener> {
  let contextMenu: ContextMenuListener | null = null
  vi.resetModules()
  vi.stubGlobal('chrome', chromeApi)
  vi.stubGlobal('Element', TestElement)
  vi.stubGlobal('HTMLElement', TestElement)
  vi.stubGlobal('Node', TestNode)
  vi.stubGlobal('location', { href: 'https://example.com/form' })
  vi.stubGlobal(
    'getSelection',
    vi.fn(() => ({ toString: () => '' }))
  )
  vi.stubGlobal('document', {
    title: 'Example form',
    addEventListener: vi.fn((type: string, listener: ContextMenuListener) => {
      if (type === 'contextmenu') {
        contextMenu = listener
      }
    }),
    removeEventListener: vi.fn(),
    ...overrides
  })

  await import('./page_element_content')
  if (!contextMenu) {
    throw new Error('contextmenu listener was not registered')
  }
  return contextMenu
}

function contextMenuEvent(element: TestElement): MouseEvent {
  return {
    target: element,
    composedPath: () => [element]
  } as unknown as MouseEvent
}
