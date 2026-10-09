import { getMessage } from '$lib/i18n'

/**
 * What the user reads for a client or conversation status code. The codes stay
 * English because logic matches on them; the display folds them into a few
 * labels, and a failure's details travel separately in the system message. An
 * unknown code (a status a newer daemon added) shows as sent.
 */
export function statusLabel(status: string): string {
  return statusText(status) || status
}

function statusText(status: string): string {
  switch (status) {
    case 'sending':
    case 'submitted':
    case 'working':
    case 'transcribing':
    case 'speaking':
      return getMessage('working')
    case 'syncing':
      return getMessage('syncing')
    case 'starting':
    case 'connecting':
    case 'reconnecting':
      return getMessage('statusConnecting')
    case 'ready':
    case 'idle':
    case 'connected':
      return getMessage('ready')
    case 'completed':
      return getMessage('statusCompleted')
    case 'cancelled':
      return getMessage('statusCancelled')
    case 'disconnected':
    case 'extension unavailable':
      return getMessage('statusUnavailable')
  }
  return status === 'failed' || status.endsWith(' failed') ? getMessage('statusFailed') : ''
}
