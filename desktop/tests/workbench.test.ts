import { expect, it } from 'vitest'
import { SCROLLBACK, appendScrollback } from '../src/shared/workbench'

it('keeps at least the latest scrollback while trimming terminal output in bulk', () => {
  let output = ''
  for (let i = 0; i < 3000; i++) output = appendScrollback(output, `${i}`.padEnd(1024, '.'))
  expect(output.length).toBeLessThanOrEqual(2 * SCROLLBACK)
  expect(output.length).toBeGreaterThanOrEqual(SCROLLBACK)
  expect(output.endsWith('2999'.padEnd(1024, '.'))).toBe(true)
})
