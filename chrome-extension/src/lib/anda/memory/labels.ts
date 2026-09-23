import { getMessage } from '$lib/i18n'

export function memoryModeLabel(mode: string) {
  const labels: Record<string, string> = {
    standard: getMessage('memoryMode_standard'),
    no_store: getMessage('memoryMode_no_store'),
    off: getMessage('memoryMode_off'),
    unknown: getMessage('memoryMode_unknown')
  }
  return labels[mode] || labels.unknown
}
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
