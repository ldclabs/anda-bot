import { describe, expect, it, vi } from 'vitest'
import type { DaemonApi } from './daemon'
import { SkillsApi, normalizePromptSkills } from './skills'

function createDaemon(result: unknown, overrides: Partial<DaemonApi> = {}) {
  const toolCall = vi.fn(async () => ({ output: { result }, usage: {} }) as never)
  const daemon: DaemonApi = {
    authorized: true,
    rpc: vi.fn(async () => undefined as never),
    toolCall,
    ...overrides
  }
  return { daemon, toolCall }
}

describe('normalizePromptSkills', () => {
  it('trims, drops the unnamed, and sorts by name', () => {
    expect(
      normalizePromptSkills([
        { name: '  zebra ', description: '  stripes  ' },
        { name: '', description: 'dropped' },
        { name: 'alpha', description: '   ' },
        { name: '   ', description: 'dropped too' }
      ])
    ).toEqual([
      { name: 'alpha', description: undefined },
      { name: 'zebra', description: 'stripes' }
    ])
  })

  it('treats a missing list as empty', () => {
    expect(normalizePromptSkills(undefined)).toEqual([])
  })
})

describe('SkillsApi reads', () => {
  it('normalizes prompt skills from anda_bot_api', async () => {
    const { daemon, toolCall } = createDaemon([{ name: ' b ' }, { name: 'a' }])
    const api = new SkillsApi(daemon)

    expect(await api.listPrompts()).toEqual([
      { name: 'a', description: undefined },
      { name: 'b', description: undefined }
    ])
    expect(toolCall).toHaveBeenCalledWith('anda_bot_api', { type: 'ListSkills' })
  })

  it('asks skills_api for the managed library', async () => {
    const { daemon, toolCall } = createDaemon([])
    const api = new SkillsApi(daemon)

    await api.list(false)

    expect(toolCall).toHaveBeenCalledWith('skills_api', {
      type: 'ListSkills',
      include_inactive: false
    })
  })

  it('coerces a null list into an empty array', async () => {
    const { daemon } = createDaemon(null)
    const api = new SkillsApi(daemon)

    expect(await api.list()).toEqual([])
    expect(await api.listSources()).toEqual([])
  })

  it('skips the daemon entirely without a token', async () => {
    const { daemon, toolCall } = createDaemon([], { authorized: false })
    const api = new SkillsApi(daemon)

    expect(await api.listPrompts()).toEqual([])
    expect(await api.listSources()).toEqual([])
    expect(await api.list()).toEqual([])
    expect(toolCall).not.toHaveBeenCalled()
  })

  it('reads one skill and one of its files', async () => {
    const { daemon, toolCall } = createDaemon({ id: 'demo' })
    const api = new SkillsApi(daemon)

    await api.get('demo')
    expect(toolCall).toHaveBeenLastCalledWith('skills_api', { type: 'GetSkill', id: 'demo' })

    await api.getFile('demo', 'SKILL.md')
    expect(toolCall).toHaveBeenLastCalledWith('skills_api', {
      type: 'GetSkillFile',
      id: 'demo',
      path: 'SKILL.md'
    })
  })
})

describe('SkillsApi mutations', () => {
  it('announces skills-changed after every library mutation', async () => {
    const { daemon } = createDaemon([])
    const api = new SkillsApi(daemon)
    const changed = vi.fn()
    api.addEventListener('skills-changed', changed)

    await api.clone('demo', 'demo-copy')
    await api.setEnabled('demo', false)
    await api.deletePersonal('demo')
    await api.reload()

    expect(changed).toHaveBeenCalledTimes(4)
  })

  it('does not announce a change for a read or a validation', async () => {
    const { daemon } = createDaemon([])
    const api = new SkillsApi(daemon)
    const changed = vi.fn()
    api.addEventListener('skills-changed', changed)

    await api.list()
    await api.validate('# Skill')

    expect(changed).not.toHaveBeenCalled()
  })

  it('sends a null new_name when cloning without one', async () => {
    const { daemon, toolCall } = createDaemon({ id: 'demo' })
    const api = new SkillsApi(daemon)

    await api.clone('demo')

    expect(toolCall).toHaveBeenCalledWith('skills_api', {
      type: 'CloneSkill',
      id: 'demo',
      new_name: null
    })
  })

  it('propagates a daemon failure instead of announcing a change', async () => {
    const { daemon } = createDaemon(null, {
      toolCall: vi.fn(async () => {
        throw new Error('skill is read-only')
      })
    })
    const api = new SkillsApi(daemon)
    const changed = vi.fn()
    api.addEventListener('skills-changed', changed)

    await expect(api.deletePersonal('bundled')).rejects.toThrow('skill is read-only')
    expect(changed).not.toHaveBeenCalled()
  })
})
