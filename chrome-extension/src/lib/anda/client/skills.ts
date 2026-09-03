import { apiResult, apiResultList, type DaemonApi } from './daemon'
import type {
  ManagedSkill,
  ManagedSkillDetail,
  PromptSkill,
  SkillFileContent,
  SkillSourceInfo,
  SkillValidationResult
} from './types'

/**
 * The skill library, as the side panel and the dashboard use it.
 *
 * Read verbs answer with an empty list when no token is configured, so a view
 * can render before the daemon connects. Every verb that mutates the library
 * (`clone`, `setEnabled`, `deletePersonal`, `reload`) fires `skills-changed` on
 * completion, so open views refresh without polling.
 */
export class SkillsApi extends EventTarget {
  #daemon: DaemonApi

  constructor(daemon: DaemonApi) {
    super()
    this.#daemon = daemon
  }

  /** Skills offered as `/name` completions in the composer, sorted by name. */
  async listPrompts(): Promise<PromptSkill[]> {
    if (!this.#daemon.authorized) {
      return []
    }
    return normalizePromptSkills(
      await apiResultList<PromptSkill>(this.#daemon, 'anda_bot_api', { type: 'ListSkills' })
    )
  }

  /** The directories skills are loaded from, with their diagnostics. */
  async listSources(): Promise<SkillSourceInfo[]> {
    if (!this.#daemon.authorized) {
      return []
    }
    return apiResultList<SkillSourceInfo>(this.#daemon, 'skills_api', {
      type: 'ListSkillSources'
    })
  }

  async list(includeInactive = true): Promise<ManagedSkill[]> {
    if (!this.#daemon.authorized) {
      return []
    }
    return apiResultList<ManagedSkill>(this.#daemon, 'skills_api', {
      type: 'ListSkills',
      include_inactive: includeInactive
    })
  }

  async get(id: string): Promise<ManagedSkillDetail> {
    return apiResult<ManagedSkillDetail>(this.#daemon, 'skills_api', { type: 'GetSkill', id })
  }

  async getFile(id: string, path: string): Promise<SkillFileContent> {
    return apiResult<SkillFileContent>(this.#daemon, 'skills_api', {
      type: 'GetSkillFile',
      id,
      path
    })
  }

  /** Copies a bundled or shared skill into the personal library. */
  async clone(id: string, newName?: string): Promise<ManagedSkillDetail> {
    const detail = await apiResult<ManagedSkillDetail>(this.#daemon, 'skills_api', {
      type: 'CloneSkill',
      id,
      new_name: newName || null
    })
    this.#emitChanged()
    return detail
  }

  async setEnabled(id: string, enabled: boolean): Promise<ManagedSkill[]> {
    const skills = await apiResultList<ManagedSkill>(this.#daemon, 'skills_api', {
      type: 'SetSkillEnabled',
      id,
      enabled
    })
    this.#emitChanged()
    return skills
  }

  async deletePersonal(id: string): Promise<void> {
    await apiResult<{ deleted: boolean }>(this.#daemon, 'skills_api', {
      type: 'DeletePersonalSkill',
      id
    })
    this.#emitChanged()
  }

  /** Checks skill source without installing it; never throws on invalid input. */
  async validate(content: string): Promise<SkillValidationResult> {
    return apiResult<SkillValidationResult>(this.#daemon, 'skills_api', {
      type: 'ValidateSkill',
      content
    })
  }

  /** Rescans every source directory and returns the reloaded library. */
  async reload(): Promise<ManagedSkill[]> {
    const skills = await apiResultList<ManagedSkill>(this.#daemon, 'skills_api', {
      type: 'ReloadSkills'
    })
    this.#emitChanged()
    return skills
  }

  #emitChanged(): void {
    this.dispatchEvent(new Event('skills-changed'))
  }
}

/** Drops unnamed skills, trims the rest, and sorts them for stable completion. */
export function normalizePromptSkills(skills: PromptSkill[] | undefined): PromptSkill[] {
  return (skills || [])
    .filter((skill) => typeof skill?.name === 'string' && Boolean(skill.name.trim()))
    .map((skill) => ({
      name: skill.name.trim(),
      description: skill.description?.trim() || undefined
    }))
    .sort((left, right) => left.name.localeCompare(right.name))
}
