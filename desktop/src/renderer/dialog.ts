/** Keyboard focus stays inside a modal and returns to the invoking control. */
export function focusDialog(node: HTMLElement, close: () => void) {
  const previous = document.activeElement as HTMLElement | null
  const controls = () =>
    Array.from(
      node.querySelectorAll<HTMLElement>(
        'button:not(:disabled), input:not(:disabled), select:not(:disabled), textarea:not(:disabled), [tabindex="0"]'
      )
    ).filter((element) => element.getClientRects().length > 0)
  queueMicrotask(() =>
    (node.querySelector<HTMLElement>('input, textarea') || controls()[0] || node).focus()
  )
  function keydown(event: KeyboardEvent) {
    if (event.key === 'Escape') {
      event.preventDefault()
      event.stopPropagation()
      close()
    }
    if (event.key !== 'Tab') return
    const items = controls()
    const first = items[0],
      last = items.at(-1)
    if (!first || !last) {
      event.preventDefault()
      node.focus()
      return
    }
    if (event.shiftKey && (document.activeElement === first || document.activeElement === node)) {
      event.preventDefault()
      last.focus()
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault()
      first.focus()
    }
  }
  node.addEventListener('keydown', keydown)
  return {
    destroy() {
      node.removeEventListener('keydown', keydown)
      previous?.focus()
    }
  }
}
