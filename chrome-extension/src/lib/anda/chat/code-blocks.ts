// Fenced code in rendered markdown gets its language in the corner and a copy
// button. Both live inside the <pre>: the markdown block owns its top-level
// nodes and replaces them when the text changes, so nothing is added outside.

const copyIcon =
  '<svg class="md-code-idle" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect width="14" height="14" x="8" y="8" rx="2"/><path d="M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2"/></svg>'
const copiedIcon =
  '<svg class="md-code-copied" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M20 6 9 17l-5-5"/></svg>'

const copiedMs = 1400

/** The Prism language a highlighted block was rendered with, or ''. */
export function codeBlockLanguage(className: string): string {
  return /(?:^|\s)language-([\w#+.-]+)/.exec(className)?.[1] ?? ''
}

/** Adds the language label and copy button to every fenced block under `root`. */
export function enhanceCodeBlocks(root: ParentNode, copyLabel: string): void {
  for (const pre of root.querySelectorAll<HTMLPreElement>('pre')) {
    const code = pre.querySelector(':scope > code')
    if (!code || pre.querySelector(':scope > .md-code-copy')) {
      continue
    }
    const language = codeBlockLanguage(pre.className)
    if (language) {
      pre.dataset.lang = language
    }
    const button = document.createElement('button')
    button.type = 'button'
    button.className = 'md-code-copy'
    button.title = copyLabel
    button.setAttribute('aria-label', copyLabel)
    button.innerHTML = copyIcon + copiedIcon
    pre.append(button)
  }
}

/** Handles a click inside rendered markdown; true when it was a copy button. */
export function copyCodeFromClick(event: MouseEvent): boolean {
  const button = (event.target as Element | null)?.closest?.<HTMLButtonElement>('.md-code-copy')
  const code = button?.parentElement?.querySelector(':scope > code')
  if (!button || !code) {
    return false
  }
  event.preventDefault()
  event.stopPropagation()
  void navigator.clipboard?.writeText(code.textContent || '').then(() => {
    button.dataset.copied = ''
    window.setTimeout(() => delete button.dataset.copied, copiedMs)
  })
  return true
}
