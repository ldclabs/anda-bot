<script lang="ts">
  /**
   * The reasoning, tool calls and background updates of one run of work. A
   * settled activity row and the working line both open into this list.
   */
  import { Bell, Bot, BrainCircuit, FileText, Globe, Terminal, Wrench } from '@lucide/svelte'
  import { getMessage } from '$lib/i18n'
  import { renderMarkdown } from '$lib/utils/markdown'
  import ChatDetailRow from './ChatDetailRow.svelte'
  import { noticeName, noticeSummary } from './chat/activity-summary'
  import type { ActivityDetail } from './chat/message-display'
  import { toolCallStatus, toolCallSummary, toolDetailSections, toolKind } from './chat/tool-view'

  let {
    details,
    inline = false
  }: {
    details: ActivityDetail[]
    /** The parent row stands for the only detail: show its body directly. */
    inline?: boolean
  } = $props()

  const icons = {
    shell: Terminal,
    file: FileText,
    memory: BrainCircuit,
    web: Globe,
    agent: Bot,
    tool: Wrench
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

{#each details as detail (detail.id)}
  {#if detail.kind === 'thought'}
    <div
      dir="auto"
      class="md-content activity-thought py-1 text-xs leading-relaxed wrap-break-word"
    >
      {@html renderMarkdown(detail.text)}
    </div>
  {:else if detail.kind === 'tool'}
    <ChatDetailRow
      rowKey={detail.id + ':detail'}
      icon={icons[toolKind(detail.tool.name)]}
      name={detail.tool.name}
      summary={toolCallSummary(detail.tool)}
      status={toolCallStatus(detail.tool)}
      {inline}
      mono
    >
      {@render toolBody(detail)}
    </ChatDetailRow>
  {:else}
    <ChatDetailRow
      rowKey={detail.id + ':detail'}
      icon={Bell}
      name={noticeName(detail)}
      summary={noticeSummary(detail)}
      {inline}
    >
      <pre dir="auto" class="activity-data">{detail.text}</pre>
    </ChatDetailRow>
  {/if}
{/each}

<style>
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
