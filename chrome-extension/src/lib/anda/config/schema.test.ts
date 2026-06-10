import { describe, expect, it } from 'vitest'
import { normalizeConfigDraft, renderConfigYaml } from './schema'

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
    const draft = normalizeConfigDraft({
      addr: '127.0.0.1:8042',
      log_level: 'info',
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
    })

    const rendered = renderConfigYaml(draft, source)

    for (const line of source.split('\n').filter((item) => item.trimStart().startsWith('#'))) {
      expect(rendered).toContain(line)
    }
    expect(rendered).toContain('log_level: info')
    expect(rendered).toContain('channels:')
  })
})
