import type { PromptSkill } from './types'

export function normalizePromptSkills(skills: PromptSkill[] | undefined): PromptSkill[] {
	return (skills || [])
		.filter((skill) => typeof skill?.name === 'string' && Boolean(skill.name.trim()))
		.map((skill) => ({
			name: skill.name.trim(),
			description: skill.description?.trim() || undefined
		}))
		.sort((left, right) => left.name.localeCompare(right.name))
}
