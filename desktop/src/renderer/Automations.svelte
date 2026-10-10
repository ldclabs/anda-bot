<script lang="ts">
  import { focusDialog } from './dialog'
  import { onMount } from 'svelte'
  import {
    Plus,
    Clock3,
    Play,
    Pause,
    Pencil,
    Plug,
    Trash2,
    RefreshCw,
    X,
    Zap
  } from '@lucide/svelte'
  import type { DesktopClient } from './client.svelte'
  import type {
    McpTrigger,
    McpTriggerDetail,
    McpTriggerState,
    RpcOutput
  } from '$lib/anda/client/types'
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
    last_finished_at?: number
    last_error?: string
  }
  interface Run {
    _id: number
    started_at: number
    result?: string
    error?: string
  }
  type Selection = { kind: 'job' | 'trigger'; id: number }
  type Tone = 'success' | 'warning' | 'danger' | 'muted'
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
  let selection = $state<Selection | null>(null)
  /** The selected job in full: list entries carry previews of the prompt only. */
  let jobDetail = $state<Job | null>(null)
  let runs = $state<Run[]>([])
  let triggerDetail = $state<McpTriggerDetail | null>(null)
  let detailRequest = 0
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
  const selectedJob = $derived(
    selection?.kind === 'job' ? jobs.find((job) => job._id === selection?.id) : undefined
  )
  const selectedTrigger = $derived(
    selection?.kind === 'trigger'
      ? triggers.find((trigger) => trigger.id === selection?.id)
      : undefined
  )
  const fullJob = $derived(jobDetail && jobDetail._id === selectedJob?._id ? jobDetail : null)
  const when = (ms: number) =>
    new Date(ms).toLocaleString(undefined, { dateStyle: 'medium', timeStyle: 'short' })
  function jobStatus(job: Job) {
    return job.completed
      ? t('jobCompleted')
      : job.paused
        ? getMessage('mcpTriggerStatePaused')
        : job.next_run
          ? `${t('nextRun')} ${when(job.next_run * 1000)}`
          : '—'
  }
  function jobTone(job: Job): Tone {
    return job.last_error ? 'danger' : job.completed || job.paused ? 'muted' : 'success'
  }
  function triggerTone(trigger: McpTrigger): Tone {
    return trigger.state === 'active'
      ? 'success'
      : trigger.state === 'ended'
        ? 'danger'
        : trigger.state === 'paused' || trigger.state === 'starting'
          ? 'muted'
          : 'warning'
  }
  async function load() {
    busy = true
    error = ''
    try {
      jobs =
        (await client.toolCall<RpcOutput<Job[]>>('list_cron_jobs', { limit: 100, cursor: null }))
          .output.result || []
      triggers = await client.mcp.triggers().catch(() => triggers)
      // Keep the selection while it exists; otherwise show the first automation.
      const kept =
        selection &&
        (selection.kind === 'job'
          ? jobs.some((job) => job._id === selection?.id)
          : triggers.some((trigger) => trigger.id === selection?.id))
      await select(
        kept
          ? selection
          : jobs[0]
            ? { kind: 'job', id: jobs[0]._id }
            : triggers[0]
              ? { kind: 'trigger', id: triggers[0].id }
              : null,
        true
      )
    } catch (e) {
      error = String(e)
    } finally {
      busy = false
    }
  }
  onMount(() => {
    void load()
  })
  /** Shows an automation; `reload` refetches the one already shown. */
  async function select(next: Selection | null, reload = false) {
    const same = next && selection?.kind === next.kind && selection.id === next.id
    if (same && !reload) return
    selection = next
    const request = ++detailRequest
    // A reload keeps the current details on screen until the new ones arrive.
    if (!same) {
      jobDetail = null
      runs = []
      triggerDetail = null
    }
    if (!next) return
    try {
      if (next.kind === 'job') {
        const [full, history] = await Promise.all([
          client.toolCall<RpcOutput<{ job: Job }>>('manage_cron_job', {
            id: next.id,
            action: 'get'
          }),
          client.toolCall<RpcOutput<Run[]>>('list_cron_runs', {
            job_id: next.id,
            limit: 20,
            cursor: null
          })
        ])
        if (request !== detailRequest) return
        jobDetail = full.output.result.job
        runs = history.output.result || []
      } else {
        const detail = await client.mcp.trigger(next.id)
        if (request !== detailRequest) return
        triggerDetail = detail
      }
    } catch (e) {
      if (request === detailRequest) error = String(e)
    }
  }
  /** Opens the editor; an existing job must be the full one from `select`. */
  function edit(job?: Job) {
    editId = job?._id || null
    name = job?.name || ''
    prompt = job?.job || ''
    kind = job?.job_kind || 'agent'
    scheduleKind = job?.schedule_kind || 'every'
    schedule = job?.schedule || '1d'
    timezone = job?.tz || Intl.DateTimeFormat().resolvedOptions().timeZone
    error = ''
    editing = true
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
      const saved = await client.toolCall<RpcOutput<Job>>(
        editId ? 'update_cron_job' : 'create_cron_job',
        editId ? { ...args, id: editId, origin: false } : args
      )
      editing = false
      const id = saved.output.result?._id
      if (!editId && id) selection = { kind: 'job', id }
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
</script>

{#snippet item(
  Icon: typeof Clock3,
  title: string,
  detail: string,
  tone: Tone,
  active: boolean,
  onclick: () => void
)}
  <button class="automation-item" class:active aria-current={active ? 'true' : undefined} {onclick}>
    <Icon size={15} />
    <span>
      <strong>{title}</strong>
      <small><i class="automation-dot" data-tone={tone}></i>{detail}</small>
    </span>
  </button>
{/snippet}

