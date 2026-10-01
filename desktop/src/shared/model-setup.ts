import { isAlias, isMap, isSeq, parseDocument } from 'yaml'
import type { DaemonView } from './contract'

export interface ModelPreset {
  family: string
  model: string
  api_base: string
  api_key: string
  [key: string]: unknown
}

/** Read the shipped runtime template, so the wizard follows release defaults. */
export function modelPresets(source: string): ModelPreset[] {
  const doc = parseDocument(source)
  if (doc.errors.length) throw new Error('Invalid bundled model template')
  return (doc.toJS().model.providers as ModelPreset[]).filter(
    (provider) => provider.model && provider.api_base && !provider.auth
  )
}

/** Patch only the chosen provider and active model; retain other YAML and comments. */
export function configurePreset(source: string, preset: ModelPreset, apiKey: string): string {
  const doc = parseDocument(source)
  if (doc.errors.length || !isMap(doc.contents)) throw new Error('setupInvalidConfig')
  if (!apiKey.trim()) throw new Error('setupKeyRequired')
  if (!doc.has('model')) doc.set('model', doc.createNode({ providers: [] }))
  if (!isMap(doc.get('model'))) throw new Error('setupInvalidConfig')
  if (!doc.hasIn(['model', 'providers'])) doc.setIn(['model', 'providers'], doc.createNode([]))
  const providers = doc.getIn(['model', 'providers'])
  if (!isSeq(providers)) throw new Error('setupInvalidConfig')
  const matches = providers.items.filter((item) => {
    if (isAlias(item)) item = item.resolve(doc)
    if (!isMap(item) || item.get('model') !== preset.model) return false
    const auth = item.get('auth')
    return !isMap(auth) || auth.get('type') !== 'chatgpt'
  })
  if (matches.length > 1) throw new Error('setupPresetConflict')
  const existing = matches[0]
  if (isAlias(existing) || (isMap(existing) && existing.anchor))
    throw new Error('setupInvalidConfig')
  if (isMap(existing)) {
    // Do not replace a custom endpoint (or send its key to a different service).
    if (existing.get('family') !== preset.family || existing.get('api_base') !== preset.api_base)
      throw new Error('setupPresetConflict')
    for (const [key, value] of Object.entries(preset)) existing.set(key, doc.createNode(value))
    existing.set('api_key', apiKey.trim())
    existing.set('disabled', false)
    existing.delete('auth')
  } else {
    providers.add(doc.createNode({ ...preset, api_key: apiKey.trim(), disabled: false }))
  }
  doc.setIn(['model', 'active'], preset.model)
  return doc.toString()
}

export function needsModelSetup(connection: DaemonView): boolean {
  // Unknown/older gateways can still be configured manually. Only a confirmed
  // model issue should automatically interrupt the user with onboarding.
  return Boolean(
    connection.needsSetup && connection.setupIssues?.some((issue) => issue.startsWith('model.'))
  )
}
