<script lang="ts">
  import { Bell, Lightbulb, ListTree } from '@lucide/svelte'
  import { getMessage } from '$lib/i18n'
  import ChatActivityDetails from './ChatActivityDetails.svelte'
  import ChatDetailRow from './ChatDetailRow.svelte'
  import { activityCounts, activityLabel, noticeName, noticeSummary } from './chat/activity-summary'
  import type { ActivityRow } from './chat/message-display'
  import { toolCallStatus, toolCallSummary } from './chat/tool-view'

  let { row }: { row: ActivityRow } = $props()
  const only = $derived(row.details.length === 1 ? row.details[0] : undefined)
  const calls = $derived(row.details.filter((detail) => detail.kind === 'tool'))
  const failures = $derived(calls.filter(({ tool }) => toolCallStatus(tool) === 'error').length)
  const thoughtsOnly = $derived(row.details.every((detail) => detail.kind === 'thought'))
  // Say what the run did ("Ran 3 commands and read 2 files"); one call also names its target.
  const label = $derived(activityLabel(activityCounts(row.details)))
  const name = $derived(
    only?.kind === 'notice'
      ? noticeName(only)
      : thoughtsOnly
        ? getMessage('thinkingProcess')
        : label || getMessage('chatActivity')
  )
  const summary = $derived(
    only?.kind === 'notice'
      ? noticeSummary(only)
      : only?.kind === 'tool' && only.tool.name !== 'update_plan'
        ? toolCallSummary(only.tool)
        : ''
  )
</script>

<div id={row.id} class="chat-activity min-w-0">
  <ChatDetailRow
    rowKey={row.id}
    icon={only?.kind === 'notice' ? Bell : thoughtsOnly ? Lightbulb : ListTree}
    {name}
    {summary}
    mono={only?.kind === 'tool'}
    status={only?.kind === 'tool' ? toolCallStatus(only.tool) : undefined}
    issue={failures ? getMessage('toolFailuresCount', String(failures)) : ''}
  >
    <ChatActivityDetails details={row.details} inline={Boolean(only)} />
  </ChatDetailRow>
</div>

<style>
  .chat-activity {
    color: var(--message-muted, #737373);
  }
</style>
