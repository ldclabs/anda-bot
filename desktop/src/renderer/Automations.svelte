<script lang="ts">
  import { focusDialog } from './dialog'
  import { onMount } from 'svelte'
  import { Plus, Clock3, Play, Pause, Trash2, RefreshCw, X, Zap } from '@lucide/svelte'
  import type { DesktopClient } from './client.svelte'
  import type { McpTrigger, McpTriggerState, RpcOutput } from '$lib/anda/client/types'
  import { getMessage } from '$lib/i18n'
  import DropdownMenu from '$lib/anda/DropdownMenu.svelte'
  import { label, type Label } from './labels'
  let { client }: { client: DesktopClient } = $props()
  const t = (key: Label) => label(client.preferences.language, key)
  interface Job {
    _id: number
    name?: string
    job: string
    job_kind: string
    schedule: string
    schedule_kind: string
    tz?: string
    next_run?: number
    paused?: boolean
    completed?: boolean
    last_error?: string
  }
  interface Run {
    _id: number
    started_at: number
    result?: string
    error?: string
  }
  let jobs = $state<Job[]>([])
  // Automations that run on MCP server events; created on the MCP page.
  let triggers = $state<McpTrigger[]>([])
  const triggerStates: Record<McpTriggerState, string> = {
    starting: getMessage('mcpTriggerStateStarting'),
    active: getMessage('mcpTriggerStateActive'),
    retrying: getMessage('mcpTriggerStateRetrying'),
    paused: getMessage('mcpTriggerStatePaused'),
    waiting: getMessage('mcpTriggerStateWaiting'),
    needs_auth: getMessage('mcpTriggerStateNeedsAuth'),
    needs_ingress: getMessage('mcpTriggerStateNeedsIngress'),
    ended: getMessage('mcpTriggerStateEnded')
  }
  let runs = $state<Run[]>([])
  let busy = $state(false)
  let error = $state('')
  let editing = $state(false)
  let editId = $state<number | null>(null)
  let name = $state('')
  let prompt = $state('')
  let kind = $state('agent')
  let scheduleKind = $state('every')
  const kindItems = [
    { value: 'agent', label: 'Agent' },
    { value: 'shell', label: 'Shell' }
  ]
  const scheduleKindItems = [
    { value: 'every', label: 'Every' },
    { value: 'once', label: 'Once' },
    { value: 'cron', label: 'Cron' },
    { value: 'at', label: 'At (RFC3339)' }
  ]
  let schedule = $state('1d')
  let timezone = $state(Intl.DateTimeFormat().resolvedOptions().timeZone)
  let selected = $state<number | null>(null)
  async function load() {
    busy = true
    error = ''
    try {
      jobs =
        (await client.toolCall<RpcOutput<Job[]>>('list_cron_jobs', { limit: 100, cursor: null }))
          .output.result || []
      triggers = await client.mcp.triggers().catch(() => triggers)
    } catch (e) {
      error = String(e)
    } finally {
      busy = false
    }
  }
  onMount(() => {
    void load()
  })
  async function edit(job?: Job) {
    if (busy) return
    busy = true
    error = ''
    try {
      // List responses contain previews, not the complete task payload.
      const full = job
        ? (
            await client.toolCall<RpcOutput<{ job: Job }>>('manage_cron_job', {
              id: job._id,
              action: 'get'
            })
          ).output.result.job
        : undefined
      editId = full?._id || null
      name = full?.name || ''
      prompt = full?.job || ''
      kind = full?.job_kind || 'agent'
      scheduleKind = full?.schedule_kind || 'every'
      schedule = full?.schedule || '1d'
      timezone = full?.tz || Intl.DateTimeFormat().resolvedOptions().timeZone
      editing = true
    } catch (e) {
      error = String(e)
    } finally {
      busy = false
    }
  }
  async function save() {
    busy = true
    error = ''
    try {
      // An empty name clears it on update; creation stores no name.
      const args = {
        name: name.trim(),
        job_kind: kind,
        job: prompt,
        schedule_kind: scheduleKind,
        schedule,
        tz: scheduleKind === 'cron' ? timezone : null
      }
      await client.toolCall(
        editId ? 'update_cron_job' : 'create_cron_job',
        editId ? { ...args, id: editId, origin: false } : args
      )
      editing = false
      await load()
    } catch (e) {
      error = String(e)
    } finally {
      busy = false
    }
  }
  async function manage(job: Job, action: string) {
    if (action === 'remove' && !confirm(`${t('remove')} “${job.name || job._id}”?`)) return
    try {
      await client.toolCall('manage_cron_job', { id: job._id, action })
      await load()
    } catch (e) {
      error = String(e)
    }
  }
  async function manageTrigger(trigger: McpTrigger, action: 'pause' | 'resume' | 'delete') {
    if (action === 'delete' && !confirm(`${t('remove')} “${trigger.name}”?`)) return
    try {
      await client.mcp.applyTrigger(
        action === 'delete'
          ? { op: 'delete', id: trigger.id }
          : { op: 'set_enabled', id: trigger.id, enabled: action === 'resume' }
      )
      await load()
    } catch (e) {
      error = String(e)
    }
  }
  async function history(job: Job) {
    selected = job._id
    try {
      runs =
        (
          await client.toolCall<RpcOutput<Run[]>>('list_cron_runs', {
            job_id: job._id,
            limit: 20,
            cursor: null
          })
        ).output.result || []
    } catch (e) {
      error = String(e)
    }
  }
</script>

