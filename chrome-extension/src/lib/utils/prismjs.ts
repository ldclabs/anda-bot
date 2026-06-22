import Prism from 'prismjs'
;(globalThis as typeof globalThis & { Prism?: typeof Prism }).Prism = Prism

// 1st tier
await import('prismjs/components/prism-clike')
await import('prismjs/components/prism-markup')
await import('prismjs/components/prism-markup-templating')

// 2nd tier
await import('prismjs/components/prism-css')
await import('prismjs/components/prism-javascript')
await import('prismjs/components/prism-jsx')
await import('prismjs/components/prism-json')
await import('prismjs/components/prism-json5')
await import('prismjs/components/prism-markdown')
await import('prismjs/components/prism-bash')
await import('prismjs/components/prism-python')
await import('prismjs/components/prism-typescript')
await import('prismjs/components/prism-tsx')
await import('prismjs/components/prism-yaml')
await import('prismjs/components/prism-toml')
await import('prismjs/components/prism-rust')

export default Prism
