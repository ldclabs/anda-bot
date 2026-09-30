import { setNativeMessages } from '$lib/i18n'
import { describe, expect, it } from 'vitest'
import {
  ensureObject,
  getObject,
  objectArray,
  modelProviderFields,
  normalizeConfigDraft,
  parseConfigDraft,
  renderConfigYaml
} from './schema'

const baseDraft = {
  addr: '127.0.0.1:8042',
  log_level: 'warn',
  workspaces: [],
  users: [],
  model: { active: 'demo', providers: [] },
  tts: {
    enabled: false,
    default_provider: 'edge',
    default_format: 'mp3',
    max_text_length: 4096
  },
  transcription: {
    enabled: false,
    default_provider: 'groq',
    initial_prompt: null
  },
  channels: { telegram: [], wechat: [], discord: [], lark: [] }
}

describe('config yaml renderer', () => {
  it('preserves comment lines while formatting from the structured draft', () => {
    const source = `## anda_bot runtime configuration
# keep the gateway local
addr: 127.0.0.1:8042
# error, warn, info, debug
log_level: warn

tts:
  enabled: false
  # edge:
  #   binary_path: edge-tts
channels:
  # telegram:
  #   - id: personal
`
    const draft = normalizeConfigDraft({ ...baseDraft, log_level: 'info' })

    const rendered = renderConfigYaml(draft, source)

    for (const line of source.split('\n').filter((item) => item.trimStart().startsWith('#'))) {
      expect(rendered).toContain(line)
    }
    expect(rendered).toContain('log_level: info')
    expect(rendered).toContain('channels:')
  })

  it('keeps leading comments attached to their keys', () => {
    const source = `# keep the gateway local
addr: 127.0.0.1:8042
# error, warn, info, debug
log_level: warn
`
    const rendered = renderConfigYaml(
      normalizeConfigDraft({ ...baseDraft, log_level: 'debug' }),
      source
    )

    const lines = rendered.split('\n')
    expect(lines[lines.indexOf('addr: 127.0.0.1:8042') - 1]).toBe('# keep the gateway local')
    expect(lines[lines.indexOf('log_level: debug') - 1]).toBe('# error, warn, info, debug')
  })

  it('preserves inline comments when a value changes', () => {
    const source = `addr: 127.0.0.1:8042 # local gateway only
log_level: warn # raise to debug when troubleshooting
`
    const rendered = renderConfigYaml(
      normalizeConfigDraft({ ...baseDraft, addr: '0.0.0.0:9000' }),
      source
    )

    expect(rendered).toContain('addr: 0.0.0.0:9000 # local gateway only')
    expect(rendered).toContain('log_level: warn # raise to debug when troubleshooting')
  })

  it('preserves keys the form schema does not know about', () => {
    const source = `addr: 127.0.0.1:8042
# experimental block maintained by hand
future_feature:
  enabled: true
  endpoints:
    - https://example.com
model:
  active: demo
  providers: []
  router_hint: latency # not in the form schema
`
    const rendered = renderConfigYaml(
      normalizeConfigDraft({ ...baseDraft, model: { active: 'claude', providers: [] } }),
      source
    )

    expect(rendered).toContain('# experimental block maintained by hand')
    expect(rendered).toContain('future_feature:')
    expect(rendered).toContain('  enabled: true')
    expect(rendered).toContain('    - https://example.com')
    expect(rendered).toContain('  router_hint: latency # not in the form schema')
    expect(rendered).toContain('active: claude')
  })

  it('updates nested provider values without disturbing comments around them', () => {
    const source = `model:
  active: demo
  # primary completion providers
  providers:
    - family: openai
      model: gpt-test
      api_key: secret # rotate monthly
`
    const draft = normalizeConfigDraft({
      ...baseDraft,
      model: {
        active: 'demo',
        providers: [{ family: 'openai', model: 'gpt-next', api_key: 'secret' }]
      }
    })

    const rendered = renderConfigYaml(draft, source)

    expect(rendered).toContain('# primary completion providers')
    expect(rendered).toContain('model: gpt-next')
    expect(rendered).toContain('api_key: secret # rotate monthly')
  })

  it('removes a disabled optional provider block', () => {
    const source = `tts:
  enabled: true
  edge:
    binary_path: edge-tts
    voice: en-US-AriaNeural
`
    const draft = normalizeConfigDraft({
      ...baseDraft,
      tts: { ...baseDraft.tts, enabled: true, edge: null }
    })

    const rendered = renderConfigYaml(draft, source)

    expect(rendered).not.toContain('edge:')
    expect(rendered).not.toContain('binary_path')
    expect(rendered).toContain('enabled: true')
  })

  it('does not pad missing keys with empty placeholders', () => {
    const source = `addr: 127.0.0.1:8042
log_level: warn
`
    const rendered = renderConfigYaml(normalizeConfigDraft(baseDraft), source)

    expect(rendered).not.toContain('workspaces:')
    expect(rendered).not.toContain('users:')
    expect(rendered).not.toContain('https_proxy:')
  })

  it('truncates array items removed in the form', () => {
    const source = `users:
  - id: alice
    pubkey: aaa
  - id: bob
    pubkey: bbb
`
    const draft = normalizeConfigDraft({
      ...baseDraft,
      users: [{ id: 'alice', pubkey: 'aaa' }]
    })

    const rendered = renderConfigYaml(draft, source)

    expect(rendered).toContain('id: alice')
    expect(rendered).not.toContain('bob')
  })

  it('falls back to a clean render when the previous source is invalid YAML', () => {
    const rendered = renderConfigYaml(
      normalizeConfigDraft(baseDraft),
      'addr: [unclosed\n  log_level :::'
    )

    expect(rendered).toContain('addr: 127.0.0.1:8042')
    expect(rendered).toContain('model:')
  })

  it('keeps quoting style when only the value changes', () => {
    const source = `model:
  active: "demo"
`
    const rendered = renderConfigYaml(
      normalizeConfigDraft({ ...baseDraft, model: { active: 'claude', providers: [] } }),
      source
    )

    expect(rendered).toContain('active: "claude"')
  })
})

