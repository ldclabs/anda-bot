import { describe, expect, it } from 'vitest'
import { normalizeConfigDraft, parseConfigDraft, renderConfigYaml } from './schema'

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
    initial_prompt: null,
    max_duration_secs: 120,
    transcribe_non_ptt_audio: false
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
    const rendered = renderConfigYaml(normalizeConfigDraft({ ...baseDraft, log_level: 'debug' }), source)

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
