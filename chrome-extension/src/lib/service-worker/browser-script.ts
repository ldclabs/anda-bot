import type { BrowserActionArgs } from './types'

/**
 * How `execute_javascript` reaches the page, and the source rewriting that lets
 * a bare expression behave like a console entry.
 *
 * Two delivery modes exist because pages differ: the debugger evaluates in the
 * page's own world and is not stopped by a strict CSP, while
 * `scripting.executeScript` is cheaper but bound by the page's policy. The
 * debugger is the default; asking for `world: 'ISOLATED' | 'MAIN'` opts into
 * scripting instead.
 */

export type ScriptExecutionMode = 'debugger' | 'scripting'
type ScriptExecutionWorld = 'ISOLATED' | 'MAIN'
type RequestedScriptWorld = ScriptExecutionWorld | 'DEBUGGER'

export function scriptExecutionMode(args: BrowserActionArgs): ScriptExecutionMode {
  const world = requestedScriptWorld(args.world)
  if (world === 'DEBUGGER') {
    return 'debugger'
  }
  return args.use_bridge === false ? 'scripting' : 'debugger'
}

export function scriptExecutionWorld(args: BrowserActionArgs): ScriptExecutionWorld {
  return requestedScriptWorld(args.world) === 'MAIN' ? 'MAIN' : 'ISOLATED'
}

function requestedScriptWorld(value: unknown): RequestedScriptWorld {
  const normalized = typeof value === 'string' ? value.trim().toLowerCase() : ''
  if (normalized === 'main') {
    return 'MAIN'
  }
  if (normalized === 'debugger' || normalized === 'bridge') {
    return 'DEBUGGER'
  }
  return 'ISOLATED'
}

export function scriptWithImplicitReturn(code: string): string | null {
  const body = code.trim().replace(/;+$/, '')
  if (!body) {
    return null
  }
  const splitAt = lastTopLevelSemicolon(body)
  if (splitAt < 0) {
    return null
  }
  const prefix = body.slice(0, splitAt + 1)
  const tail = body.slice(splitAt + 1).trim()
  if (!tail || !canImplicitlyReturn(tail)) {
    return null
  }
  return `${prefix}\nreturn (${tail});`
}

function canImplicitlyReturn(statement: string): boolean {
  return !/^(break|catch|class|const|continue|do|export|finally|for|function|if|import|let|return|switch|throw|try|var|while)\b/.test(
    statement
  )
}

function lastTopLevelSemicolon(code: string): number {
  let quote: string | null = null
  let escaped = false
  let lineComment = false
  let blockComment = false
  let parenDepth = 0
  let braceDepth = 0
  let bracketDepth = 0
  let last = -1

  for (let index = 0; index < code.length; index += 1) {
    const char = code[index]
    const next = code[index + 1]

    if (lineComment) {
      if (char === '\n' || char === '\r') {
        lineComment = false
      }
      continue
    }
    if (blockComment) {
      if (char === '*' && next === '/') {
        blockComment = false
        index += 1
      }
      continue
    }
    if (quote) {
      if (escaped) {
        escaped = false
      } else if (char === '\\') {
        escaped = true
      } else if (char === quote) {
        quote = null
      }
      continue
    }

    if (char === '/' && next === '/') {
      lineComment = true
      index += 1
      continue
    }
    if (char === '/' && next === '*') {
      blockComment = true
      index += 1
      continue
    }
    if (char === '"' || char === "'" || char === '`') {
      quote = char
      continue
    }
    if (char === '(') {
      parenDepth += 1
    } else if (char === ')') {
      parenDepth = Math.max(0, parenDepth - 1)
    } else if (char === '{') {
      braceDepth += 1
    } else if (char === '}') {
      braceDepth = Math.max(0, braceDepth - 1)
    } else if (char === '[') {
      bracketDepth += 1
    } else if (char === ']') {
      bracketDepth = Math.max(0, bracketDepth - 1)
    } else if (char === ';' && parenDepth === 0 && braceDepth === 0 && bracketDepth === 0) {
      last = index
    }
  }

  return last
}