{#snippet fact(term: string, value: string)}
  <div>
    <dt>{term}</dt>
    <dd>{value}</dd>
  </div>
{/snippet}

<div class="automation-page">
  <aside class="automation-list" aria-label={t('automations')}>
    <div class="automation-list-head">
      <h1>{t('automations')}</h1>
      <div class="toolbar">
        <button
          class="icon-button"
          title={t('refresh')}
          aria-label={t('refresh')}
          disabled={busy}
          onclick={() => void load()}><RefreshCw size={15} /></button
        ><button
          class="icon-button"
          title={t('newJob')}
          aria-label={t('newJob')}
          onclick={() => edit()}><Plus size={16} /></button
        >
      </div>
    </div>
    <div class="automation-list-body">
      <h2>{t('scheduledAutomations')}</h2>
      {#each jobs as job (job._id)}
        {@render item(
          Clock3,
          job.name || `#${job._id}`,
          jobStatus(job),
          jobTone(job),
          selection?.kind === 'job' && selection.id === job._id,
          () => void select({ kind: 'job', id: job._id })
        )}
      {:else}
        {#if !busy}<button class="automation-add" onclick={() => edit()}
            ><Plus size={14} />{t('newJob')}</button
          >{/if}
      {/each}
      <h2>{t('eventAutomations')}</h2>
      {#each triggers as trigger (trigger.id)}
        {@render item(
          Zap,
          trigger.name,
          `${trigger.server_id} · ${triggerStates[trigger.state] || trigger.state}`,
          triggerTone(trigger),
          selection?.kind === 'trigger' && selection.id === trigger.id,
          () => void select({ kind: 'trigger', id: trigger.id })
        )}
      {:else}
        <p>{t('eventAutomationsHint')}</p>
      {/each}
    </div>
    <div class="automation-list-foot">
      <button onclick={() => (client.view = 'mcp')}><Plug size={14} />{t('openMcp')}</button>
    </div>
  </aside>

  <section class="automation-detail">
    {#if selectedJob}
      {@const job = fullJob || selectedJob}
      <header class="automation-detail-head">
        <div>
          <h2>{job.name || `#${job._id}`}</h2>
          <p>
            <i class="automation-dot" data-tone={jobTone(job)}></i>{job.job_kind === 'shell'
              ? 'Shell'
              : 'Agent'} · {jobStatus(job)}
          </p>
        </div>
        <div class="automation-actions">
          <button class="dialog-button" disabled={!fullJob} onclick={() => edit(fullJob!)}
            ><Pencil size={14} />{t('edit')}</button
          >{#if !job.completed}<button
              class="dialog-button"
              onclick={() => void manage(job, job.paused ? 'resume' : 'pause')}
              >{#if job.paused}<Play size={14} />{t('resume')}{:else}<Pause size={14} />{t(
                  'pause'
                )}{/if}</button
            >{/if}<button class="danger" onclick={() => void manage(job, 'remove')}
            ><Trash2 size={14} />{t('remove')}</button
          >
        </div>
      </header>
      {#if error}<div class="status-banner error">{error}</div>{/if}
      <div class="automation-detail-body">
        <div class="automation-columns">
          <div>
            <dl class="automation-facts">
              {@render fact(t('schedule'), `${job.schedule_kind} · ${job.schedule}`)}
              {#if job.tz}{@render fact(t('timezone'), job.tz)}{/if}
              {@render fact(
                t('nextRun'),
                job.next_run && !job.paused && !job.completed ? when(job.next_run * 1000) : '—'
              )}
              {@render fact(t('lastRun'), job.last_finished_at ? when(job.last_finished_at) : '—')}
            </dl>
            {#if job.last_error}<p class="job-error">{job.last_error}</p>{/if}
            <section class="automation-section">
              <h3>{t('task')}</h3>
              <pre class="automation-text">{job.job}</pre>
            </section>
          </div>
          <section class="automation-section automation-runs">
            <h3>{t('runHistory')}</h3>
            {#each runs as run (run._id)}<article>
                <time>{when(run.started_at)}</time>
                {#if run.error}<pre class="failed">{run.error}</pre>{/if}
                {#if run.result || !run.error}<pre>{run.result || '—'}</pre>{/if}
              </article>{:else}<p>{getMessage('mcpTriggerNoRuns')}</p>{/each}
          </section>
        </div>
      </div>
    {:else if selectedTrigger}
      {@const trigger = selectedTrigger}
      <header class="automation-detail-head">
        <div>
          <h2>{trigger.name}</h2>
          <p>
            <i class="automation-dot" data-tone={triggerTone(trigger)}></i>{triggerStates[
              trigger.state
            ] || trigger.state} · {trigger.server_id}
          </p>
        </div>
        <div class="automation-actions">
          <button class="dialog-button" onclick={() => (client.view = 'mcp')}
            ><Plug size={14} />{t('openMcp')}</button
          ><button
            class="dialog-button"
            onclick={() => void manageTrigger(trigger, trigger.enabled ? 'pause' : 'resume')}
            >{#if trigger.enabled}<Pause size={14} />{t('pause')}{:else}<Play size={14} />{t(
                'resume'
              )}{/if}</button
          ><button class="danger" onclick={() => void manageTrigger(trigger, 'delete')}
            ><Trash2 size={14} />{t('remove')}</button
          >
        </div>
      </header>
      {#if error}<div class="status-banner error">{error}</div>{/if}
      <div class="automation-detail-body">
        <div class="automation-columns">
          <div>
            <dl class="automation-facts">
              {@render fact(
                getMessage('mcpTriggerDelivery'),
                trigger.mode ||
                  (trigger.delivery === 'auto'
                    ? getMessage('mcpTriggerDeliveryAuto')
                    : trigger.delivery)
              )}
              {@render fact(t('lastRun'), trigger.last_run_at ? when(trigger.last_run_at) : '—')}
            </dl>
            <p class="automation-meta">
              <code>{trigger.event}</code><span
                >{getMessage('mcpTriggerStats', [
                  String(trigger.events_received),
                  String(trigger.runs)
                ])}</span
              >{#if trigger.pending}<span
                  >{getMessage('mcpTriggerPending', String(trigger.pending))}</span
                >{/if}{#if trigger.last_event_at}<span
                  >{getMessage('mcpTriggerLastEvent', when(trigger.last_event_at))}</span
                >{/if}
            </p>
            {#if trigger.last_error}<p class="job-error">{trigger.last_error}</p>{/if}
            {#if trigger.missed_events_at}<p class="job-warning">
                {getMessage('mcpTriggerMissed', when(trigger.missed_events_at))}
              </p>{/if}
            <section class="automation-section">
              <h3>{getMessage('mcpTriggerInstructions')}</h3>
              <pre class="automation-text">{trigger.instructions}</pre>
              {#if Object.keys(trigger.arguments || {}).length}<code
                  >{JSON.stringify(trigger.arguments)}</code
                >{/if}
            </section>
          </div>
          <section class="automation-section automation-runs">
            <h3>{getMessage('mcpTriggerRuns')}</h3>
            {#each triggerDetail?.id === trigger.id ? triggerDetail.runs_recent : [] as run (run.id)}<article
              >
                <time>{when(run.started_at)}</time>
                {#if run.error}<pre class="failed">{getMessage('mcpTriggerRunFailed', [
                      String(run.events),
                      run.error
                    ])}</pre>{:else}<pre>{getMessage(
                      'mcpTriggerRunOk',
                      String(run.events)
                    )}{run.result ? `\n\n${run.result}` : ''}</pre>{/if}
              </article>{:else}<p>{getMessage('mcpTriggerNoRuns')}</p>{/each}
          </section>
        </div>
      </div>
    {:else}
      {#if error}<div class="status-banner error">{error}</div>{/if}
      {#if !busy}<div class="empty-automations">
          <Clock3 size={34} />
          <h2>{t('noJobs')}</h2>
          <button class="primary" onclick={() => edit()}><Plus size={15} />{t('newJob')}</button>
        </div>{/if}
    {/if}
  </section>
</div>
{#if editing}<div class="modal-backdrop">
    <div
      use:focusDialog={() => (editing = false)}
      class="automation-editor"
      role="dialog"
      aria-modal="true"
      aria-label={editId ? t('edit') : t('newJob')}
      tabindex="-1"
    >
      <div class="page-heading">
        <h2>{editId ? t('edit') : t('newJob')}</h2>
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
