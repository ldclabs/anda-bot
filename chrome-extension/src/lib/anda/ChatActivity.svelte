<script lang="ts">
  import {
    Bell,
    Bot,
    BrainCircuit,
    FileText,
    Globe,
    Lightbulb,
    ListTree,
    Terminal,
    Wrench
  } from '@lucide/svelte'
  import { getMessage } from '$lib/i18n'
  import { renderMarkdown } from '$lib/utils/markdown'
  import ChatDetailRow from './ChatDetailRow.svelte'
  import type { ActivityDetail, TranscriptRow } from './chat/message-display'
  import {
    firstLine,
    toolCallStatus,
    toolCallSummary,
    toolDetailSections,
    toolKind
  } from './chat/tool-view'

  let { row }: { row: Extract<TranscriptRow, { kind: 'activity' }> } = $props()
  const only = $derived(row.details.length === 1 ? row.details[0] : undefined)
  const calls = $derived(row.details.filter((detail) => detail.kind === 'tool'))
  const failures = $derived(calls.filter(({ tool }) => toolCallStatus(tool) === 'error').length)
  const thoughtsOnly = $derived(row.details.every((detail) => detail.kind === 'thought'))
  const icons = {
    shell: Terminal,
    file: FileText,
    memory: BrainCircuit,
    web: Globe,
    agent: Bot,
    tool: Wrench
  }

  type Notice = Extract<ActivityDetail, { kind: 'notice' }>
  function noticeName(notice: Notice) {
    return notice.name === 'background shell'
      ? getMessage('backgroundShellNotice')
      : notice.name || getMessage('runtimeNotice')
  }
  function noticeSummary(notice: Notice) {
    return notice.name.startsWith('background ') ? '' : firstLine(notice.text)
  }
</script>

{#snippet toolBody(detail: Extract<ActivityDetail, { kind: 'tool' }>)}
  {#each toolDetailSections(detail.tool) as section (section.kind)}
    <div class="grid min-w-0 gap-0.5 py-0.5">
      <span class="text-[11px] font-medium">
        {section.kind === 'input'
          ? getMessage('toolInput')
          : section.kind === 'output'
            ? getMessage('toolOutput')
            : 'stderr'}{section.meta ? ' · ' + section.meta : ''}
      </span>
      <pre dir="auto" class="activity-data">{section.text}</pre>
    </div>
  {/each}
{/snippet}

<div id={row.id} class="chat-activity min-w-0">
  <ChatDetailRow
    rowKey={row.id}
    icon={only?.kind === 'notice' ? Bell : thoughtsOnly ? Lightbulb : ListTree}
    name={only?.kind === 'notice'
      ? noticeName(only)
      : getMessage(thoughtsOnly ? 'thinkingProcess' : 'chatActivity')}
    summary={only?.kind === 'notice'
      ? noticeSummary(only)
      : calls.length
        ? getMessage('toolCallsCount', String(calls.length))
        : ''}
    status={only?.kind === 'tool' ? toolCallStatus(only.tool) : undefined}
    issue={failures ? getMessage('toolFailuresCount', String(failures)) : ''}
  >
    {#each row.details as detail (detail.id)}
      {#if detail.kind === 'thought'}
        <div dir="auto" class="md-content activity-thought py-1 text-xs leading-relaxed wrap-break-word">
          {@html renderMarkdown(detail.text)}
        </div>
      {:else if detail.kind === 'tool'}
        {#if only}
          <span dir="auto" class="font-mono text-xs font-medium">{detail.tool.name}</span>
          {@render toolBody(detail)}
        {:else}
          <ChatDetailRow
            rowKey={detail.id + ':detail'}
            icon={icons[toolKind(detail.tool.name)]}
            name={detail.tool.name}
            summary={toolCallSummary(detail.tool)}
            status={toolCallStatus(detail.tool)}
            mono
          >
            {@render toolBody(detail)}
          </ChatDetailRow>
        {/if}
      {:else if only}
        <pre dir="auto" class="activity-data">{detail.text}</pre>
      {:else}
        <ChatDetailRow
          rowKey={detail.id + ':detail'}
          icon={Bell}
          name={noticeName(detail)}
          summary={noticeSummary(detail)}
        >
          <pre dir="auto" class="activity-data">{detail.text}</pre>
        </ChatDetailRow>
      {/if}
    {/each}
  </ChatDetailRow>
</div>

<style>
  .chat-activity {
    color: var(--message-muted, #737373);
  }
  .activity-thought,
  .activity-data {
    color: var(--message-text, #272727);
  }
  .activity-data {
    max-height: 18rem;
    overflow: auto;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    border: 1px solid var(--message-border, #e6e6e6);
    border-radius: 0.375rem;
    background: var(--message-surface, #f5f5f5);
    padding: 0.375rem 0.5rem;
    font: 0.75rem/1.6 var(--font-mono, monospace);
  }
</style>
