import Prism from 'prismjs'
;(globalThis as typeof globalThis & { Prism?: typeof Prism }).Prism = Prism

// 1st tier
import 'prismjs/components/prism-clike'
import 'prismjs/components/prism-markup'
import 'prismjs/components/prism-markup-templating'

// 2nd tier
import 'prismjs/components/prism-css'
import 'prismjs/components/prism-javascript'
import 'prismjs/components/prism-jsx'
import 'prismjs/components/prism-json'
import 'prismjs/components/prism-json5'
import 'prismjs/components/prism-markdown'
import 'prismjs/components/prism-bash'
import 'prismjs/components/prism-python'
import 'prismjs/components/prism-typescript'
import 'prismjs/components/prism-tsx'
import 'prismjs/components/prism-yaml'
import 'prismjs/components/prism-toml'
import 'prismjs/components/prism-rust'

export default Prism
