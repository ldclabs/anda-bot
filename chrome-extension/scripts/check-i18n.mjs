import { promises as fs } from 'node:fs'
import path from 'node:path'
import process from 'node:process'
import { fileURLToPath } from 'node:url'

const SOURCE_EXTENSIONS = new Set([
  '.cjs',
  '.cts',
  '.html',
  '.js',
  '.json',
  '.jsx',
  '.mjs',
  '.mts',
  '.svelte',
  '.ts',
  '.tsx'
])

const projectRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const localesRoot = path.join(projectRoot, 'public', '_locales')
const manifestPath = path.join(projectRoot, 'public', 'manifest.json')

function toPosixPath(filePath) {
  return filePath.split(path.sep).join('/')
}

function toRelativePath(filePath) {
  return toPosixPath(path.relative(projectRoot, filePath))
}

function getLineNumber(text, index) {
  let line = 1
  for (let i = 0; i < index; i += 1) {
    if (text[i] === '\n') {
      line += 1
    }
  }
  return line
}

function parseArgs(argv) {
  const options = {
    json: false,
    reportPath: null,
    strict: false,
    help: false
  }

  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index]
    if (arg === '--json') {
      options.json = true
      continue
    }
    if (arg === '--strict') {
      options.strict = true
      continue
    }
    if (arg === '--help' || arg === '-h') {
      options.help = true
      continue
    }
    if (arg === '--report') {
      const next = argv[index + 1]
      if (!next || next.startsWith('--')) {
        throw new Error('Expected a file path after --report')
      }
      options.reportPath = next
      index += 1
      continue
    }
    throw new Error(`Unknown argument: ${arg}`)
  }

  return options
}

async function fileExists(filePath) {
  try {
    await fs.access(filePath)
    return true
  } catch {
    return false
  }
}

async function collectSourceFiles(rootDir) {
  const files = []

  async function visit(currentDir) {
    const entries = await fs.readdir(currentDir, { withFileTypes: true })
    for (const entry of entries) {
      const fullPath = path.join(currentDir, entry.name)
      if (entry.isDirectory()) {
        if (fullPath === localesRoot) {
          continue
        }
        await visit(fullPath)
        continue
      }
      if (!SOURCE_EXTENSIONS.has(path.extname(entry.name))) {
        continue
      }
      if (/\.(test|spec)\.[^.]+$/i.test(entry.name)) {
        continue
      }
      files.push(fullPath)
    }
  }

  if (await fileExists(rootDir)) {
    await visit(rootDir)
  }

  return files.sort((left, right) => left.localeCompare(right))
}

