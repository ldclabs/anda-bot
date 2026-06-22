import { afterEach, describe, expect, it, vi } from 'vitest'

const pageElementMemoryKey = '__andaLastRightClickedElement'
const listenerKey = '__andaPageElementContentScriptContextMenuListener'

type ContextMenuListener = (event: MouseEvent) => void

class TestNode {
  static ELEMENT_NODE = 1
  nodeType = TestNode.ELEMENT_NODE
  parentElement: TestElement | null = null
}

class TestElement extends TestNode {
  tagName = 'BUTTON'
  id = 'submit'
  className = 'primary'
  innerText = 'Submit'
  textContent = 'Submit'
  outerHTML = '<button id="submit">Submit</button>'
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
})

describe('page element content script', () => {
  it('captures the right-clicked element without touching extension APIs', async () => {
    const chromeApi = new Proxy(
      {},
      {
        get() {
          throw new Error('Extension context invalidated.')
        }
      }
    )
    const contextMenu = await importContentScript(chromeApi)

    contextMenu(contextMenuEvent(new TestElement()))

    expect((globalThis as Record<string, unknown>)[pageElementMemoryKey]).toMatchObject({
      tagName: 'BUTTON',
      outerHTML: '<button id="submit">Submit</button>',
      pageUrl: 'https://example.com/form'
    })
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
  vi.stubGlobal('getSelection', vi.fn(() => ({ toString: () => '' })))
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
