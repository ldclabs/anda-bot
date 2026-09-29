import { getMessage } from '$lib/i18n'

export function learningLabel(state: string) {
  const labels: Record<string, string> = {
    not_compiled: getMessage('memoryLearning_not_compiled'),
    services_missing: getMessage('memoryLearning_services_missing'),
    identity_mismatch: getMessage('memoryLearning_identity_mismatch'),
    calibration_missing: getMessage('memoryLearning_calibration_missing'),
    awaiting_approval: getMessage('memoryLearning_awaiting_approval'),
    ready: getMessage('memoryLearning_ready'),
    unavailable: getMessage('memoryLearning_unavailable')
  }
  return labels[state] || labels.unavailable
}

export function memoryActivityLabel(state: string): string {
  const labels: Record<string, string> = {
    submitting: getMessage('memoryState_submitting'),
    accepted: getMessage('memoryState_accepted'),
    processing: getMessage('memoryState_processing'),
    completed: getMessage('memoryState_completed'),
    rejected: getMessage('memoryState_rejected'),
    failed: getMessage('memoryState_failed'),
    suppressed: getMessage('memoryState_suppressed'),
    recalled: getMessage('memoryState_recalled'),
    recall_failed: getMessage('memoryState_recall_failed'),
    legacy_unattributed: getMessage('memoryState_legacy_unattributed'),
    unknown: getMessage('memoryState_unknown')
  }
  return labels[state] || labels.unknown
}

/** A claim endpoint's label, with the Brain's own actors named. */
export function actorLabel(label: string): string {
  if (label === '$self') return getMessage('memoryBrainSelf')
  if (label === '$system') return getMessage('memoryBrainSystem')
  return label
}

/** How an entity reads in the Memory views: the caller is "you", and the
 * Brain's own actors have names instead of `$self` and `$system`. */
export function entityName(entity: {
  name: string
  about_owner: boolean
  actor?: string | null
}): string {
  if (entity.about_owner) return getMessage('memoryYou')
  if (entity.actor === 'self') return getMessage('memoryBrainSelf')
  if (entity.actor === 'system') return getMessage('memoryBrainSystem')
  return actorLabel(entity.name)
}
