import Prism from 'prismjs'

;(globalThis as typeof globalThis & { Prism?: typeof Prism }).Prism = Prism

// 1st tier
await import('prismjs/components/prism-markup')
await import('prismjs/components/prism-markup-templating')

// 2nd tier
await import('prismjs/components/prism-json')
await import('prismjs/components/prism-json5')
await import('prismjs/components/prism-markdown')

export default Prism
