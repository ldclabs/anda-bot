<script lang="ts">
  /**
   * The owner's MCP servers: their state, their tools and when the agent asks
   * before calling them, the automations that run on their events, their
   * sign-in and secrets, and adding new ones.
   * Shared by the extension dashboard (`#mcp`) and Anda Desktop.
   *
   * Everything goes through the daemon's owner-only `mcp_*` methods; secret
   * values are written, never read back. The page polls the snapshot while it
   * is visible, since servers connect and fail in the background.
   */
  import { useAndaClient } from '$lib/anda/client/context'
  import {
    emptySnapshot,
    localEntry,
    moveCredentialsToSecrets,
    parseMcpConfig,
    remoteEntry,
    secretReferences,
    suggestServerId
  } from '$lib/anda/client/mcp'
  import { openExternalUrl, readClientState, storeClientState } from '$lib/anda/client/platform'
  import type {
    Json,
    McpApproval,
    McpChange,
    McpEntry,
    McpReceipt,
    McpSecretView,
    McpServerOptions,
    McpServerDetail,
    McpServerView,
    McpSnapshot,
    McpStatus,
    McpTestReport,
    McpToolDiff,
    McpToolView
  } from '$lib/anda/client/types'
  import DropdownMenu from '$lib/anda/DropdownMenu.svelte'
  import Modal from '$lib/anda/Modal.svelte'
  import McpEventsPanel from './McpEventsPanel.svelte'
  import McpImportDialog from './McpImportDialog.svelte'
  import McpOptionsForm from './McpOptionsForm.svelte'
  import McpRegistryDialog from './McpRegistryDialog.svelte'
  import { badgeClass, buttonClass, inputClass, textareaClass } from '$lib/anda/ui'
  import { getMessage } from '$lib/i18n'
  import { errorToMessage } from '$lib/service-worker/settings'
  import { cn } from '$lib/utils'
  import {
    AlertTriangle,
    Check,
    Download,
    ExternalLink,
    Eye,
    EyeOff,
    KeyRound,
    LoaderCircle,
    LogIn,
    LogOut,
    Plug,
    Plus,
    Power,
    RefreshCw,
    Search,
    Store,
    ShieldCheck,
    Trash2,
    X
  } from '@lucide/svelte'
  import { onMount, untrack } from 'svelte'

  let {
    installLink = null,
    onInstallLinkHandled
  }: {
    /** A server offered by an `anda://mcp/install` link: opened in the add dialog, never saved by itself. */
    installLink?: { name: string; config: string } | null
    onInstallLinkHandled?: () => void
  } = $props()

  const andaClient = useAndaClient()
  const mcp = andaClient.mcp

  type DetailTab = 'overview' | 'tools' | 'events' | 'access' | 'config'
  type ListFilter = 'all' | 'attention' | 'ready' | 'disabled'
  type AddMode = 'url' | 'command' | 'json'
  type AddAction = 'add' | 'import' | 'registry'
  type ToolApproval = McpApproval | 'inherit'

  /** The secrets list takes the place of a server in the detail pane. */
  const SECRETS = '$secrets'
  const POLL_MS = 5000
  const SIGN_IN_WAIT_MS = 5 * 60_000
  const noticeStorageKey = 'mcpApprovalNoticeDismissed'

  let snapshot = $state<McpSnapshot>(emptySnapshot())
  let secrets = $state<McpSecretView[]>([])
  let loaded = $state(false)
  let selectedId = $state('')
  let detail = $state<McpServerDetail | null>(null)
  let detailLoading = $state(false)
  let activeTab = $state<DetailTab>('overview')
  let search = $state('')
  let filter = $state<ListFilter>('all')
  let busy = $state('')
  let error = $state('')
  let notice = $state('')
  let noticeDismissed = $state(true)
  let diffs = $state<Record<string, McpToolDiff | 'loading'>>({})
  let secretDrafts = $state<Record<string, string>>({})
  let newSecretName = $state('')
  let newSecretValue = $state('')
  let removeOpen = $state(false)
  let signIn = $state<{ id: string; url: string; until: number } | null>(null)
  let detailRequest = 0

  // The add dialog.
  let addOpen = $state(false)
  let addMode = $state<AddMode>('url')
  let addId = $state('')
  let addUrl = $state('')
  let addHeaders = $state('')
  let addCommand = $state('')
  let addEnv = $state('')
  let addJson = $state('')
  let storeAsSecrets = $state(true)
  let tests = $state<Record<string, McpTestReport | 'testing'>>({})
  /** The dialog holds what a link offered, which the owner checks first. */
  let addFromLink = $state(false)
  let importOpen = $state(false)
  let registryOpen = $state(false)

  // A link opens the add dialog with its configuration; nothing is saved
  // until the owner adds it.
  $effect(() => {
    const link = installLink
    if (!link) return
    untrack(() => {
      openAdd()
      addMode = 'json'
      addJson = link.config
      addId = link.name
      addFromLink = true
      onInstallLinkHandled?.()
    })
  })

  const statusLabels: Record<McpStatus, string> = {
    disabled: getMessage('mcpStatusDisabled'),
    invalid: getMessage('mcpStatusInvalid'),
    connecting: getMessage('mcpStatusConnecting'),
    ready: getMessage('mcpStatusReady'),
    needs_auth: getMessage('mcpStatusNeedsAuth'),
    failed: getMessage('mcpStatusFailed'),
    disconnected: getMessage('mcpStatusDisconnected'),
    unknown: getMessage('mcpStatusUnknown')
  }
  const approvalLabels: Record<McpApproval, string> = {
    auto: getMessage('mcpApprovalAuto'),
    ask: getMessage('mcpApprovalAsk'),
    allow: getMessage('mcpApprovalAllow')
  }
  const approvalDetails: Record<McpApproval, string> = {
    auto: getMessage('mcpApprovalAutoDetail'),
    ask: getMessage('mcpApprovalAskDetail'),
    allow: getMessage('mcpApprovalAllowDetail')
  }
  const serverApprovalItems = (['auto', 'ask', 'allow'] as McpApproval[]).map((value) => ({
    value,
    label: approvalLabels[value],
    description: approvalDetails[value]
  }))
  const filterItems: { value: ListFilter; label: string }[] = [
    { value: 'all', label: getMessage('mcpFilterAll') },
    { value: 'attention', label: getMessage('mcpFilterAttention') },
    { value: 'ready', label: getMessage('mcpFilterReady') },
    { value: 'disabled', label: getMessage('mcpFilterDisabled') }
  ]
  const detailTabs: { value: DetailTab; label: string }[] = [
    { value: 'overview', label: getMessage('mcpTabOverview') },
    { value: 'tools', label: getMessage('mcpTabTools') },
    { value: 'events', label: getMessage('mcpTabEvents') },
    { value: 'access', label: getMessage('mcpTabAccess') },
    { value: 'config', label: getMessage('mcpTabConfig') }
  ]
  const addMenuItems: { value: AddAction; label: string; description: string }[] = [
    {
      value: 'add',
      label: getMessage('mcpAddServer'),
      description: getMessage('mcpAddByHandDetail')
    },
    {
      value: 'import',
      label: getMessage('mcpImportTitle'),
      description: getMessage('mcpImportMenuDetail')
    },
    {
      value: 'registry',
      label: getMessage('mcpRegistryBrowse'),
      description: getMessage('mcpRegistryMenuDetail')
    }
  ]
  const addModes: { value: AddMode; label: string }[] = [
    { value: 'url', label: getMessage('mcpAddRemote') },
    { value: 'command', label: getMessage('mcpAddLocal') },
    { value: 'json', label: getMessage('mcpAddJson') }
  ]

  const visibleServers = $derived.by(() => {
    const query = search.trim().toLowerCase()
    return snapshot.servers.filter(
      (server) =>
        matchesFilter(server, filter) &&
        (!query ||
          server.id.toLowerCase().includes(query) ||
          (server.title || '').toLowerCase().includes(query) ||
          server.summary.toLowerCase().includes(query))
    )
  })
  const selected = $derived(snapshot.servers.find((server) => server.id === selectedId) || null)
  const reviewCount = $derived(
    (detail?.tools.filter((tool) => tool.review && tool.review !== 'trusted').length ?? 0) +
      (detail?.instructions_changed ? 1 : 0)
  )
  const serverSecrets = $derived(
    selected ? secrets.filter((secret) => secret.used_by.includes(selected.id)) : []
  )
  const visibleTools = $derived(detail?.tools.filter((tool) => !tool.hidden) ?? [])
  const hiddenTools = $derived(detail?.tools.filter((tool) => tool.hidden) ?? [])
  const draft = $derived.by(() => buildDraft())

  onMount(() => {
    void readClientState([noticeStorageKey])
      .then((saved) => (noticeDismissed = saved[noticeStorageKey] === true))
      .catch(() => (noticeDismissed = false))
    andaClient
      .init({ conversations: false })
      .catch(() => undefined)
      .finally(() => void run('load', () => refresh().then(() => '')))
    const changed = () => void refresh()
    mcp.addEventListener('mcp-changed', changed)
    const timer = window.setInterval(() => {
      if (document.visibilityState === 'visible' && !busy) void refresh()
    }, POLL_MS)
    return () => {
      mcp.removeEventListener('mcp-changed', changed)
      window.clearInterval(timer)
    }
  })

  /** Runs one change at a time, reporting its outcome in the banner. */
  async function run(action: string, work: () => Promise<string>) {
    if (busy) return
    busy = action
    error = ''
    notice = ''
    try {
      notice = await work()
    } catch (err) {
      error = errorToMessage(err)
    } finally {
      busy = ''
    }
  }

  /** Re-reads the servers and secrets, and the server on show. */
  async function refresh() {
    const next = await mcp.list().catch((err) => {
      if (!loaded) error = errorToMessage(err)
      return null
    })
    if (!next) return
    snapshot = next
    loaded = true
    if (next.running) {
      secrets = await mcp.secrets().catch(() => secrets)
    }
    if (selectedId !== SECRETS && !next.servers.some((server) => server.id === selectedId)) {
      selectedId = next.servers[0]?.id || ''
      detail = null
      activeTab = 'overview'
    }
    if (selectedId && selectedId !== SECRETS) await loadDetail(selectedId)
    finishSignIn()
  }

  async function loadDetail(id: string) {
    const request = ++detailRequest
    detailLoading = detail?.id !== id
    try {
      const next = await mcp.get(id)
      if (request === detailRequest) detail = next
    } catch (err) {
      if (request === detailRequest) {
        detail = null
        error = errorToMessage(err)
      }
    } finally {
      if (request === detailRequest) detailLoading = false
    }
  }

  function select(id: string) {
    if (id === selectedId) return
    selectedId = id
    detail = null
    diffs = {}
    activeTab = 'overview'
    error = ''
    notice = ''
    if (id !== SECRETS) void loadDetail(id)
  }

  /** Makes a change to mcp.json or the secrets and reports what it did. */
  function change(action: string, request: McpChange, done: string) {
    void run(action, async () => {
      // A change to mcp.json is refused when the file changed underneath.
      const revision = 'id' in request && request.op !== 'mark_reviewed' ? snapshot.revision : ''
      const receipt = await mcp.apply(request, revision || undefined)
      await refresh()
      return receiptText(receipt, done)
    })
  }

  /** `done`, or the failures and warnings a change reported. */
  function receiptText(receipt: McpReceipt, done: string): string {
    const failures = (receipt.failed || []).map((failure) => `${failure.id}: ${failure.message}`)
    const problems = [...failures, ...(receipt.warnings || [])]
    if (problems.length) throw new Error(problems.join('\n'))
    return done
  }

  function reload() {
    void run('reload', async () => {
      receiptText(await mcp.reload(), '')
      await refresh()
      return getMessage('mcpReloaded')
    })
  }

  function reconnect(id: string) {
    void run('reconnect', async () => {
      const receipt = await mcp.reconnect(id)
      await refresh()
      return receiptText(receipt, getMessage('mcpReconnected', id))
    })
  }

  function setEnabled(server: McpServerView, enabled: boolean) {
    change(
      'enable',
      { op: 'set_enabled', id: server.id, enabled },
      getMessage(enabled ? 'mcpEnabled' : 'mcpDisabledNotice', server.id)
    )
  }

  function remove() {
    removeOpen = false
    const server = selected
    if (!server) return
    void run('remove', async () => {
      const receipt = await mcp.apply(
        { op: 'remove', id: server.id },
        snapshot.revision || undefined
      )
      await refresh()
      const removed = receipt.secrets_removed?.length
        ? ` ${getMessage('mcpSecretsRemoved', receipt.secrets_removed.join(', '))}`
        : ''
      return receiptText(receipt, getMessage('mcpRemoved', server.id) + removed)
    })
  }

  function startSignIn(id: string, reauthorize = false) {
    void run('signin', () => beginSignIn(id, reauthorize))
  }

  /** Starts an OAuth sign-in and opens its page; the poll notices when it lands. */
  async function beginSignIn(id: string, reauthorize: boolean): Promise<string> {
    const result = await mcp.signIn({ id, reauthorize })
    if (result.status === 'connected') {
      await refresh()
      return getMessage('mcpSignedIn', id)
    }
    const url = result.authorization_url || ''
    signIn = { id, url, until: Date.now() + SIGN_IN_WAIT_MS }
    await openExternalUrl(url).catch(() => undefined)
    return ''
  }

  function finishSignIn() {
    if (!signIn) return
    const server = snapshot.servers.find((item) => item.id === signIn?.id)
    if (server?.status === 'ready') {
      notice = getMessage('mcpSignedIn', server.id)
      signIn = null
    } else if (Date.now() > signIn.until) {
      signIn = null
    }
  }

  function signOut(id: string) {
    void run('signout', async () => {
      await mcp.signOut(id)
      await refresh()
      return getMessage('mcpSignedOut', id)
    })
  }

  function setToolApproval(tool: McpToolView, value: ToolApproval) {
    if (!selected) return
    change(
      'approval',
      {
        op: 'set_approval',
        id: selected.id,
        tool: tool.remote_name,
        approval: value === 'inherit' ? null : value
      },
      getMessage('mcpPolicySaved')
    )
  }

  function setToolVisible(tool: string, visible: boolean) {
    if (!selected) return
    change(
      'visibility',
      { op: 'set_tool_visible', id: selected.id, tool, visible },
      getMessage(visible ? 'mcpToolShown' : 'mcpToolHidden', tool)
    )
  }

  function markReviewed(tools: string[] = []) {
    if (!selected) return
    diffs = {}
    change('review', { op: 'mark_reviewed', id: selected.id, tools }, getMessage('mcpReviewed'))
  }

  async function toggleDiff(tool: string) {
    if (!selected) return
    if (diffs[tool]) {
      delete diffs[tool]
      return
    }
    diffs[tool] = 'loading'
    try {
      diffs[tool] = await mcp.toolDiff(selected.id, tool)
    } catch (err) {
      delete diffs[tool]
      error = errorToMessage(err)
    }
  }

  function saveSecret(name: string, value: string | null) {
    const trimmed = value?.trim() ?? null
    if (value !== null && !trimmed) return
    void run('secret', async () => {
      const receipt = await mcp.apply({ op: 'set_secret', name, value: trimmed })
      delete secretDrafts[name]
      if (name === newSecretName.trim()) {
        newSecretName = ''
        newSecretValue = ''
      }
      await refresh()
      return receiptText(
        receipt,
        getMessage(trimmed === null ? 'mcpSecretDeleted' : 'mcpSecretSaved', name)
      )
    })
  }

  function dismissNotice() {
    noticeDismissed = true
    void storeClientState({ [noticeStorageKey]: true }).catch(() => undefined)
  }

  function startAdding(action: AddAction) {
    if (action === 'import') importOpen = true
    else if (action === 'registry') registryOpen = true
    else openAdd()
  }

  function setOptions(server: McpServerView, options: McpServerOptions) {
    change('options', { op: 'set_options', id: server.id, options }, getMessage('mcpOptionsSaved'))
  }

  /** After an import: show what came in. */
  function imported(receipt: McpReceipt) {
    void run('import', async () => {
      await refresh()
      const ids = receipt.imported || []
      if (ids[0]) {
        selectedId = ''
        select(ids[0])
      }
      return receiptText(receipt, getMessage('mcpImported', ids.join(', ')))
    })
  }

  /** After a Registry install: show it, and sign in when its test asked for that. */
  function installed(id: string, needsAuth: boolean) {
    void run('add', async () => {
      await refresh()
      selectedId = ''
      select(id)
      if (needsAuth) return beginSignIn(id, false)
      return getMessage('mcpAdded', id)
    })
  }

  function openAdd() {
    addFromLink = false
    addMode = 'url'
    addId = ''
    addUrl = ''
    addHeaders = ''
    addCommand = ''
    addEnv = ''
    addJson = ''
    storeAsSecrets = true
    tests = {}
    addOpen = true
  }

  /** The servers the dialog would add, with the secrets that go with them. */
  function buildDraft(): { servers: McpEntry[]; secrets: Record<string, string>; problem: string } {
    let servers: McpEntry[] = []
    let problem = ''
    if (addMode === 'url') {
      if (addUrl.trim()) {
        servers = [remoteEntry(addId.trim() || suggestServerId(addUrl), addUrl, addHeaders)]
      }
    } else if (addMode === 'command') {
      if (addCommand.trim()) {
        servers = [localEntry(addId.trim() || suggestServerId(addCommand), addCommand, addEnv)]
      }
    } else if (addJson.trim()) {
      const parsed = parseMcpConfig(addJson, addId)
      servers = parsed.servers
      if (parsed.error === 'invalid_json') problem = getMessage('mcpJsonInvalid')
      if (parsed.error === 'no_servers') problem = getMessage('mcpJsonNoServers')
      if (parsed.error === 'needs_id') problem = getMessage('mcpJsonNeedsId')
    }
    const taken = new Set(secrets.map((secret) => secret.name))
    const collected: Record<string, string> = {}
    if (storeAsSecrets) {
      servers = servers.map((server) => {
        const moved = moveCredentialsToSecrets(server, taken)
        for (const [name, value] of Object.entries(moved.secrets)) {
          collected[name] = value
          taken.add(name)
        }
        return moved.entry
      })
    }
    const existing = new Set(snapshot.servers.map((server) => server.id))
    const clash = servers.find((server) => existing.has(server.id))
    if (clash && !problem) problem = getMessage('mcpIdTaken', clash.id)
    return { servers, secrets: collected, problem }
  }

  function testDraft() {
    const { servers, secrets: values } = draft
    void run('test', async () => {
      await Promise.all(
        servers.map(async (server) => {
          tests[server.id] = 'testing'
          try {
            tests[server.id] = await mcp.test(server, values)
          } catch (err) {
            tests[server.id] = { status: 'failed', error: errorToMessage(err), tools: [] }
          }
        })
      )
      return ''
    })
  }

  function addDraft() {
    const { servers, secrets: values } = draft
    if (!servers.length || draft.problem) return
    void run('add', async () => {
      for (const [name, value] of Object.entries(values)) {
        await mcp.apply({ op: 'set_secret', name, value })
      }
      for (const server of servers) {
        await mcp.apply({ op: 'add', server, persist: true }, snapshot.revision || undefined)
        snapshot = await mcp.list()
      }
      addOpen = false
      await refresh()
      selectedId = ''
      select(servers[0].id)
      // A server that signs in with OAuth asks for it right away.
      const first = tests[servers[0].id]
      if (servers.length === 1 && first !== 'testing' && first?.status === 'needs_auth') {
        return beginSignIn(servers[0].id, false)
      }
      return getMessage('mcpAdded', servers.map((server) => server.id).join(', '))
    })
  }

  function matchesFilter(server: McpServerView, value: ListFilter): boolean {
    switch (value) {
      case 'attention':
        return needsAttention(server)
      case 'ready':
        return server.status === 'ready'
      case 'disabled':
        return server.status === 'disabled'
      default:
        return true
    }
  }

  function needsAttention(server: McpServerView): boolean {
    return (
      ['invalid', 'needs_auth', 'failed'].includes(server.status) ||
      server.tools.needs_review > 0 ||
      Boolean(server.instructions_changed)
    )
  }

  function statusTone(status: McpStatus): string {
    switch (status) {
      case 'ready':
        return 'bg-emerald-500'
      case 'connecting':
        return 'bg-sky-500 animate-pulse'
      case 'needs_auth':
        return 'bg-amber-500'
      case 'failed':
      case 'invalid':
        return 'bg-destructive'
      default:
        return 'bg-muted-foreground/40'
    }
  }

  function canSignIn(server: McpServerView): boolean {
    return (
      server.transport === 'http' &&
      server.enabled &&
      (server.auth === 'oauth' || server.status === 'needs_auth' || server.status === 'failed')
    )
  }

  function toolApprovalItems(server: McpServerView): { value: ToolApproval; label: string }[] {
    return [
      {
        value: 'inherit',
        label: getMessage('mcpApprovalInherit', approvalLabels[server.approval])
      },
      ...(['auto', 'ask', 'allow'] as McpApproval[]).map((value) => ({
        value,
        label: approvalLabels[value]
      }))
    ]
  }

  /** A tool's own policy, or `inherit` when it follows the server. */
  function toolApproval(server: McpServerView, tool: McpToolView): ToolApproval {
    const own = server.settings.approval
    const tools =
      own && typeof own === 'object' && !Array.isArray(own) ? (own.tools as Json) : undefined
    const value =
      tools && typeof tools === 'object' && !Array.isArray(tools)
        ? tools[tool.remote_name]
        : undefined
    return value === 'auto' || value === 'ask' || value === 'allow' ? value : 'inherit'
  }

  function hintLabels(tool: McpToolView): string[] {
    const hints = tool.annotations
    return [
      hints.read_only === true && getMessage('mcpHintReadOnly'),
      hints.destructive === true && getMessage('mcpHintDestructive'),
      hints.idempotent === true && getMessage('mcpHintIdempotent'),
      hints.open_world === true && getMessage('mcpHintOpenWorld')
    ].filter((label): label is string => Boolean(label))
  }

  const sourceLabels: Record<McpServerView['source'], string> = {
    file: getMessage('mcpSourceFile'),
    manual: getMessage('mcpSourceManual'),
    model: getMessage('mcpSourceModel'),
    import: getMessage('mcpSourceImport'),
    registry: getMessage('mcpSourceRegistry')
  }

  function sourceLabel(server: McpServerView): string {
    return server.persisted
      ? sourceLabels[server.source] || sourceLabels.file
      : getMessage('mcpSourceRuntime')
  }

  function entrySummary(server: McpEntry): string {
    if (typeof server.url === 'string') return server.url
    const args = Array.isArray(server.args) ? server.args.map(String) : []
    return [String(server.command ?? ''), ...args.map(quoteArg)].join(' ')
  }

  function quoteArg(arg: string): string {
    return /\s|^$/.test(arg) ? JSON.stringify(arg) : arg
  }

  /** The header and env names of an entry, and whether each value is kept as a secret. */
  function entryValues(server: McpEntry): { name: string; secret: string }[] {
    const values: { name: string; secret: string }[] = []
    for (const field of ['headers', 'env', 'environment']) {
      const map = server[field]
      if (!map || typeof map !== 'object' || Array.isArray(map)) continue
      for (const [name, value] of Object.entries(map)) {
        values.push({ name, secret: secretReferences(String(value ?? '')).join(', ') })
      }
    }
    return values
  }

  function jsonText(value: Json | undefined): string {
    if (value === undefined || value === null) return ''
    return typeof value === 'string' ? value : JSON.stringify(value, null, 2)
  }

  function timeLabel(ms?: number): string {
    return ms ? new Date(ms).toLocaleString() : ''
  }
