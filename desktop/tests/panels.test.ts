import { mkdtemp, rm, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'
import {
  addPane,
  edgePane,
  panes,
  readWorkbench,
  removePane,
  resizeSplit,
  splitPane,
  swapPanes,
  validWorkbench,
  type LayoutNode
} from '../src/shared/panels'
import {
  cornerPanes,
  dropZone,
  edgeOf,
  fitWorkbench,
  gridOf,
  measure,
  minExtent,
  zoneBox
} from '../src/renderer/panel-layout'
import { DesktopStore } from '../src/main/store'

const chat: LayoutNode = { pane: 'chat' }

describe('workbench canvas', () => {
  it('opens panels beside the chat, then stacks them in that column', () => {
    let tree = addPane(chat, 'changes')
    expect(tree).toEqual({
      axis: 'row',
      children: [{ pane: 'chat' }, { pane: 'changes' }],
      sizes: [0.58, 0.42]
    })
    tree = addPane(tree, 'terminal')
    tree = addPane(tree, 'browser')
    expect(tree).toMatchObject({
      axis: 'row',
      children: [
        { pane: 'chat' },
        {
          axis: 'column',
          children: [{ pane: 'changes' }, { pane: 'terminal' }, { pane: 'browser' }]
        }
      ]
    })
    expect(panes(tree)).toEqual(['chat', 'changes', 'terminal', 'browser'])
    // Each stacked panel gets an even share of the column.
    expect((tree as { children: { sizes: number[] }[] }).children[1]!.sizes).toEqual([
      0.333333, 0.333333, 0.333333
    ])
    expect(addPane(tree, 'browser')).toBe(tree)
  })

  it('closes a pane and folds splits left with one child into their parent', () => {
    let tree = addPane(addPane(chat, 'changes'), 'terminal')
    tree = removePane(tree, 'changes')
    expect(tree).toEqual({
      axis: 'row',
      children: [{ pane: 'chat' }, { pane: 'terminal' }],
      sizes: [0.58, 0.42]
    })
    expect(removePane(tree, 'terminal')).toEqual(chat)
  })

  it('splits a pane on any side and merges splits that share an axis', () => {
    const tree = addPane(addPane(chat, 'changes'), 'terminal')
    // The browser beside the terminal, inside the right-hand column.
    const beside = splitPane(tree, 'terminal', 'browser', 'end')
    expect(beside).toMatchObject({
      children: [
        { pane: 'chat' },
        {
          axis: 'column',
          children: [
            { pane: 'changes' },
            {
              axis: 'row',
              children: [{ pane: 'terminal' }, { pane: 'browser' }],
              sizes: [0.5, 0.5]
            }
          ]
        }
      ]
    })
    // Left of the chat joins the root row instead of nesting a new one.
    const left = splitPane(tree, 'chat', 'browser', 'start')
    expect(left).toMatchObject({
      axis: 'row',
      children: [{ pane: 'browser' }, { pane: 'chat' }, { axis: 'column' }]
    })
    expect((left as { sizes: number[] }).sizes.reduce((a, b) => a + b)).toBeCloseTo(1)
    // Moving a pane onto itself, or onto a pane that is not there, changes nothing.
    expect(splitPane(tree, 'terminal', 'terminal', 'top')).toBe(tree)
    expect(splitPane(tree, 'resources', 'terminal', 'top')).toBe(tree)
  })

  it('moves a pane along a whole edge and swaps two panes', () => {
    const tree = addPane(addPane(chat, 'changes'), 'terminal')
    const bottom = edgePane(tree, 'terminal', 'bottom')
    expect(bottom).toEqual({
      axis: 'column',
      children: [
        { axis: 'row', children: [{ pane: 'chat' }, { pane: 'changes' }], sizes: [0.58, 0.42] },
        { pane: 'terminal' }
      ],
      sizes: [0.65, 0.35]
    })
    expect(panes(swapPanes(bottom, 'chat', 'terminal'))).toEqual(['terminal', 'changes', 'chat'])
  })

  it('accepts exactly a canvas', () => {
    const tree = addPane(addPane(chat, 'changes'), 'terminal')
    expect(validWorkbench(tree)).toBe(true)
    expect(validWorkbench({ pane: 'terminal' })).toBe(false)
    expect(validWorkbench({ axis: 'row', children: [chat, chat], sizes: [0.5, 0.5] })).toBe(false)
    expect(
      validWorkbench({ axis: 'row', children: [chat, { pane: 'git' }], sizes: [0.5, 0.5] })
    ).toBe(false)
    expect(
      validWorkbench({ axis: 'row', children: [chat, { pane: 'browser' }], sizes: [0.9, 0.5] })
    ).toBe(false)
    expect(validWorkbench({ axis: 'row', children: [chat], sizes: [1] })).toBe(false)
    expect(
      validWorkbench({
        axis: 'row',
        children: [chat, { pane: 'browser' }],
        sizes: [0.5, 0.5],
        x: 1
      })
    ).toBe(false)
    expect(readWorkbench(tree, ['terminal'])).toEqual(removePane(tree, 'terminal'))
    expect(readWorkbench('nonsense', [])).toEqual(chat)
  })

  it('drops the single panel width of earlier versions', async () => {
    const directory = await mkdtemp(join(tmpdir(), 'anda-panels-'))
    try {
      const path = join(directory, 'desktop.json')
      const tree = addPane(chat, 'browser')
      await writeFile(
        path,
        JSON.stringify({
          preferences: { panelWidth: 520, chats: [], workbench: tree, detachedPanels: ['browser'] }
        })
      )
      const store = new DesktopStore(path)
      await store.load()
      expect('panelWidth' in store.state.preferences).toBe(false)
      // A panel that opens in its own window is not on the canvas too.
      expect(store.state.preferences.workbench).toEqual(chat)
      expect(store.state.preferences.detachedPanels).toEqual(['browser'])
    } finally {
      await rm(directory, { recursive: true, force: true })
    }
  })
})

describe('canvas geometry', () => {
  const tree = splitPane(
    addPane(addPane(chat, 'changes'), 'terminal'),
    'terminal',
    'browser',
    'end'
  )

  it('turns the tree into one grid with shared lines', () => {
    const geometry = measure(tree)
    expect(geometry.panes.get('chat')).toEqual({ x0: 0, x1: 0.58, y0: 0, y1: 1 })
    expect(geometry.panes.get('terminal')).toEqual({ x0: 0.58, x1: 0.79, y0: 0.5, y1: 1 })
    expect(geometry.dividers.map((divider) => [divider.key, divider.axis, divider.at])).toEqual([
      [':0', 'row', 0.58],
      ['1:0', 'column', 0.5],
      ['1.1:0', 'row', 0.79]
    ])
    const grid = gridOf(geometry)
    expect(grid.columns).toBe('minmax(0, 580.000fr) minmax(0, 210.000fr) minmax(0, 210.000fr)')
    expect(grid.rows).toBe('minmax(0, 500.000fr) minmax(0, 500.000fr)')
    // Column 1 is the sidebar's.
    expect(grid.pane('chat')).toBe('1 / 2 / 3 / 3')
    expect(grid.pane('changes')).toBe('1 / 3 / 2 / 5')
    expect(grid.pane('browser')).toBe('2 / 4 / 3 / 5')
    expect(grid.pane('resources')).toBeUndefined()
    expect(grid.divider(geometry.dividers[0]!)).toBe('1 / 3 / 3 / 4')
    expect(grid.divider(geometry.dividers[1]!)).toBe('2 / 3 / 3 / 5')
  })

  it('names the edge a pane runs along and the panes under the window controls', () => {
    const geometry = measure(edgePane(tree, 'changes', 'top'))
    expect(edgeOf(geometry.panes.get('changes'))).toBe('top')
    expect(edgeOf(geometry.panes.get('chat'))).toBeNull()
    expect(edgeOf(measure(addPane(chat, 'browser')).panes.get('browser'))).toBe('end')
    // The sidebar holds the left corner; right to left, the canvas starts on the right.
    expect(cornerPanes(geometry.panes, 'left', false)).toEqual({ left: null, right: 'changes' })
    expect(cornerPanes(measure(tree).panes, null, false)).toEqual({
      left: 'chat',
      right: 'changes'
    })
    expect(cornerPanes(measure(tree).panes, 'right', true)).toEqual({
      left: 'changes',
      right: null
    })
  })

  it('keeps the minimum size of everything on one side of a divider', () => {
    expect(minExtent(tree, 'row')).toBe(340 + 220 + 220)
    // The right-hand column stacks two panes, which need more than the chat.
    expect(minExtent(tree, 'column')).toBe(140 + 140)
  })

  it('shows each pane at its minimum size or more when the room allows', () => {
    // Dropping the browser left of the chat halves the chat's share.
    const squeezed = splitPane(tree, 'chat', 'browser', 'start')
    const shown = fitWorkbench(squeezed, 1000, 800) as { sizes: number[] }
    expect(shown.sizes[1]! * 1000).toBeCloseTo(340)
    expect(shown.sizes.reduce((a, b) => a + b)).toBeCloseTo(1)
    expect(shown.sizes[0]! * 1000).toBeGreaterThanOrEqual(220)
    // Without the room for every minimum, the proportions stay.
    expect((fitWorkbench(squeezed, 500, 800) as { sizes: number[] }).sizes).toEqual(
      (squeezed as { sizes: number[] }).sizes
    )
    expect(fitWorkbench(squeezed, 0, 0)).toBe(squeezed)
  })

  it('lands a dragged panel by the nearest edge, or in a pane’s place', () => {
    expect(dropZone(0.1, 0.5, false)).toBe('start')
    expect(dropZone(0.1, 0.5, true)).toBe('end')
    expect(dropZone(0.5, 0.9, false)).toBe('bottom')
    expect(dropZone(0.5, 0.5, false)).toBe('center')
    const box = { left: 100, top: 50, width: 400, height: 300 }
    expect(zoneBox(box, 'end', false)).toEqual({ left: 300, top: 50, width: 200, height: 300 })
    expect(zoneBox(box, 'end', true)).toEqual({ left: 100, top: 50, width: 200, height: 300 })
    expect(zoneBox(box, 'bottom', false)).toEqual({ left: 100, top: 200, width: 400, height: 150 })
    expect(zoneBox(box, 'center', false)).toEqual(box)
  })

  it('resizes the split a divider belongs to', () => {
    const resized = resizeSplit(tree, [1], [0.3, 0.7])
    expect(measure(resized).panes.get('changes')).toEqual({ x0: 0.58, x1: 1, y0: 0, y1: 0.3 })
  })
})