describe('parseConfigDraft', () => {
  it('parses valid YAML into a normalized draft', () => {
    const draft = parseConfigDraft('addr: 0.0.0.0:1234\nmodel:\n  active: demo\n')

    expect(draft).not.toBeNull()
    expect(draft?.addr).toBe('0.0.0.0:1234')
    expect((draft?.model as { active: string }).active).toBe('demo')
    expect(Array.isArray(draft?.users)).toBe(true)
  })

  it('returns null for invalid or non-mapping YAML', () => {
    expect(parseConfigDraft('addr: [unclosed')).toBeNull()
    expect(parseConfigDraft('- just\n- a\n- list\n')).toBeNull()
    expect(parseConfigDraft('')).toBeNull()
  })
})

describe('config array row identity', () => {
  it('keeps unknown fields and comments with surviving and reordered rows', () => {
    const source = `users:
  - id: alice
    pubkey: aaa
    private_hint: alice-only # Alice hint
  - id: bob
    pubkey: bbb
    future_option: enabled # Bob hint
`
    const draft = parseConfigDraft(source)!
    const users = draft.users as Array<Record<string, any>>
    users.shift()
    const rendered = renderConfigYaml(draft, source)
    expect(rendered).toContain('future_option: enabled # Bob hint')
    expect(rendered).not.toContain('private_hint')
    expect(rendered).not.toContain('Alice hint')
    const reordered = parseConfigDraft(source)!
    ;(reordered.users as unknown[]).reverse()
    const moved = renderConfigYaml(reordered, source)
    expect(moved.indexOf('id: bob')).toBeLessThan(moved.indexOf('id: alice'))
    expect(moved).toContain('future_option: enabled # Bob hint')
    expect(moved).toContain('private_hint: alice-only # Alice hint')
  })
})

describe('configuration rendering reads', () => {
  it('reads absent fields without mutating legacy providers or frozen state', () => {
    const provider = Object.freeze({ family: 'openai', model: 'legacy' })
    expect(getObject(provider, 'auth', { type: 'api_key' })).toEqual({ type: 'api_key' })
    expect(getObject(provider, 'missing')).toEqual({})
    expect(objectArray(provider, 'missing')).toEqual([])
    expect(Object.keys(provider)).toEqual(['family', 'model'])
  })

  it('only persists an authorization object when editing it', () => {
    const draft = parseConfigDraft(
      'model:\n  providers:\n    - family: openai\n      model: legacy\n'
    )!
    const provider = objectArray(getObject(draft, 'model'), 'providers')[0]
    const initial = { type: 'api_key' }
    expect(getObject(provider, 'auth', initial).type).toBe('api_key')
    expect(renderConfigYaml(draft, '')).not.toContain('auth:')
    const auth = ensureObject(provider, 'auth', initial)
    auth.type = 'chatgpt'
    auth.profile = 'saved-account'
    expect(getObject(provider, 'auth')).toBe(auth)
    expect(initial.type).toBe('api_key')
    const rendered = renderConfigYaml(draft, '')
    expect(rendered).toContain('type: chatgpt')
    expect(rendered).toContain('profile: saved-account')
  })
})

it('resolves model field labels after native translations are installed', () => {
  setNativeMessages('en', {})
  const auth = modelProviderFields.find((field) => field.key === 'auth')!
  setNativeMessages('zh_CN', {
    chatgptAuthorization: { message: '授权方式' },
    chatgptAccount: { message: 'ChatGPT 账号' }
  })
  expect(auth.label).toBe('授权方式')
  expect(auth.fields!.find((field) => field.key === 'profile')!.label).toBe('ChatGPT 账号')
  setNativeMessages('en', {})
})