function extractBalancedParentheses(text, openIndex) {
  let depth = 0
  let quote = null
  let lineComment = false
  let blockComment = false

  for (let index = openIndex; index < text.length; index += 1) {
    const char = text[index]
    const next = text[index + 1]

    if (lineComment) {
      if (char === '\n') {
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
      if (char === '\\') {
        index += 1
        continue
      }
      if (char === quote) {
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

    if (char === '\'' || char === '"' || char === '`') {
      quote = char
      continue
    }

    if (char === '(') {
      depth += 1
      continue
    }

    if (char === ')') {
      depth -= 1
      if (depth === 0) {
        return {
          body: text.slice(openIndex + 1, index),
          endIndex: index + 1
        }
      }
    }
  }

  return null
}

function takeFirstArgument(expression) {
  let parenDepth = 0
  let bracketDepth = 0
  let braceDepth = 0
  let quote = null
  let lineComment = false
  let blockComment = false

  for (let index = 0; index < expression.length; index += 1) {
    const char = expression[index]
    const next = expression[index + 1]

    if (lineComment) {
      if (char === '\n') {
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
      if (char === '\\') {
        index += 1
        continue
      }
      if (char === quote) {
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

    if (char === '\'' || char === '"' || char === '`') {
      quote = char
      continue
    }

    if (char === '(') {
      parenDepth += 1
      continue
    }

    if (char === ')') {
      parenDepth -= 1
      continue
    }

    if (char === '[') {
      bracketDepth += 1
      continue
    }

    if (char === ']') {
      bracketDepth -= 1
      continue
    }

    if (char === '{') {
      braceDepth += 1
      continue
    }

    if (char === '}') {
      braceDepth -= 1
      continue
    }

    if (char === ',' && parenDepth === 0 && bracketDepth === 0 && braceDepth === 0) {
      return expression.slice(0, index).trim()
    }
  }

  return expression.trim()
}

function extractStringLiterals(expression) {
  const values = []
  const literalPattern = /'([^'\\]*(?:\\.[^'\\]*)*)'|"([^"\\]*(?:\\.[^"\\]*)*)"/g

  for (const match of expression.matchAll(literalPattern)) {
    const value = match[1] ?? match[2]
    if (value) {
      values.push(value)
    }
  }

  return values
}

function addOccurrence(map, key, occurrence) {
  const bucket = map.get(key) ?? []
  const marker = `${occurrence.file}:${occurrence.line}:${occurrence.kind}`
  if (!bucket.some((entry) => `${entry.file}:${entry.line}:${entry.kind}` === marker)) {
    bucket.push(occurrence)
  }
  map.set(key, bucket)
}

function scanGetMessageCalls(filePath, text, occurrences, dynamicUsages) {
  // Matches chrome.i18n.getMessage(...) and the $lib/i18n getMessage(...)
  // wrapper, but not the wrapper's own function declaration.
  const callPattern = /(?<!function\s)(?:chrome\.i18n\.)?getMessage\s*\(/g

  for (const match of text.matchAll(callPattern)) {
    const openIndex = (match.index ?? 0) + match[0].length - 1
    const callBody = extractBalancedParentheses(text, openIndex)
    if (!callBody) {
      continue
    }

    const firstArgument = takeFirstArgument(callBody.body)
    const keys = [...new Set(extractStringLiterals(firstArgument))].filter(
      (key) => !key.startsWith('@@')
    )
    const line = getLineNumber(text, match.index ?? 0)

    if (keys.length === 0) {
      dynamicUsages.push({
        file: toRelativePath(filePath),
        line,
        expression: firstArgument.replace(/\s+/g, ' ').trim()
      })
      continue
    }

    for (const key of keys) {
      addOccurrence(occurrences, key, {
        file: toRelativePath(filePath),
        line,
        kind: 'getMessage'
      })
    }
  }
}

function scanManifestPlaceholders(filePath, text, occurrences) {
  const placeholderPattern = /__MSG_([A-Za-z0-9_@]+)__/g

  for (const match of text.matchAll(placeholderPattern)) {
    const key = match[1]
    if (!key || key.startsWith('@@')) {
      continue
    }
    addOccurrence(occurrences, key, {
      file: toRelativePath(filePath),
      line: getLineNumber(text, match.index ?? 0),
      kind: '__MSG__'
    })
  }
}

async function loadManifestDefaultLocale() {
  const manifestText = await fs.readFile(manifestPath, 'utf8')
  const manifest = JSON.parse(manifestText)
  return manifest.default_locale || 'en'
}

async function loadLocaleDefinitions() {
  const localeEntries = await fs.readdir(localesRoot, { withFileTypes: true })
  const locales = new Map()

  for (const entry of localeEntries) {
    if (!entry.isDirectory()) {
      continue
    }
    const locale = entry.name
    const localeFile = path.join(localesRoot, locale, 'messages.json')
    const localeText = await fs.readFile(localeFile, 'utf8')
    const messages = JSON.parse(localeText)
    locales.set(locale, {
      file: toRelativePath(localeFile),
      keys: Object.keys(messages).sort((left, right) => left.localeCompare(right))
    })
  }

  return new Map([...locales.entries()].sort(([left], [right]) => left.localeCompare(right)))
}

function sortOccurrences(occurrences) {
  return [...occurrences].sort((left, right) => {
    if (left.file !== right.file) {
      return left.file.localeCompare(right.file)
    }
    if (left.line !== right.line) {
      return left.line - right.line
    }
    return left.kind.localeCompare(right.kind)
  })
}

function sortStrings(values) {
  return [...values].sort((left, right) => left.localeCompare(right))
}

function createReport({ sourceFiles, defaultLocale, occurrences, dynamicUsages, localeDefinitions }) {
  const usedKeys = sortStrings(occurrences.keys())
  const localeReports = {}
  const missingInAnyLocale = new Set()
  const unusedAcrossAllLocales = new Set()

  for (const [locale, localeData] of localeDefinitions.entries()) {
    const definedKeys = new Set(localeData.keys)
    const missingKeys = usedKeys.filter((key) => !definedKeys.has(key))
    const unusedKeys = localeData.keys.filter((key) => !occurrences.has(key))

    for (const key of missingKeys) {
      missingInAnyLocale.add(key)
    }
    for (const key of unusedKeys) {
      unusedAcrossAllLocales.add(key)
    }

    localeReports[locale] = {
      file: localeData.file,
      definedKeyCount: localeData.keys.length,
      missingKeys,
      unusedKeys
    }
  }

  const report = {
    scannedAt: new Date().toISOString(),
    projectRoot: '.',
    defaultLocale,
    summary: {
      sourceFileCount: sourceFiles.length,
      localeFileCount: localeDefinitions.size,
      usedKeyCount: usedKeys.length,
      dynamicUsageCount: dynamicUsages.length,
      missingInAnyLocaleCount: missingInAnyLocale.size,
      unusedAcrossAllLocalesCount: unusedAcrossAllLocales.size
    },
    usedKeys,
    usedKeySources: Object.fromEntries(
      usedKeys.map((key) => [key, sortOccurrences(occurrences.get(key) ?? [])])
    ),
    dynamicUsages: sortOccurrences(dynamicUsages),
    locales: localeReports,
    missingInAnyLocale: sortStrings(missingInAnyLocale),
    unusedAcrossAllLocales: sortStrings(unusedAcrossAllLocales),
    defaultLocaleMissingKeys: localeReports[defaultLocale]?.missingKeys ?? []
  }

  return report
}

function formatKeyList(keys) {
  return keys.length > 0 ? keys.join(', ') : 'none'
}

function printHumanSummary(report, reportPath) {
  console.log('I18n audit summary')
  console.log(`- Source files scanned: ${report.summary.sourceFileCount}`)
  console.log(`- Locale files scanned: ${report.summary.localeFileCount}`)
  console.log(`- Used keys: ${report.summary.usedKeyCount}`)
  console.log(`- Dynamic getMessage usages: ${report.summary.dynamicUsageCount}`)
  console.log(`- Missing keys in any locale: ${report.summary.missingInAnyLocaleCount}`)
  console.log(`- Unused keys across locales: ${report.summary.unusedAcrossAllLocalesCount}`)
  console.log('')

  for (const locale of Object.keys(report.locales).sort((left, right) => left.localeCompare(right))) {
    const localeReport = report.locales[locale]
    console.log(`${locale} (${localeReport.file})`)
    console.log(`- Defined keys: ${localeReport.definedKeyCount}`)
    console.log(`- Missing keys (${localeReport.missingKeys.length}): ${formatKeyList(localeReport.missingKeys)}`)
    console.log(`- Unused keys (${localeReport.unusedKeys.length}): ${formatKeyList(localeReport.unusedKeys)}`)
    console.log('')
  }

  if (report.dynamicUsages.length > 0) {
    console.log('Dynamic getMessage usages')
    for (const usage of report.dynamicUsages) {
      console.log(`- ${usage.file}:${usage.line} -> ${usage.expression}`)
    }
    console.log('')
  }

  if (reportPath) {
    console.log(`JSON report written to ${toPosixPath(reportPath)}`)
  }
}

async function main() {
  const options = parseArgs(process.argv.slice(2))

  if (options.help) {
    console.log('Usage: npm run i18n:check -- [--json] [--report <path>] [--strict]')
    console.log('')
    console.log('--json             Print the full audit report as JSON')
    console.log('--report <path>    Write the JSON audit report to a file')
    console.log('--strict           Exit with code 1 when missing, unused, or dynamic keys are found')
    return
  }

  const sourceFiles = [
    ...(await collectSourceFiles(path.join(projectRoot, 'src'))),
    ...(await collectSourceFiles(path.join(projectRoot, 'public')))
  ]
  const resolvedFiles = [...sourceFiles]
  const indexFile = path.join(projectRoot, 'index.html')

  if (await fileExists(indexFile)) {
    resolvedFiles.push(indexFile)
  }

  resolvedFiles.sort((left, right) => left.localeCompare(right))

  const occurrences = new Map()
  const dynamicUsages = []

  for (const filePath of resolvedFiles) {
    const text = await fs.readFile(filePath, 'utf8')
    scanGetMessageCalls(filePath, text, occurrences, dynamicUsages)
    scanManifestPlaceholders(filePath, text, occurrences)
  }

  const defaultLocale = await loadManifestDefaultLocale()
  const localeDefinitions = await loadLocaleDefinitions()
  const report = createReport({
    sourceFiles: resolvedFiles.map((filePath) => toRelativePath(filePath)),
    defaultLocale,
    occurrences,
    dynamicUsages,
    localeDefinitions
  })

  if (options.reportPath) {
    const reportPath = path.resolve(projectRoot, options.reportPath)
    await fs.mkdir(path.dirname(reportPath), { recursive: true })
    await fs.writeFile(reportPath, `${JSON.stringify(report, null, 2)}\n`, 'utf8')
  }

  if (options.json) {
    console.log(JSON.stringify(report, null, 2))
  } else {
    printHumanSummary(report, options.reportPath)
  }

  const hasIssues =
    report.dynamicUsages.length > 0 ||
    report.missingInAnyLocale.length > 0 ||
    report.unusedAcrossAllLocales.length > 0

  if (options.strict && hasIssues) {
    process.exitCode = 1
  }
}

main().catch((error) => {
  console.error(error instanceof Error ? error.message : String(error))
  process.exitCode = 1
})