</script>

<div class="grid h-full min-h-0 grid-cols-[17rem_minmax(0,1fr)] overflow-hidden">
  <aside class="grid min-h-0 grid-rows-[auto_minmax(0,1fr)_auto] border-r bg-sidebar/70">
    <div class="grid gap-2 border-b p-3">
      <div class="flex items-center gap-2">
        <div class="relative min-w-0 flex-1">
          <Search
            class="pointer-events-none absolute top-1/2 left-2.5 size-3.5 -translate-y-1/2 text-muted-foreground"
          />
          <input
            class={inputClass('h-8 pr-8 pl-8 text-xs')}
            bind:value={search}
            placeholder={getMessage('mcpSearchPlaceholder')}
            aria-label={getMessage('mcpSearchPlaceholder')}
          />
          {#if search}
            <button
              type="button"
              class={buttonClass(
                'ghost',
                'icon-xs',
                'absolute top-1/2 right-1.5 -translate-y-1/2 text-muted-foreground hover:text-foreground'
              )}
              title={getMessage('clearSearch')}
              aria-label={getMessage('clearSearch')}
              onclick={() => (search = '')}
            >
              <X class="size-3" />
            </button>
          {/if}
        </div>
        <button
          type="button"
          class={buttonClass('outline', 'icon-sm')}
          title={getMessage('mcpReload')}
          aria-label={getMessage('mcpReload')}
          disabled={Boolean(busy) || !snapshot.running}
          onclick={reload}
        >
          {#if busy === 'reload'}
            <LoaderCircle class="size-3.5 animate-spin" />
          {:else}
            <RefreshCw class="size-3.5" />
          {/if}
        </button>
        <DropdownMenu
          class={buttonClass('default', 'icon-sm')}
          items={addMenuItems}
          onSelect={startAdding}
          ariaLabel={getMessage('mcpAddServer')}
          title={getMessage('mcpAddServer')}
          disabled={!snapshot.running}
          align="end"
        >
          {#snippet trigger()}<Plus class="size-3.5" />{/snippet}
        </DropdownMenu>
      </div>
      <DropdownMenu
        class="h-8 text-xs"
        items={filterItems}
        bind:value={filter}
        ariaLabel={getMessage('mcpFilter')}
      />
    </div>

    <div class="min-h-0 overflow-y-auto p-2">
      {#if busy === 'load' && !loaded}
        <div class="grid h-28 place-items-center text-muted-foreground">
          <LoaderCircle class="size-5 animate-spin" />
        </div>
      {:else if visibleServers.length === 0}
        <div
          class="grid w-full place-items-center gap-2 rounded-md border border-dashed p-6 text-center text-sm text-muted-foreground"
        >
          <Plug class="size-6" />
          <span>
            {snapshot.servers.length ? getMessage('mcpNoMatch') : getMessage('mcpEmpty')}
          </span>
          {#if !snapshot.servers.length && snapshot.running}
            <div class="grid gap-1.5">
              <button type="button" class={buttonClass('outline', 'sm')} onclick={openAdd}>
                <Plus class="size-3.5" />
                {getMessage('mcpAddServer')}
              </button>
              <button
                type="button"
                class={buttonClass('outline', 'sm')}
                onclick={() => (importOpen = true)}
              >
                <Download class="size-3.5" />
                {getMessage('mcpImportTitle')}
              </button>
              <button
                type="button"
                class={buttonClass('outline', 'sm')}
                onclick={() => (registryOpen = true)}
              >
                <Store class="size-3.5" />
                {getMessage('mcpRegistryBrowse')}
              </button>
            </div>
          {/if}
        </div>
      {:else}
        <div class="grid gap-1.5">
          {#each visibleServers as server (server.id)}
            <button
              type="button"
              class={cn(
                'grid min-w-0 gap-1 rounded-md border px-2.5 py-2 text-left transition',
                selectedId === server.id
                  ? 'border-primary/30 bg-background shadow-xs'
                  : 'border-transparent hover:border-border hover:bg-background/80'
              )}
              onclick={() => select(server.id)}
            >
              <div class="flex min-w-0 items-center gap-2">
                <span
                  class={cn('size-2 shrink-0 rounded-full', statusTone(server.status))}
                  aria-hidden="true"
                ></span>
                <span class="truncate text-sm font-semibold">{server.title || server.id}</span>
                {#if server.tools.needs_review || server.instructions_changed}
                  <span
                    class={badgeClass(
                      'outline',
                      'ml-auto h-4 border-amber-500/50 px-1.5 text-[10px] text-amber-700 dark:text-amber-300'
                    )}
                  >
                    {getMessage(
                      'mcpNeedsReviewCount',
                      String(server.tools.needs_review + (server.instructions_changed ? 1 : 0))
                    )}
                  </span>
                {/if}
              </div>
              <p class="truncate font-mono text-[11px] text-muted-foreground">{server.summary}</p>
              <div class="flex min-w-0 items-center gap-1.5 text-[10px] text-muted-foreground">
                <span class={badgeClass('outline', 'h-4 px-1.5 text-[10px]')}>
                  {server.transport === 'stdio' ? getMessage('mcpLocal') : getMessage('mcpRemote')}
                </span>
                <span class="truncate">{statusLabels[server.status]}</span>
                {#if server.status === 'ready'}
                  <span class="ml-auto shrink-0 tabular-nums">
                    {getMessage('mcpToolCount', String(server.tools.total))}
                  </span>
                {/if}
              </div>
            </button>
          {/each}
        </div>
      {/if}
    </div>

    <div class="border-t p-2">
      <button
        type="button"
        class={cn(
          'flex w-full min-w-0 items-center gap-2 rounded-md border px-2.5 py-2 text-left text-sm transition',
          selectedId === SECRETS
            ? 'border-primary/30 bg-background shadow-xs'
            : 'border-transparent hover:border-border hover:bg-background/80'
        )}
        disabled={!snapshot.running}
        onclick={() => select(SECRETS)}
      >
        <KeyRound class="size-3.5 shrink-0" />
        <span class="truncate font-semibold">{getMessage('mcpSecrets')}</span>
        <span class="ml-auto text-[11px] text-muted-foreground tabular-nums">
          {secrets.filter((secret) => secret.is_set).length}
        </span>
      </button>
    </div>
  </aside>

  <section class="grid min-h-0 min-w-0 grid-rows-[auto_auto_minmax(0,1fr)]">
    <div
      class="flex min-h-14 min-w-0 flex-wrap items-center justify-between gap-3 border-b px-4 py-2"
    >
      <div class="min-w-0">
        <h1 class="truncate text-base font-bold">
          {selectedId === SECRETS
            ? getMessage('mcpSecrets')
            : selected
              ? selected.title || selected.id
              : getMessage('mcpTitle')}
        </h1>
        <p class="truncate text-xs text-muted-foreground">
          {#if selectedId === SECRETS}
            {getMessage('mcpSecretsDetail')}
          {:else if selected}
            {statusLabels[selected.status]} · {selected.id} · {sourceLabel(selected)}
          {:else}
            {getMessage('mcpSubtitle')}
          {/if}
        </p>
      </div>
      {#if selected && selectedId !== SECRETS}
        <div class="flex shrink-0 flex-wrap items-center gap-2">
          {#if selected.enabled && selected.status !== 'invalid'}
            <button
              type="button"
              class={buttonClass('outline', 'sm')}
              disabled={Boolean(busy)}
              onclick={() => reconnect(selected.id)}
            >
              {#if busy === 'reconnect'}
                <LoaderCircle class="size-3.5 animate-spin" />
              {:else}
                <RefreshCw class="size-3.5" />
              {/if}
              {getMessage('mcpReconnect')}
            </button>
          {/if}
          {#if canSignIn(selected)}
            <button
              type="button"
              class={buttonClass(selected.status === 'needs_auth' ? 'default' : 'outline', 'sm')}
              disabled={Boolean(busy)}
              onclick={() => startSignIn(selected.id, selected.status === 'ready')}
            >
              {#if busy === 'signin'}
                <LoaderCircle class="size-3.5 animate-spin" />
              {:else}
                <LogIn class="size-3.5" />
              {/if}
              {selected.status === 'ready' ? getMessage('mcpReauthorize') : getMessage('mcpSignIn')}
            </button>
          {/if}
          {#if selected.persisted}
            <button
              type="button"
              class={buttonClass('outline', 'sm')}
              disabled={Boolean(busy)}
              onclick={() => setEnabled(selected, !selected.enabled)}
            >
              <Power class="size-3.5" />
              {selected.enabled ? getMessage('mcpDisable') : getMessage('mcpEnable')}
            </button>
          {/if}
          <button
            type="button"
            class={buttonClass('destructive', 'sm')}
            disabled={Boolean(busy)}
            onclick={() => (removeOpen = true)}
          >
            <Trash2 class="size-3.5" />
            {getMessage('mcpRemove')}
          </button>
        </div>
      {/if}
    </div>

    <div>
      {#if !noticeDismissed && snapshot.servers.length}
        <div
          class="flex items-start gap-2 border-b bg-sky-50 px-4 py-2 text-sm text-sky-900 dark:bg-sky-950/30 dark:text-sky-100"
        >
          <ShieldCheck class="mt-0.5 size-4 shrink-0" />
          <p class="min-w-0 flex-1">{getMessage('mcpApprovalNotice')}</p>
          <button
            type="button"
            class={buttonClass('ghost', 'icon-xs')}
            title={getMessage('mcpDismiss')}
            aria-label={getMessage('mcpDismiss')}
            onclick={dismissNotice}
          >
            <X class="size-3" />
          </button>
        </div>
      {/if}
      {#if snapshot.config_changed_on_disk}
        <div
          class="flex flex-wrap items-center gap-2 border-b bg-amber-50 px-4 py-2 text-sm text-amber-900 dark:bg-amber-950/30 dark:text-amber-100"
        >
          <AlertTriangle class="size-4 shrink-0" />
          <span class="min-w-0 flex-1">{getMessage('mcpChangedOnDisk')}</span>
          <button
            type="button"
            class={buttonClass('outline', 'xs')}
            disabled={Boolean(busy)}
            onclick={reload}
          >
            {getMessage('mcpApplyFile')}
          </button>
        </div>
      {/if}
      {#each snapshot.diagnostics || [] as diagnostic}
        <div class="border-b bg-destructive/5 px-4 py-2 text-sm text-destructive">{diagnostic}</div>
      {/each}
      {#if signIn}
        <div class="flex flex-wrap items-center gap-2 border-b bg-muted/40 px-4 py-2 text-sm">
          <LoaderCircle class="size-4 shrink-0 animate-spin" />
          <span class="min-w-0 flex-1">{getMessage('mcpSignInWaiting', signIn.id)}</span>
          {#if signIn.url}
            <button
              type="button"
              class={buttonClass('outline', 'xs')}
              onclick={() => signIn && void openExternalUrl(signIn.url)}
            >
              <ExternalLink class="size-3" />
              {getMessage('mcpOpenSignIn')}
            </button>
          {/if}
        </div>
      {/if}
      {#if error}
        <div
          class="border-b bg-destructive/5 px-4 py-2 text-sm whitespace-pre-wrap text-destructive"
          role="alert"
        >
          {error}
        </div>
      {:else if notice}
        <div
          class="border-b bg-emerald-50 px-4 py-2 text-sm text-emerald-800 dark:bg-emerald-950/30 dark:text-emerald-200"
        >
          {notice}
        </div>
      {/if}
    </div>

    <div class="min-h-0 min-w-0">
      {#if loaded && !snapshot.running}
        <div class="grid h-64 place-items-center p-6 text-center text-sm text-muted-foreground">
          {getMessage('mcpNotConnected')}
        </div>
      {:else if selectedId === SECRETS}
        {@render secretsPane()}
      {:else if detailLoading}
        <div class="grid h-48 place-items-center text-muted-foreground">
          <LoaderCircle class="size-6 animate-spin" />
        </div>
      {:else if detail && selected}
        <div class="grid h-full min-h-0 grid-rows-[auto_minmax(0,1fr)]">
          <div class="flex gap-1 border-b px-4 pt-3" role="tablist">
            {#each detailTabs as tab (tab.value)}
              <button
                type="button"
                role="tab"
                aria-selected={activeTab === tab.value}
                class={cn(
                  'flex items-center gap-1.5 rounded-t-md px-3 py-2 text-xs font-semibold',
                  activeTab === tab.value
                    ? 'bg-muted text-foreground'
                    : 'text-muted-foreground hover:bg-muted/60 hover:text-foreground'
                )}
                onclick={() => (activeTab = tab.value)}
              >
                {tab.label}
                {#if tab.value === 'tools' && reviewCount}
                  <span class="size-1.5 rounded-full bg-amber-500" aria-hidden="true"></span>
                {/if}
              </button>
            {/each}
          </div>
          <div class="min-h-0 overflow-y-auto">
            {#if activeTab === 'overview'}
              {@render overview(selected, detail)}
            {:else if activeTab === 'tools'}
              {@render toolsPane(selected, detail)}
            {:else if activeTab === 'events'}
              <McpEventsPanel server={selected} />
            {:else if activeTab === 'access'}
              {@render accessPane(selected)}
            {:else}
              {@render configPane(selected)}
            {/if}
          </div>
        </div>
      {:else if loaded}
        <div class="grid h-64 place-items-center p-6 text-center text-sm text-muted-foreground">
          {snapshot.servers.length ? getMessage('mcpSelectServer') : getMessage('mcpEmptyDetail')}
        </div>
      {/if}
    </div>
  </section>
</div>

{#snippet overview(server: McpServerView, info: McpServerDetail)}
  <div class="grid gap-5 p-4">
    {#if server.last_error || server.diagnostics?.length}
      <div class="grid gap-2">
        {#if server.last_error}
          <div
            class="grid gap-1 rounded-md border border-destructive/30 bg-destructive/5 px-3 py-2 text-sm text-destructive"
          >
            <span class="font-semibold">{getMessage('mcpLastError')}</span>
            <span class="break-words whitespace-pre-wrap">{server.last_error.message}</span>
            {#if server.next_retry_at}
              <span class="text-xs">
                {getMessage('mcpNextRetry', timeLabel(server.next_retry_at))}
              </span>
            {/if}
          </div>
        {/if}
        {#each server.diagnostics || [] as diagnostic}
          <div
            class="rounded-md border border-destructive/30 bg-destructive/5 px-3 py-2 text-sm text-destructive"
          >
            {diagnostic}
          </div>
        {/each}
      </div>
    {/if}

    <div class="grid gap-3 md:grid-cols-2 xl:grid-cols-3">
      <div class="grid gap-1 rounded-md border p-3">
        <div class="text-xs font-semibold text-muted-foreground">{getMessage('mcpStatus')}</div>
        <div class="flex items-center gap-2 text-sm">
          <span class={cn('size-2 rounded-full', statusTone(server.status))}></span>
          {statusLabels[server.status]}
        </div>
        {#if server.last_ready_at}
          <div class="text-[11px] text-muted-foreground">
            {getMessage('mcpLastReady', timeLabel(server.last_ready_at))}
          </div>
        {/if}
      </div>
      <div class="grid gap-1 rounded-md border p-3">
        <div class="text-xs font-semibold text-muted-foreground">
          {server.transport === 'stdio' ? getMessage('mcpCommand') : getMessage('mcpEndpoint')}
        </div>
        <div class="font-mono text-xs break-all">{server.summary}</div>
      </div>
      <div class="grid gap-1 rounded-md border p-3">
        <div class="text-xs font-semibold text-muted-foreground">{getMessage('mcpSource')}</div>
        <div class="text-sm">{sourceLabel(server)}</div>
        {#if server.source_ref}
          <div
            class="truncate font-mono text-[11px] text-muted-foreground"
            title={server.source_ref}
          >
            {server.source_ref}
          </div>
        {/if}
        <div class="text-[11px] text-muted-foreground">
          {server.startup === 'eager'
            ? getMessage('mcpStartupEager')
            : getMessage('mcpStartupBackground')}
        </div>
      </div>
      <div class="grid gap-1 rounded-md border p-3">
        <div class="text-xs font-semibold text-muted-foreground">{getMessage('mcpTabTools')}</div>
        <div class="text-sm">
          {getMessage('mcpToolCount', String(info.tools.filter((tool) => !tool.hidden).length))}
        </div>
        {#if hiddenTools.length}
          <div class="text-[11px] text-muted-foreground">
            {getMessage('mcpHiddenCount', String(hiddenTools.length))}
          </div>
        {/if}
      </div>
      <div class="grid gap-1 rounded-md border p-3">
        <div class="text-xs font-semibold text-muted-foreground">{getMessage('mcpUsage')}</div>
        <div class="text-sm">
          {server.usage?.calls
            ? getMessage('mcpUsageCalls', [
                String(server.usage.calls),
                String(server.usage.errors ?? 0)
              ])
            : getMessage('mcpUsageNone')}
        </div>
        {#if server.usage?.last_used_at}
          <div class="text-[11px] text-muted-foreground">
            {getMessage('mcpLastUsed', timeLabel(server.usage.last_used_at))}
          </div>
        {/if}
      </div>
      <div class="grid gap-1 rounded-md border p-3">
        <div class="text-xs font-semibold text-muted-foreground">
          {getMessage('mcpDefaultApproval')}
        </div>
        <div class="text-sm">{approvalLabels[server.approval]}</div>
      </div>
    </div>

    {#if server.description}
      <div class="grid gap-1">
        <div class="text-xs font-semibold text-muted-foreground">
          {getMessage('mcpServerDescription')}
        </div>
        <p class="text-sm">{server.description}</p>
      </div>
    {/if}

    {#if info.instructions || info.instructions_changed}
      <div class="grid gap-2">
        <div class="flex flex-wrap items-center gap-2">
          <span class="text-xs font-semibold text-muted-foreground">
            {getMessage('mcpInstructions')}
          </span>
          <span class={badgeClass('outline', 'h-4 px-1.5 text-[10px]')}>
            {getMessage('mcpFromServer')}
          </span>
          {#if info.instructions_changed}
            <span
              class={badgeClass(
                'outline',
                'h-4 border-amber-500/50 px-1.5 text-[10px] text-amber-700 dark:text-amber-300'
              )}
            >
              {getMessage('mcpReviewChanged')}
            </span>
            <button
              type="button"
              class={buttonClass('outline', 'xs', 'ml-auto')}
              disabled={Boolean(busy)}
              onclick={() => markReviewed()}
            >
              <Check class="size-3" />
              {getMessage('mcpAcceptAll')}
            </button>
          {/if}
        </div>
        {#if info.instructions_changed}
          <div class="grid gap-2 lg:grid-cols-2">
            <div class="grid gap-1">
              <span class="text-[11px] text-muted-foreground"
                >{getMessage('mcpReviewedVersion')}</span
              >
              <pre
                class="max-h-64 overflow-auto rounded-md border bg-muted/30 p-3 text-xs whitespace-pre-wrap">{info.reviewed_instructions ||
                  getMessage('mcpNone')}</pre>
            </div>
            <div class="grid gap-1">
              <span class="text-[11px] text-muted-foreground"
                >{getMessage('mcpCurrentVersion')}</span
              >
              <pre
                class="max-h-64 overflow-auto rounded-md border border-amber-500/40 bg-amber-50/50 p-3 text-xs whitespace-pre-wrap dark:bg-amber-950/20">{info.instructions ||
                  getMessage('mcpNone')}</pre>
            </div>
          </div>
        {:else}
          <details class="rounded-md border bg-muted/20 px-3 py-2">
            <summary class="cursor-pointer text-xs text-muted-foreground">
              {getMessage('mcpShowInstructions')}
            </summary>
            <pre
              class="mt-2 max-h-80 overflow-auto text-xs whitespace-pre-wrap">{info.instructions}</pre>
          </details>
        {/if}
      </div>
    {/if}
  </div>
{/snippet}

{#snippet toolsPane(server: McpServerView, info: McpServerDetail)}
  <div class="grid gap-4 p-4">
    <div class="grid gap-2">
      <div class="flex flex-wrap items-end gap-3">
        <label class="grid min-w-56 gap-1 text-xs font-medium">
          {getMessage('mcpDefaultApproval')}
          <DropdownMenu
            class="h-8 text-xs"
            items={serverApprovalItems}
            value={server.approval}
            disabled={Boolean(busy) || !server.persisted}
            ariaLabel={getMessage('mcpDefaultApproval')}
            onSelect={(value) =>
              change(
                'approval',
                {
                  op: 'set_approval',
                  id: server.id,
                  approval: value === 'auto' ? null : value
                },
                getMessage('mcpPolicySaved')
              )}
          />
        </label>
        {#if reviewCount}
          <button
            type="button"
            class={buttonClass('outline', 'sm', 'ml-auto')}
            disabled={Boolean(busy)}
            onclick={() => markReviewed()}
          >
            <Check class="size-3.5" />
            {getMessage('mcpAcceptAll')}
          </button>
        {/if}
      </div>
      <p class="text-xs text-muted-foreground">{getMessage('mcpApprovalHelp')}</p>
    </div>

    {#if server.status !== 'ready' && !visibleTools.length}
      <div class="rounded-md border border-dashed p-6 text-center text-sm text-muted-foreground">
        {getMessage('mcpToolsWhenConnected')}
      </div>
    {:else if !visibleTools.length}
      <div class="rounded-md border border-dashed p-6 text-center text-sm text-muted-foreground">
        {getMessage('mcpNoTools')}
      </div>
    {:else}
      <div class="grid gap-2">
        {#each visibleTools as tool (tool.remote_name)}
          {@const diff = diffs[tool.remote_name]}
          <div
            class={cn(
              'grid gap-2 rounded-md border p-3',
              tool.review && tool.review !== 'trusted' && 'border-amber-500/40'
            )}
          >
            <div class="flex min-w-0 flex-wrap items-start gap-3">
              <div class="grid min-w-0 flex-1 gap-1">
                <div class="flex min-w-0 flex-wrap items-center gap-1.5">
                  <span class="truncate font-mono text-sm font-semibold">{tool.remote_name}</span>
                  {#if tool.title && tool.title !== tool.remote_name}
                    <span class="truncate text-xs text-muted-foreground">{tool.title}</span>
                  {/if}
                  {#each hintLabels(tool) as hint}
                    <span class={badgeClass('secondary', 'h-4 px-1.5 text-[10px]')}>{hint}</span>
                  {/each}
                  {#if tool.review === 'new' || tool.review === 'changed'}
                    <span
                      class={badgeClass(
                        'outline',
                        'h-4 border-amber-500/50 px-1.5 text-[10px] text-amber-700 dark:text-amber-300'
                      )}
                    >
                      {tool.review === 'new'
                        ? getMessage('mcpReviewNew')
                        : getMessage('mcpReviewChanged')}
                    </span>
                  {/if}
                </div>
                {#if tool.description}
                  <p class="line-clamp-2 text-xs text-muted-foreground" title={tool.description}>
                    {tool.description}
                  </p>
                {/if}
              </div>
              <div class="flex shrink-0 items-center gap-1.5">
                <DropdownMenu
                  class="h-7 w-44 text-xs"
                  items={toolApprovalItems(server)}
                  value={toolApproval(server, tool)}
                  disabled={Boolean(busy) || !server.persisted}
                  ariaLabel={getMessage('mcpToolApproval', tool.remote_name)}
                  onSelect={(value) => setToolApproval(tool, value)}
                />
                <button
                  type="button"
                  class={buttonClass('ghost', 'icon-sm')}
                  title={getMessage('mcpHideTool')}
                  aria-label={getMessage('mcpHideTool')}
                  disabled={Boolean(busy) || !server.persisted}
                  onclick={() => setToolVisible(tool.remote_name, false)}
                >
                  <EyeOff class="size-3.5" />
                </button>
              </div>
            </div>
            {#if tool.review === 'new' || tool.review === 'changed'}
              <div class="flex flex-wrap items-center gap-2">
                <button
                  type="button"
                  class={buttonClass('outline', 'xs')}
                  onclick={() => void toggleDiff(tool.remote_name)}
                >
                  {diff ? getMessage('mcpHideChanges') : getMessage('mcpShowChanges')}
                </button>
                <button
                  type="button"
                  class={buttonClass('outline', 'xs')}
                  disabled={Boolean(busy)}
                  onclick={() => markReviewed([tool.remote_name])}
                >
                  <Check class="size-3" />
                  {getMessage('mcpAccept')}
                </button>
              </div>
              {#if diff === 'loading'}
                <LoaderCircle class="size-4 animate-spin text-muted-foreground" />
              {:else if diff}
                <div class="grid gap-2">
                  {#each diff.changes as fieldChange (fieldChange.field)}
                    <div class="grid gap-1">
                      <span class="font-mono text-[11px] font-semibold">{fieldChange.field}</span>
                      <div class="grid gap-2 lg:grid-cols-2">
                        <pre
                          class="max-h-48 overflow-auto rounded-md border bg-muted/30 p-2 text-[11px] whitespace-pre-wrap">{jsonText(
                            fieldChange.before
                          ) || getMessage('mcpNone')}</pre>
                        <pre
                          class="max-h-48 overflow-auto rounded-md border border-amber-500/40 bg-amber-50/50 p-2 text-[11px] whitespace-pre-wrap dark:bg-amber-950/20">{jsonText(
                            fieldChange.after
                          ) || getMessage('mcpNone')}</pre>
                      </div>
                    </div>
                  {/each}
                </div>
              {/if}
            {/if}
          </div>
        {/each}
      </div>
    {/if}

    {#if hiddenTools.length}
      <div class="grid gap-2">
        <div class="text-xs font-semibold text-muted-foreground">
          {getMessage('mcpHiddenTools')}
        </div>
        <div class="flex flex-wrap gap-2">
          {#each hiddenTools as tool (tool.remote_name)}
            <span class="flex items-center gap-1 rounded-md border px-2 py-1 font-mono text-xs">
              {tool.remote_name}
              <button
                type="button"
                class={buttonClass('ghost', 'icon-xs')}
                title={getMessage('mcpShowTool')}
                aria-label={getMessage('mcpShowTool')}
                disabled={Boolean(busy) || !server.persisted}
                onclick={() => setToolVisible(tool.remote_name, true)}
              >
                <Eye class="size-3" />
              </button>
            </span>
          {/each}
        </div>
      </div>
    {/if}
    {#if !server.persisted}
      <p class="text-xs text-muted-foreground">{getMessage('mcpRuntimeReadOnly')}</p>
    {/if}
  </div>
{/snippet}

{#snippet accessPane(server: McpServerView)}
  <div class="grid max-w-3xl gap-5 p-4">
    {#if server.transport === 'http'}
      <div class="grid gap-2">
        <div class="text-xs font-semibold text-muted-foreground">
          {getMessage('mcpSignInTitle')}
        </div>
        <div class="flex flex-wrap items-center gap-2 rounded-md border p-3 text-sm">
          <span class="min-w-0 flex-1">
            {server.auth === 'oauth'
              ? server.status === 'needs_auth'
                ? getMessage('mcpOAuthNeeded')
                : getMessage('mcpOAuthSignedIn')
              : server.auth === 'none'
                ? getMessage('mcpAuthNone')
                : getMessage('mcpAuthHeaders')}
          </span>
          {#if canSignIn(server)}
            <button
              type="button"
              class={buttonClass('outline', 'sm')}
              disabled={Boolean(busy)}
              onclick={() => startSignIn(server.id, server.status === 'ready')}
            >
              <LogIn class="size-3.5" />
              {server.status === 'ready' ? getMessage('mcpReauthorize') : getMessage('mcpSignIn')}
            </button>
          {/if}
          {#if server.auth === 'oauth'}
            <button
              type="button"
              class={buttonClass('outline', 'sm')}
              disabled={Boolean(busy)}
              onclick={() => signOut(server.id)}
            >
              <LogOut class="size-3.5" />
              {getMessage('mcpSignOut')}
            </button>
          {/if}
        </div>
      </div>
    {/if}

    <div class="grid gap-2">
      <div class="text-xs font-semibold text-muted-foreground">
        {getMessage('mcpServerSecrets')}
      </div>
      {#if serverSecrets.length}
        <div class="grid gap-2">
          {#each serverSecrets as secret (secret.name)}
            {@render secretRow(secret)}
          {/each}
        </div>
      {:else}
        <p class="text-sm text-muted-foreground">{getMessage('mcpNoServerSecrets')}</p>
      {/if}
    </div>

    <label class="flex items-start gap-3 rounded-md border p-3">
      <input
        type="checkbox"
        class="mt-0.5"
        checked={server.allow_external_users}
        disabled={Boolean(busy) || !server.persisted}
        onchange={(event) =>
          change(
            'external',
            {
              op: 'set_external_users',
              id: server.id,
              allowed: (event.currentTarget as HTMLInputElement).checked
            },
            getMessage('mcpPolicySaved')
          )}
      />
      <span class="grid gap-1">
        <span class="text-sm font-semibold">{getMessage('mcpExternalUsers')}</span>
        <span class="text-xs text-muted-foreground">{getMessage('mcpExternalUsersHelp')}</span>
      </span>
    </label>
  </div>
{/snippet}

{#snippet configPane(server: McpServerView)}
  <div class="grid gap-3 p-4">
    {#if server.options}
      <McpOptionsForm
        {server}
        busy={Boolean(busy)}
        saving={busy === 'options'}
        onSave={(options) => setOptions(server, options)}
      />
    {:else}
      <p class="text-xs text-muted-foreground">{getMessage('mcpOptionsUnavailable')}</p>
    {/if}
    <p class="text-xs text-muted-foreground">
      {getMessage('mcpConfigHelp', snapshot.config_path || 'mcp.json')}
    </p>
    <pre
      class="max-h-[32rem] overflow-auto rounded-md border bg-muted/30 p-3 font-mono text-xs whitespace-pre-wrap">{JSON.stringify(
        server.settings,
        null,
        2
      )}</pre>
  </div>
{/snippet}

{#snippet secretRow(secret: McpSecretView)}
  <div class="grid gap-2 rounded-md border p-3">
    <div class="flex min-w-0 flex-wrap items-center gap-2">
      <span class="font-mono text-sm font-semibold">{secret.name}</span>
      <span
        class={badgeClass(
          secret.is_set ? 'secondary' : 'outline',
          cn(
            'h-4 px-1.5 text-[10px]',
            !secret.is_set && 'border-amber-500/50 text-amber-700 dark:text-amber-300'
          )
        )}
      >
        {secret.is_set
          ? getMessage('mcpSecretSet', timeLabel(secret.updated_at))
          : getMessage('mcpSecretNotSet')}
      </span>
      <span class="min-w-0 flex-1 truncate text-right text-[11px] text-muted-foreground">
        {secret.used_by.length
          ? getMessage('mcpSecretUsedBy', secret.used_by.join(', '))
          : getMessage('mcpSecretUnused')}
      </span>
    </div>
    <div class="flex items-center gap-2">
      <input
        class={inputClass('h-8 text-xs')}
        type="password"
        autocomplete="off"
        placeholder={secret.is_set ? getMessage('mcpSecretReplace') : getMessage('mcpSecretValue')}
        aria-label={getMessage('mcpSecretValueFor', secret.name)}
        bind:value={secretDrafts[secret.name]}
      />
      <button
        type="button"
        class={buttonClass('default', 'sm')}
        disabled={Boolean(busy) || !secretDrafts[secret.name]?.trim()}
        onclick={() => saveSecret(secret.name, secretDrafts[secret.name] ?? '')}
      >
        {getMessage('save')}
      </button>
      {#if secret.is_set}
        <button
          type="button"
          class={buttonClass('ghost', 'icon-sm')}
          title={getMessage('mcpSecretDelete')}
          aria-label={getMessage('mcpSecretDelete')}
          disabled={Boolean(busy)}
          onclick={() => saveSecret(secret.name, null)}
        >
          <Trash2 class="size-3.5" />
        </button>
      {/if}
    </div>
  </div>
{/snippet}

{#snippet secretsPane()}
  <div class="grid max-w-3xl gap-4 overflow-y-auto p-4">
    <p class="text-sm text-muted-foreground">{getMessage('mcpSecretsHelp')}</p>
    {#each secrets as secret (secret.name)}
      {@render secretRow(secret)}
    {:else}
      <p class="rounded-md border border-dashed p-6 text-center text-sm text-muted-foreground">
        {getMessage('mcpNoSecrets')}
      </p>
    {/each}
    <div class="grid gap-2 rounded-md border bg-muted/20 p-3">
      <div class="text-xs font-semibold text-muted-foreground">{getMessage('mcpNewSecret')}</div>
      <div class="flex flex-wrap items-center gap-2">
        <input
          class={inputClass('h-8 w-48 font-mono text-xs')}
          placeholder="GITHUB_PAT"
          aria-label={getMessage('mcpSecretName')}
          bind:value={newSecretName}
        />
        <input
          class={inputClass('h-8 min-w-48 flex-1 text-xs')}
          type="password"
          autocomplete="off"
          placeholder={getMessage('mcpSecretValue')}
          aria-label={getMessage('mcpSecretValue')}
          bind:value={newSecretValue}
        />
        <button
          type="button"
          class={buttonClass('default', 'sm')}
          disabled={Boolean(busy) ||
            !/^[A-Za-z_][A-Za-z0-9_]*$/.test(newSecretName.trim()) ||
            !newSecretValue.trim()}
          onclick={() => saveSecret(newSecretName.trim(), newSecretValue)}
        >
          {getMessage('save')}
        </button>
      </div>
      <p class="text-[11px] text-muted-foreground">{getMessage('mcpSecretReferenceHelp')}</p>
    </div>
  </div>
{/snippet}

{#snippet removeActions()}
  <button type="button" class={buttonClass('outline', 'sm')} onclick={() => (removeOpen = false)}>
    {getMessage('cancel')}
  </button>
  <button type="button" class={buttonClass('destructive', 'sm')} onclick={remove}>
    {getMessage('mcpRemove')}
  </button>
{/snippet}

<Modal
  alert
  bind:open={removeOpen}
  title={getMessage('mcpRemoveTitle', selected?.id || '')}
  contentClass="min-h-0 sm:max-w-sm"
  footer={removeActions}
>
  <p class="text-sm leading-relaxed text-muted-foreground">{getMessage('mcpRemoveConfirm')}</p>
</Modal>

{#snippet addActions()}
  <button type="button" class={buttonClass('outline', 'sm')} onclick={() => (addOpen = false)}>
    {getMessage('cancel')}
  </button>
  <button
    type="button"
    class={buttonClass('outline', 'sm')}
    disabled={Boolean(busy) || !draft.servers.length || Boolean(draft.problem)}
    onclick={testDraft}
  >
    {#if busy === 'test'}
      <LoaderCircle class="size-3.5 animate-spin" />
    {/if}
    {getMessage('mcpTestConnection')}
  </button>
  <button
    type="button"
    class={buttonClass('default', 'sm')}
    disabled={Boolean(busy) || !draft.servers.length || Boolean(draft.problem)}
    onclick={addDraft}
  >
    {#if busy === 'add'}
      <LoaderCircle class="size-3.5 animate-spin" />
    {/if}
    {getMessage('mcpAdd')}
  </button>
{/snippet}

<Modal
  bind:open={addOpen}
  title={getMessage('mcpAddServer')}
  description={getMessage('mcpAddDescription')}
  contentClass="sm:max-w-2xl"
  footer={addActions}
>
  <div class="grid gap-3">
    {#if addFromLink}
      <div
        class="flex items-start gap-2 rounded-md border border-amber-500/40 bg-amber-50 px-3 py-2 text-xs text-amber-900 dark:bg-amber-950/30 dark:text-amber-100"
        role="alert"
      >
        <AlertTriangle class="mt-0.5 size-4 shrink-0" />
        <p>{getMessage('mcpFromLink')}</p>
      </div>
    {/if}
    <div class="flex gap-1 rounded-md bg-muted p-1" role="tablist">
      {#each addModes as mode (mode.value)}
        <button
          type="button"
          role="tab"
          aria-selected={addMode === mode.value}
          class={cn(
            'flex-1 rounded px-2 py-1 text-xs font-semibold transition',
            addMode === mode.value
              ? 'bg-background text-foreground shadow-xs'
              : 'text-muted-foreground hover:text-foreground'
          )}
          onclick={() => {
            addMode = mode.value
            tests = {}
          }}
        >
          {mode.label}
        </button>
      {/each}
    </div>

    {#if addMode === 'url'}
      <label class="grid gap-1 text-xs font-medium">
        {getMessage('mcpEndpoint')}
        <input
          class={inputClass('h-8 font-mono text-xs')}
          placeholder="https://example.com/mcp"
          bind:value={addUrl}
        />
      </label>
      <label class="grid gap-1 text-xs font-medium">
        {getMessage('mcpHeaders')}
        <textarea
          class={textareaClass('min-h-16 font-mono text-xs')}
          placeholder="Authorization: Bearer …"
          bind:value={addHeaders}></textarea>
      </label>
    {:else if addMode === 'command'}
      <label class="grid gap-1 text-xs font-medium">
        {getMessage('mcpCommand')}
        <input
          class={inputClass('h-8 font-mono text-xs')}
          placeholder="npx -y @upstash/context7-mcp"
          bind:value={addCommand}
        />
      </label>
      <label class="grid gap-1 text-xs font-medium">
        {getMessage('mcpEnvironment')}
        <textarea
          class={textareaClass('min-h-16 font-mono text-xs')}
          placeholder="API_KEY=…"
          bind:value={addEnv}></textarea>
      </label>
    {:else}
      <label class="grid gap-1 text-xs font-medium">
        {getMessage('mcpPasteJson')}
        <textarea
          class={textareaClass('min-h-36 font-mono text-xs')}
          placeholder={'{ "mcpServers": { "docs": { "url": "https://docs.example.com/mcp" } } }'}
          bind:value={addJson}></textarea>
      </label>
    {/if}

    <label class="grid gap-1 text-xs font-medium">
      {addMode === 'json' ? getMessage('mcpIdForEntry') : getMessage('mcpServerId')}
      <input
        class={inputClass('h-8 font-mono text-xs')}
        placeholder={addMode === 'url'
          ? suggestServerId(addUrl)
          : addMode === 'command'
            ? suggestServerId(addCommand)
            : 'docs'}
        bind:value={addId}
      />
    </label>

    <label class="flex items-start gap-2 text-xs">
      <input type="checkbox" class="mt-0.5" bind:checked={storeAsSecrets} />
      <span class="grid gap-0.5">
        <span class="font-medium">{getMessage('mcpStoreAsSecrets')}</span>
        <span class="text-muted-foreground">{getMessage('mcpStoreAsSecretsHelp')}</span>
      </span>
    </label>

    {#if draft.problem}
      <p class="text-sm text-destructive">{draft.problem}</p>
    {/if}

    {#each draft.servers as server (server.id)}
      {@const test = tests[server.id]}
      <div class="grid gap-2 rounded-md border p-3">
        <div class="flex min-w-0 items-center gap-2">
          <span class="font-mono text-sm font-semibold">{server.id}</span>
          <span class={badgeClass('outline', 'h-4 px-1.5 text-[10px]')}>
            {typeof server.url === 'string' ? getMessage('mcpRemote') : getMessage('mcpLocal')}
          </span>
          {#if test === 'testing'}
            <LoaderCircle class="ml-auto size-3.5 animate-spin text-muted-foreground" />
          {:else if test}
            <span
              class={cn(
                'ml-auto text-xs font-semibold',
                test.status === 'ready'
                  ? 'text-emerald-700 dark:text-emerald-300'
                  : test.status === 'needs_auth'
                    ? 'text-amber-700 dark:text-amber-300'
                    : 'text-destructive'
              )}
            >
              {test.status === 'ready'
                ? getMessage('mcpTestReady', String(test.tools.length))
                : test.status === 'needs_auth'
                  ? getMessage('mcpTestNeedsAuth')
                  : getMessage('mcpTestFailed')}
            </span>
          {/if}
        </div>
        {#if typeof server.url !== 'string'}
          <p class="text-[11px] text-muted-foreground">{getMessage('mcpLocalWarning')}</p>
        {/if}
        <code class="rounded bg-muted/50 px-2 py-1 font-mono text-xs break-all"
          >{entrySummary(server)}</code
        >
        {#each entryValues(server) as value (value.name)}
          <div class="flex items-center gap-2 text-[11px] text-muted-foreground">
            <span class="font-mono">{value.name}</span>
            <span>→</span>
            <span>
              {value.secret
                ? getMessage('mcpValueSecret', value.secret)
                : getMessage('mcpValuePlain')}
            </span>
          </div>
        {/each}
        {#if test && test !== 'testing' && test.error}
          <p class="text-xs break-words whitespace-pre-wrap text-destructive">{test.error}</p>
        {/if}
        {#if test && test !== 'testing' && test.tools.length}
          <p class="truncate font-mono text-[11px] text-muted-foreground">
            {test.tools.map((tool) => tool.remote_name).join(', ')}
          </p>
        {/if}
      </div>
    {/each}
  </div>
</Modal>

<McpImportDialog bind:open={importOpen} revision={snapshot.revision} onImported={imported} />
<McpRegistryDialog
  bind:open={registryOpen}
  revision={snapshot.revision}
  takenIds={new Set(snapshot.servers.map((server) => server.id))}
  secretNames={new Set(secrets.map((secret) => secret.name))}
  onInstalled={installed}
/>
