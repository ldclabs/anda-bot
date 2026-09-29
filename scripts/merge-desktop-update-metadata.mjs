#!/usr/bin/env node
// Merges the per-architecture latest-mac-*.yml files that the macOS desktop
// builds write into the single latest-mac.yml electron-updater reads. Each
// build lists only its own files; electron-updater picks the arm64 or x64
// entry from the merged `files` list. Dependency-free so the release job does
// not need the workspace installed.
import { readdirSync, readFileSync, unlinkSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'

const directory = process.argv[2]
if (!directory) throw new Error('Usage: merge-desktop-update-metadata.mjs <release-dir>')
const inputs = readdirSync(directory)
  .filter((name) => /^latest-mac-.+\.yml$/.test(name))
  .sort()
if (inputs.length === 0) {
  console.log('No macOS desktop update metadata to merge.')
  process.exit(0)
}

/** Splits a document into the lines before, inside and after `files:`. */
function sections(text) {
  const lines = text.replace(/\r\n/g, '\n').trimEnd().split('\n')
  const start = lines.indexOf('files:')
  if (start < 0) throw new Error('Update metadata has no files list')
  let end = start + 1
  while (end < lines.length && lines[end].startsWith('  ')) end++
  return { head: lines.slice(0, start + 1), files: lines.slice(start + 1, end), tail: lines.slice(end) }
}
const documents = inputs.map((name) => readFileSync(join(directory, name), 'utf8'))
const versions = new Set(documents.map((text) => /^version: (.+)$/m.exec(text)?.[1]))
if (versions.size !== 1) throw new Error(`Desktop builds disagree on the version: ${[...versions]}`)
const merged = sections(documents[0])
merged.files = documents.flatMap((text) => sections(text).files)
writeFileSync(
  join(directory, 'latest-mac.yml'),
  [...merged.head, ...merged.files, ...merged.tail].join('\n') + '\n'
)
for (const name of inputs) unlinkSync(join(directory, name))
console.log(`Merged ${inputs.join(', ')} into latest-mac.yml`)
