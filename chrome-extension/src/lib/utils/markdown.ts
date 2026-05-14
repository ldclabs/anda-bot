import katex from 'katex'
import MarkdownIt from 'markdown-it'
import Prism from 'prismjs'

let prismLoading: Promise<void> | null = null

function ensurePrismLanguages() {
	if (typeof window === 'undefined') return
	if (!prismLoading) {
		prismLoading = import('./prismjs.js').then(() => {})
	}
}

ensurePrismLanguages()

// 创建 MarkdownIt 实例
const md = new MarkdownIt({
	html: true,
	linkify: true,
	typographer: true,
	breaks: true
})

md.linkify.set({ fuzzyLink: false, fuzzyEmail: false })
// 自定义链接渲染规则，让链接在新页面打开
const defaultLinkOpenRenderer =
	md.renderer.rules['link_open'] ||
	function (tokens, idx, options, env, renderer) {
		return renderer.renderToken(tokens, idx, options)
	}

const defaultLinkCloseRenderer =
	md.renderer.rules['link_close'] ||
	function (tokens, idx, options, env, renderer) {
		return renderer.renderToken(tokens, idx, options)
	}

// 检查是否为绝对路径
function isAbsoluteUrl(href: string): boolean {
	return /^(?:[a-z][a-z0-9+.-]*:|\/\/)/i.test(href)
}

md.renderer.rules['link_open'] = function (tokens, idx, options, env, renderer) {
	const token = tokens[idx]!
	const href = token.attrGet('href') || ''

	// 如果是相对路径，不渲染为链接，只返回空字符串
	if (!isAbsoluteUrl(href)) {
		return ''
	}

	// 对绝对路径添加 target="_blank" 和 rel="noopener noreferrer" 属性
	token.attrSet('target', '_blank')
	token.attrSet('rel', 'noopener noreferrer')

	return defaultLinkOpenRenderer(tokens, idx, options, env, renderer)
}

md.renderer.rules['link_close'] = function (tokens, idx, options, env, renderer) {
	// 查找对应的 link_open token 来检查 href
	let level = 1
	let i = idx - 1
	while (i >= 0 && level > 0) {
		if (tokens[i]!.type === 'link_close') level++
		if (tokens[i]!.type === 'link_open') level--
		i--
	}
	i++ // 回到 link_open 的位置

	if (i >= 0 && tokens[i]!.type === 'link_open') {
		const href = tokens[i]!.attrGet('href') || ''
		// 如果是相对路径，不渲染闭合标签
		if (!isAbsoluteUrl(href)) {
			return ''
		}
	}

	return defaultLinkCloseRenderer(tokens, idx, options, env, renderer)
}

// 修复类似 **“财多身弱”** 这种“标点紧邻分隔符”导致的加粗失效
// CommonMark 的强调规则在“前一个字符不是空白/标点、下一个字符是标点”时会拒绝把 ** 当作开启分隔符
// 这在中文排版里很常见（例如：这叫**“……”**）。这里做一个有界的补丁，处理中文引号/书名号/括号等场景。
function strongCjkQuotePlugin(md: MarkdownIt) {
	const openQuotes = new Set(['“', '‘', '「', '『', '《', '（', '【', '〖', '［', '｛'])

	md.inline.ruler.before('emphasis', 'strong_cjk_quote', function (state, silent) {
		const start = state.pos
		const src = state.src

		if (start + 2 >= state.posMax) return false

		// 处理 ** 或 *
		let isStrong = false
		let markerLen = 0
		if (src[start] === '*' && src[start + 1] === '*') {
			isStrong = true
			markerLen = 2
		} else if (src[start] === '*') {
			isStrong = false
			markerLen = 1
		} else {
			return false
		}

		// 不要抢在 *** 这种更长的前缀前面处理，让默认 emphasis 处理
		if (start + markerLen < state.posMax && src[start + markerLen] === '*') {
			return false
		}
		// 支持转义 \* 不触发
		if (start > 0 && src[start - 1] === '\\') return false

		const afterLeft = src[start + markerLen]
		if (!afterLeft || !openQuotes.has(afterLeft)) return false

		// 查找右侧的闭合标签（不允许跨行）
		const marker = isStrong ? '**' : '*'
		let pos = start + markerLen
		const nextNewline = src.indexOf('\n', pos)
		const lineEnd = nextNewline === -1 ? state.posMax : Math.min(nextNewline, state.posMax)

		while (pos < lineEnd) {
			const close = src.indexOf(marker, pos + 1)
			if (close === -1 || close >= lineEnd) break

			// 支持转义
			if (close > 0 && src[close - 1] === '\\') {
				pos = close + marker.length
				continue
			}

			// 确保闭合标记后不是同样的 marker（例如防止 ** 匹配 *** 的前两个）
			if (close + marker.length < state.posMax && src[close + marker.length] === '*') {
				pos = close + marker.length
				continue
			}

			const content = src.slice(start + markerLen, close)
			if (!content.trim()) break

			if (!silent) {
				const tag = isStrong ? 'strong' : 'em'
				const tokenOpen = state.push(tag + '_open', tag, 1)
				tokenOpen.markup = marker

				const oldMax = state.posMax
				state.pos = start + markerLen
				state.posMax = close
				state.md.inline.tokenize(state)
				state.posMax = oldMax

				const tokenClose = state.push(tag + '_close', tag, -1)
				tokenClose.markup = marker
			}

			state.pos = close + marker.length
			return true
		}

		return false
	})
}