<div class="automation-page">
  <div class="page-heading">
    <div>
      <h1>{t('automations')}</h1>
      <p>{t('noJobs')}</p>
    </div>
    <div class="toolbar">
      <button class="icon-button" title={t('refresh')} disabled={busy} onclick={() => void load()}
        ><RefreshCw size={16} /></button
      ><button class="primary" onclick={() => edit()}><Plus size={15} />{t('newJob')}</button>
    </div>
  </div>
  {#if error}<div class="status-banner error">{error}</div>{/if}
  <div class="jobs-list">
    {#each jobs as job}<article class="job-card">
        <button class="job-main" onclick={() => edit(job)}
          ><Clock3 size={20} />
          <div>
            <h2>{job.name || `#${job._id}`}</h2>
            <p>{job.job}</p>
            <span>{job.schedule_kind} · {job.schedule}{job.tz ? ` · ${job.tz}` : ''}</span>
          </div></button
        >
        <div class="job-footer">
          <span
            >{job.completed
              ? t('jobCompleted')
              : job.paused
                ? t('pause')
                : job.next_run
                  ? `${t('nextRun')} ${new Date(job.next_run * 1000).toLocaleString()}`
                  : '—'}</span
          ><button onclick={() => void history(job)}>{t('runHistory')}</button
          >{#if !job.completed}<button
              class="icon-button"
              title={job.paused ? t('resume') : t('pause')}
              onclick={() => void manage(job, job.paused ? 'resume' : 'pause')}
              >{#if job.paused}<Play size={15} />{:else}<Pause size={15} />{/if}</button
            >{/if}<button
            class="icon-button"
            title={t('remove')}
            onclick={() => void manage(job, 'remove')}><Trash2 size={15} /></button
          >
        </div>
        {#if job.last_error}<p class="job-error">{job.last_error}</p>{/if}
      </article>{/each}
  </div>
  {#if !jobs.length && !busy}<div class="empty-automations">
      <Clock3 size={34} />
      <h2>{t('noJobs')}</h2>
      <button onclick={() => edit()}>{t('newJob')}</button>
    </div>{/if}
  <section class="event-automations">
    <div class="page-heading">
      <div>
        <h2>{t('eventAutomations')}</h2>
        <p>{t('eventAutomationsHint')}</p>
      </div>
      <button onclick={() => (client.view = 'mcp')}>{t('openMcp')}</button>
    </div>
    <div class="jobs-list">
      {#each triggers as trigger (trigger.id)}<article class="job-card">
          <div class="job-main">
            <Zap size={20} />
            <div>
              <h2>{trigger.name}</h2>
              <p>{trigger.instructions}</p>
              <span
                >{trigger.event} · {trigger.server_id} · {triggerStates[trigger.state] ||
                  trigger.state}</span
              >
            </div>
          </div>
          <div class="job-footer">
            <span
              >{getMessage('mcpTriggerStats', [
                String(trigger.events_received),
                String(trigger.runs)
              ])}</span
            ><button
              class="icon-button"
              title={trigger.enabled ? t('pause') : t('resume')}
              onclick={() => void manageTrigger(trigger, trigger.enabled ? 'pause' : 'resume')}
              >{#if trigger.enabled}<Pause size={15} />{:else}<Play size={15} />{/if}</button
            ><button
              class="icon-button"
              title={t('remove')}
              onclick={() => void manageTrigger(trigger, 'delete')}><Trash2 size={15} /></button
            >
          </div>
          {#if trigger.last_error}<p class="job-error">{trigger.last_error}</p>{/if}
        </article>{/each}
    </div>
  </section>
  {#if selected}<section class="run-history">
      <div class="page-heading">
        <h2>{t('runHistory')}</h2>
        <button class="icon-button" onclick={() => (selected = null)}><X size={16} /></button>
      </div>
      {#each runs as run}<article>
          <time>{new Date(run.started_at).toLocaleString()}</time>
          <pre>{[run.error, run.result].filter(Boolean).join('\n\n') || '—'}</pre>
        </article>{/each}
    </section>{/if}
</div>
{#if editing}<div class="modal-backdrop">
    <div
      use:focusDialog={() => (editing = false)}
      class="automation-editor"
      role="dialog"
      aria-modal="true"
      aria-label={t('newJob')}
      tabindex="-1"
    >
      <div class="page-heading">
        <h2>{t('newJob')}</h2>
        <button class="icon-button" onclick={() => (editing = false)}><X size={17} /></button>
      </div>
      <label>{t('name')}<input bind:value={name} /></label><label
        >{t('task')}<DropdownMenu
          items={kindItems}
          bind:value={kind}
          ariaLabel={t('task')}
        /><textarea rows="6" bind:value={prompt}></textarea></label
      >
      <div class="schedule-fields">
        <label
          >{t('schedule')}<DropdownMenu
            items={scheduleKindItems}
            bind:value={scheduleKind}
            ariaLabel={t('schedule')}
          /></label
        ><label
          >{t('interval')}<input
            bind:value={schedule}
            placeholder={scheduleKind === 'cron' ? '0 9 * * 1-5' : '1d'}
          /></label
        >
      </div>
      {#if scheduleKind === 'cron'}<label>{t('timezone')}<input bind:value={timezone} /></label
        >{/if}{#if error}<p class="job-error">{error}</p>{/if}
      <div class="dialog-actions">
        <button onclick={() => (editing = false)}>{t('cancel')}</button><button
          class="primary"
          disabled={busy || !prompt.trim() || !schedule.trim()}
          onclick={() => void save()}>{editId ? t('save') : t('create')}</button
        >
      </div>
    </div>
  </div>{/if}
