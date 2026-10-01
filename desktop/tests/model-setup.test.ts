import { readFileSync } from 'node:fs'
import { describe, expect, it } from 'vitest'
import { parse, stringify } from 'yaml'
import { configurePreset, modelPresets, needsModelSetup } from '../src/shared/model-setup'

const template = readFileSync(new URL('../../anda_bot/assets/config.yaml', import.meta.url), 'utf8')
const presets = modelPresets(template)

describe('first-use model setup', () => {
  it('can activate every shipped preset by supplying just its API key', () => {
    expect(presets.length).toBeGreaterThan(1)
    for (const preset of presets) {
      const original = parse(template)
      const updated = parse(configurePreset(template, preset, ' example-key '))
      expect(updated.model.active).toBe(preset.model)
      expect(updated.model.providers).toHaveLength(original.model.providers.length)
      expect(
        updated.model.providers.find((item: { model: string }) => item.model === preset.model)
      ).toEqual({ ...preset, api_key: 'example-key', disabled: false })
      expect(updated.tts).toEqual(original.tts)
      expect(updated.channels).toEqual(original.channels)
    }
  })

  it('preserves unrelated models, secrets, YAML comments and custom sections', () => {
    const original =
      '# keep this comment\n' +
      stringify({
        model: {
          active: 'custom',
          providers: [
            { model: 'custom', api_key: 'existing-key', api_base: 'https://custom.invalid' }
          ]
        },
        channels: { telegram: [{ id: 'owner', bot_token: 'existing-token' }] },
        future_option: { value: 42 }
      })
    const output = configurePreset(original, presets[0]!, 'new-key')
    expect(output).toContain('# keep this comment')
    const result = parse(output)
    expect(result.model.providers[0]).toEqual(parse(original).model.providers[0])
    expect(result.channels).toEqual(parse(original).channels)
    expect(result.future_option).toEqual({ value: 42 })
  })

  it('does not replace a custom endpoint or ambiguous duplicate model', () => {
    const preset = presets[0]!
    const custom = stringify({
      model: { providers: [{ ...preset, api_base: 'https://custom.invalid' }] }
    })
    expect(() => configurePreset(custom, preset, 'key')).toThrow('setupPresetConflict')
    const duplicate = stringify({ model: { providers: [preset, preset] } })
    expect(() => configurePreset(duplicate, preset, 'key')).toThrow('setupPresetConflict')
  })

  it('keeps a same-name ChatGPT model separate from an API provider', () => {
    const preset = presets[0]!
    const chatgpt = { ...preset, auth: { type: 'chatgpt', profile: 'account' } }
    const result = parse(
      configurePreset(stringify({ model: { providers: [chatgpt] } }), preset, 'key')
    )
    expect(result.model.providers).toEqual([
      chatgpt,
      { ...preset, api_key: 'key', disabled: false }
    ])
  })

  it('rejects malformed configuration and empty keys without producing a replacement', () => {
    for (const source of ['model: [', 'model: string', 'model: { providers: invalid }', '- item'])
      expect(() => configurePreset(source, presets[0]!, 'key')).toThrow('setupInvalidConfig')
    expect(() => configurePreset(template, presets[0]!, '  ')).toThrow('setupKeyRequired')
  })

  it('only requests automatic setup for confirmed model issues', () => {
    const base = { connected: false, home: '', baseUrl: '', binary: null }
    expect(needsModelSetup(base)).toBe(false)
    expect(needsModelSetup({ ...base, needsSetup: true })).toBe(false)
    expect(
      needsModelSetup({
        ...base,
        needsSetup: true,
        setupIssues: ['channels.telegram[0].bot_token']
      })
    ).toBe(false)
    expect(
      needsModelSetup({ ...base, needsSetup: true, setupIssues: ['model.providers[0].api_key'] })
    ).toBe(true)
    expect(
      needsModelSetup({
        ...base,
        connected: true,
        needsSetup: false,
        setupIssues: ['model.active']
      })
    ).toBe(false)
  })
})