// KaTeX 插件 - 处理数学公式
function katexPlugin(md: MarkdownIt) {
	const isWhitespace = (ch: string | undefined): boolean => {
		if (!ch) return true
		return /\s/.test(ch)
	}

	const looksLikeInlineMathForDollar = (raw: string): boolean => {
		const content = raw.trim()
		if (!content || content.length > 500) return false

		// 纯数字（含千分位/小数/百分号/货币单位等常见组合）通常不当作公式
		if (/^[\d,.$¥€£%]+$/.test(content)) return false

		// 常见日期/时间/编号
		if (/^\d{4}-\d{2}-\d{2}$/.test(content)) return false
		if (/^\d{4}\/\d{1,2}\/\d{1,2}$/.test(content)) return false
		if (/^\d{1,2}:\d{2}(?::\d{2})?$/.test(content)) return false
		if (/^#\d+$/.test(content)) return false

		// 含有中文字符的极大概率不是数学公式（KaTeX 虽支持但此处应用场景主要是排除货币/文案）
		if (/[\u4e00-\u9fa5]/.test(content)) return false

		// 只要出现 KaTeX 命令，必然是数学
		if (/\\[a-zA-Z]+/.test(content)) return true

		// 典型数学符号/运算符/结构
		if (/[=<>^_+\-*/]/.test(content)) {
			// 避免把 **（加粗标记）误判为乘方（虽然 TeX 用 ^ 但 markdown 习惯里 ** 很常见）
			// 如果只有数字和 **，更可能是被错误包围的金额
			if (content.includes('**') && /^[\d.** ]+$/.test(content)) return false
			return true
		}
		if (/[(){}\[\]|]/.test(content)) return true
		if (/[±×÷∑∫√∞≈≠≤≥]/.test(content)) return true

		// 变量名：含字母/希腊字母（允许 $x$ 这类最常见写法）
		if (/[a-zA-Z]/.test(content)) return true
		if (/[Α-Ωα-ω]/.test(content)) return true

		return false
	}

	// 定义支持的分隔符
	const delimiters = [
		{ left: '$$', right: '$$', block: true, inline: true },
		{ left: '$', right: '$', block: false, inline: true },
		{ left: '\\(', right: '\\)', block: true, inline: true },
		{ left: '\\[', right: '\\]', block: true, inline: true }
	]

	// 行内数学公式解析器
	md.inline.ruler.before('escape', 'math_inline', function (state, silent) {
		const start = state.pos
		const src = state.src

		// 检查所有行内分隔符
		for (const delimiter of delimiters) {
			if (!delimiter.inline) continue // 跳过块级分隔符

			const leftDelim = delimiter.left
			const rightDelim = delimiter.right

			// 检查是否匹配左分隔符
			if (!src.slice(start).startsWith(leftDelim)) continue

			// 支持用户写 \$ 或 \\( 等进行转义：此时不当作公式分隔符
			if (start > 0 && src[start - 1] === '\\') continue

			// 行内公式：分隔符内侧不能是空白，否则很容易把货币 $9.99 ... $99.9 误判为一整个公式
			const afterLeft = src[start + leftDelim.length]
			if (isWhitespace(afterLeft)) continue

			let pos = start + leftDelim.length
			let found = false

			// 查找右分隔符
			while (pos < state.posMax) {
				// 行内公式不允许跨行，避免把后续文本吞进去
				if (src[pos] === '\n') break

				if (src.slice(pos).startsWith(rightDelim)) {
					// 右分隔符前不能是空白（否则更像是文本中的 $ 号而不是公式闭合）
					const beforeRight = src[pos - 1]
					if (!isWhitespace(beforeRight)) {
						found = true
						break
					}
				}
				if (src[pos] === '\\') pos++ // 跳过转义字符
				pos++
			}

			if (!found) continue

			const content = src.slice(start + leftDelim.length, pos)
			if (!content.trim()) continue

			// 仅对 $...$ 启用更严格的启发式，避免把货币/编号误当作公式
			if (leftDelim === '$' && !looksLikeInlineMathForDollar(content)) {
				continue
			}

			if (!silent) {
				const token = state.push('math_inline', 'math', 0)
				token.content = content
				token.markup = leftDelim
			}

			state.pos = pos + rightDelim.length
			return true
		}

		return false
	})

	// 块级数学公式解析器
	md.block.ruler.before('fence', 'math_block', function (state, start, end, silent) {
		let pos = state.bMarks[start]! + state.tShift[start]!
		let max = state.eMarks[start]!
		const src = state.src

		// 检查所有块级分隔符
		for (const delimiter of delimiters) {
			if (!delimiter.block) continue // 跳过行内分隔符

			const leftDelim = delimiter.left
			const rightDelim = delimiter.right

			// 检查是否匹配左分隔符
			if (!src.slice(pos).startsWith(leftDelim)) continue

			pos += leftDelim.length
			const firstLine = src.slice(pos, max).trim()

			// 处理单行情况（如 $$formula$$）
			if (firstLine.endsWith(rightDelim)) {
				const content = firstLine.slice(0, -rightDelim.length).trim()

				if (!silent) {
					const token = state.push('math_block', 'math', 0)
					token.content = content
					token.markup = leftDelim
					token.map = [start, start + 1]
				}

				state.line = start + 1
				return true
			}

			// 处理多行情况
			let nextLine = start + 1
			let content = firstLine
			let found = false

			while (nextLine < end) {
				pos = state.bMarks[nextLine]! + state.tShift[nextLine]!
				max = state.eMarks[nextLine]!

				if (pos < max && state.tShift[nextLine]! < state.blkIndent) break

				const line = src.slice(pos, max)

				// 检查是否包含右分隔符
				const rightIndex = line.indexOf(rightDelim)
				if (rightIndex !== -1) {
					// 找到右分隔符
					const beforeRight = line.slice(0, rightIndex).trim()
					if (beforeRight) {
						content += '\n' + beforeRight
					}
					found = true
					break
				}

				content += '\n' + line.trim()
				nextLine++
			}

			if (found) {
				if (!silent) {
					const token = state.push('math_block', 'math', 0)
					token.content = content
					token.markup = leftDelim
					token.map = [start, nextLine + 1]
				}

				state.line = nextLine + 1
				return true
			}
		}

		return false
	})

	// 渲染器
	md.renderer.rules['math_inline'] = function (tokens, idx) {
		const token = tokens[idx]!
		try {
			return katex.renderToString(token.content, { displayMode: false })
		} catch {
			return `<span class="katex-error">${token.content}</span>`
		}
	}

	md.renderer.rules['math_block'] = function (tokens, idx) {
		const token = tokens[idx]!
		try {
			return `<div class="katex-block">${katex.renderToString(token.content.trim(), { displayMode: true })}</div>`
		} catch {
			return `<div class="katex-error">${token.content.trim()}</div>`
		}
	}
}

// 代码高亮插件
function prismPlugin(md: MarkdownIt) {
	const fence = md.renderer.rules.fence!
	const langAliases: Record<string, string> = {
		js: 'javascript',
		mjs: 'javascript',
		cjs: 'javascript',
		jsx: 'jsx',
		ts: 'typescript',
		tsx: 'tsx',
		sh: 'bash',
		shell: 'bash',
		zsh: 'bash',
		yml: 'yaml',
		jsonc: 'json'
	}

	md.renderer.rules.fence = function (tokens, idx, options, env, renderer) {
		const token = tokens[idx]!
		const info = token.info ? token.info.trim() : ''
		let langName = info.split(/\s+/g)[0] || ''
		if (langName === 'mermaid') {
			// 让 mermaid 插件处理
			return fence(tokens, idx, options, env, renderer)
		} else if (langName === 'katex') {
			try {
				return `<div class="katex-block">${katex.renderToString(token.content.trim(), { displayMode: true })}</div>`
			} catch {
				return `<div class="katex-error">${md.utils.escapeHtml(token.content)}</div>`
			}
		} else if (langName) {
			langName = langAliases[langName] || langName
			if (langName && Prism.languages[langName]) {
				try {
					const highlighted = Prism.highlight(token.content, Prism.languages[langName]!, langName)
					return `<pre class="language-${langName}"><code class="language-${langName}">${highlighted}</code></pre>`
				} catch (err) {
					console.warn('Prism highlighting failed:', err)
				}
			}
		}

		// 回退到默认渲染
		return `<pre><code>${md.utils.escapeHtml(token.content)}</code></pre>`
	}
}

// 应用插件
md.use(strongCjkQuotePlugin)
md.use(katexPlugin)
md.use(prismPlugin)

/**
 * 渲染 Markdown 文本为 HTML
 * @param markdown - Markdown 文本
 * @param options - 渲染选项
 * @returns 渲染后的 HTML 字符串
 */
export function renderMarkdown(
	markdown: string,
	options?: {
		enableMermaid?: boolean
		enableKatex?: boolean
		enablePrism?: boolean
	}
): [string, () => Promise<void>] {
	if (!markdown || typeof markdown !== 'string') {
		return ['', () => Promise.resolve()]
	}

	const {
		enableMermaid = true
		// enableKatex = true,
		// enablePrism = true
	} = options || {}

	try {
		// 规范化换行符，确保不同平台的文本都能正确解析
		const normalized = markdown.replace(/\r\n/g, '\n').replace(/\r/g, '\n')
		const html = md.render(normalized)

		// 如果启用了 Mermaid，需要在 DOM 更新后渲染图表
		if (enableMermaid && html.includes('class="mermaid"')) {
			// 这里返回的 HTML 包含 mermaid div，需要在组件中调用 renderMermaidCharts
			return [html, () => Promise.resolve()]
		}

		return [html, () => Promise.resolve()]
	} catch (err) {
		console.error('Markdown rendering failed:', err)
		return [
			`<pre style="white-space: pre-wrap; word-break: break-all;">${md.utils.escapeHtml(markdown)}</pre>`,
			() => Promise.resolve()
		]
	}
}

/**
 * 获取 Markdown 文本的纯文本内容（去除格式）
 * @param markdown - Markdown 文本
 * @returns 纯文本内容
 */
export function getPlainText(markdown: string): string {
	try {
		const html = md.render(markdown).trim()
		if (typeof document === 'undefined') {
			// 服务端渲染环境下的简单回退：去除 HTML 标签
			return html.replace(/<[^>]*>/g, '').replace(/&nbsp;/g, ' ')
		}
		const div = document.createElement('div')
		div.innerHTML = html
		return div.textContent || div.innerText || ''
	} catch (err) {
		console.warn('getPlainText failed:', err)
		return markdown
	}
}

/**
 * 获取 Markdown 文本的摘要
 * @param markdown - Markdown 文本
 * @param maxLength - 最大长度，默认 200
 * @returns 摘要文本
 */
export function getMarkdownSummary(markdown: string, maxLength: number = 200): string {
	const plainText = getPlainText(markdown)
	if (plainText.length <= maxLength) return plainText

	return plainText.substring(0, maxLength).trim() + '...'
}

export default {
	renderMarkdown,
	getPlainText,
	getMarkdownSummary
}
