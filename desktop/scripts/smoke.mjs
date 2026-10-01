import { _electron as electron } from 'playwright'
import electronPath from 'electron'
import { createServer } from 'node:http'
import { WebSocketServer } from 'ws'
import { mkdtemp, mkdir, writeFile } from 'node:fs/promises'
import { execFile } from 'node:child_process'
import { promisify } from 'node:util'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import assert from 'node:assert/strict'
import { parseDocument } from 'yaml'
import { testTone } from '../src/renderer/audio-test.ts'

const directory = await mkdtemp(join(tmpdir(), 'anda-desktop-smoke-'))
const screenshotDir = resolve('test-results')
await mkdir(screenshotDir, { recursive: true })
const usage = {
  input_tokens: 10,
  output_tokens: 20,
  cached_tokens: 0,
  requests: 1
}
const assistantReply =
  'Hello from the local daemon.\n\nYour desktop connection is working. **No external model was called.**'
const sources = {}
const conversations = new Map()
let next = 1
let approvals = 0
let lastWorkspace = null
const project = join(directory, 'smoke-project')
await mkdir(project)
await promisify(execFile)('git', ['init', '-b', 'main'], { cwd: project })
await writeFile(join(project, 'smoke.txt'), 'A local Git fixture\n')
// The daemon lists a chat started from the `anda` terminal only by its
// `cli:<folder>` source; the desktop keeps no workspace of its own for it.
const terminalProject = join(directory, 'terminal-project')
await mkdir(terminalProject)
await promisify(execFile)('git', ['init', '-b', 'main'], { cwd: terminalProject })
await writeFile(join(terminalProject, 'terminal.txt'), 'Started from the anda terminal\n')
const browserConnections = new Map()
const submissionReceipts = new Map()
let recoveryExecutions = 0
let reloadExecutions = 0
let finishReloadReply
let automationReads = 0
let updatedAutomation
const automation = {
  _id: 1,
  name: 'Long automation',
  job: 'Keep the complete scheduled prompt. '.repeat(30),
  job_kind: 'agent',
  schedule_kind: 'every',
  schedule: '1d'
}
const browserWaiting = new Map()
let browserRequestId = 100000
function browserAction(source, args) {
  const target = browserConnections.get(source)
  assert.ok(target, 'Desktop browser must be registered')
  const id = ++browserRequestId
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => {
      browserWaiting.delete(id)
      reject(new Error(`Browser action timed out: ${args.action}`))
    }, 12000)
    browserWaiting.set(id, (result) => {
      clearTimeout(timer)
      result.ok ? resolve(result.value) : reject(new Error(result.error))
    })
    target.ws.send(
      JSON.stringify({
        id,
        method: 'browser_action',
        params: { session: source, request_id: id, args }
      })
    )
  })
}
let configContent =
  'addr: 127.0.0.1:8042\nmodel:\n  active: legacy-model\n  providers:\n    - family: openai\n      model: legacy-model\n      api_base: https://example.invalid/v1\n      api_key: test-key\n'
let chatgptConnected = true
let chatgptSelected = false
const server = createServer((req, res) => {
  if (req.url === '/daemon/chatgpt') {
    assert.equal(req.headers.authorization, 'Bearer desktop-test-token')
    let body = ''
    req.on('data', (chunk) => {
      body += chunk
    })
    req.on('end', () => {
      const request = JSON.parse(body)
      let result = {}
      if (request.method === 'accounts')
        result = {
          active: 'test-account',
          accounts: [
            {
              id: 'test-account',
              label: 'Local test account',
              connected: chatgptConnected,
              plan_enabled: chatgptConnected
            }
          ],
          needs_setup: false
        }
      if (request.method === 'models')
        result = { models: [{ slug: 'test-plan-model', display_name: 'Test plan model' }] }
      if (request.method === 'model_select') {
        assert.equal(request.params.profile_id, 'test-account')
        chatgptSelected = true
        const config = parseDocument(configContent)
        config.setIn(['model', 'active'], 'chatgpt:test-account:test-plan-model')
        configContent = config.toString()
        result = { active_model: 'chatgpt:test-account:test-plan-model' }
      }
      if (request.method === 'logout') {
        chatgptConnected = false
        result = { revocation_confirmed: true }
      }
      res.setHeader('Content-Type', 'application/json')
      res.end(JSON.stringify(result))
    })
    return
  }
  if (req.url === '/browser-fixture') {
    res.setHeader('Content-Type', 'text/html')
    res.end(
      '<!doctype html><title>Browser fixture</title><h1>Native browser fixture</h1><input id="name" aria-label="Name"><button id="go">Apply</button><output></output><script>document.querySelector("#go").onclick = () => { document.querySelector("output").textContent = document.querySelector("input").value }</script>'
    )
    return
  }
  res.setHeader('Content-Type', 'application/json')
  res.end(
    JSON.stringify({
      path: '/isolated/config.yaml',
      content: configContent,
      config: { addr: '127.0.0.1:8042' },
      revision: 'test-revision'
    })
  )
})
const wsServer = new WebSocketServer({ server })
wsServer.on('connection', (ws, request) => {
  assert.equal(request.headers.authorization, 'Bearer desktop-test-token')
  assert.ok(!request.url.includes('token'))
  ws.on('message', (raw) => {
    const incoming = JSON.parse(raw.toString())
    if (!incoming.method) {
      browserWaiting.get(incoming.id)?.(incoming.result)
      browserWaiting.delete(incoming.id)
      return
    }
    const id = incoming.id
    let { method, params } = incoming
    const submissionId = method === 'chat/submit' ? params.requestId : null
    if (submissionId) {
      method = 'agent_run'
      params = [params.input]
    }
    if (method === 'browser_register') browserConnections.set(params[0].session, { ws })
    const input = params?.[0] || {}
    if (submissionId && ['/side Reload check', '/side Direct check'].includes(input.prompt)) {
      const reloading = input.prompt === '/side Reload check'
      const reply = () => {
        const receipt = {
          state: 'completed',
          source: input.meta.source,
          requestId: submissionId,
          result: {
            chat_history: [
              {
                role: 'assistant',
                content: [
                  {
                    type: 'Text',
                    text: reloading ? 'Side reply after renderer reload' : 'Direct side reply'
                  }
                ]
              }
            ]
          }
        }
        submissionReceipts.set(submissionId, receipt)
        ws.send(JSON.stringify({ id, result: receipt }))
      }
      if (reloading) {
        reloadExecutions++
        finishReloadReply = reply
      } else reply()
      return
    }
    let result =
      method === 'initialize'
        ? {
            protocolVersion: 1,
            instanceId: 'smoke',
            capabilities: { stateInvalidation: true, submissionReceipts: true }
          }
        : {}
    if (method === 'model_names')
      result = {
        active_model: chatgptSelected ? 'chatgpt:test-account:test-plan-model' : 'Local test model',
        model_names: ['Local test model']
      }
    else if (method === 'capabilities')
      result = {
        transcription: ['webm', 'wav'],
        tts: ['wav'],
        desktop: {
          protocol: 1,
          app_transport: true,
          workspace_sources: true,
          config_revision: true
        }
      }
    else if (method === 'memory_overview')
      result = {
        result: {
          schema_version: 1,
          observed_at: Date.now(),
          memory: { state: 'reachable' },
          inbox: { state: 'not_configured' },
          capabilities: {}
        }
      }
    else if (method.startsWith('memory_'))
      result = {
        result: {
          schema_version: 1,
          items: [],
          complete: true,
          next_cursor: null
        }
      }
    else if (method === 'agent_run') {
      const source = input.meta.source
      lastWorkspace = input.meta.workspace || null
      let cid = sources[source]?.c || next++
      const c = conversations.get(cid) || {
        _id: cid,
        user: 'test-owner',
        status: 'idle',
        usage,
        messages: [],
        artifacts: [],
        created_at: Date.now(),
        updated_at: Date.now()
      }
      const prompt = input.prompt.replace(/^\/new\s+/, '')
      c.messages.push({
        role: 'user',
        content: [{ type: 'Text', text: prompt }],
        timestamp: Date.now()
      })
      c.messages.push({
        role: 'assistant',
        content: [{ type: 'Text', text: assistantReply }],
        timestamp: Date.now() + 1
      })
      if (prompt === 'Approval check')
        c.messages.push({
          role: 'assistant',
          name: '$action',
          content: [
            {
              type: 'Action',
              name: 'anda.tool_approval',
              payload: {
                id: 'approval-test',
                kind: 'tool_approval',
                title: 'Review this local action',
                status: 'pending',
                tool: { name: 'shell', label: 'Shell' },
                details: [{ label: 'Command', value: 'echo safe', format: 'code' }],
                approval: { approve_label: 'Approve', deny_label: 'Deny' },
                created_at: Date.now(),
                expires_at: Date.now() + 60_000
              }
            }
          ],
          timestamp: Date.now() + 2
        })
      conversations.set(cid, c)
      sources[source] = { c: cid, s: 'idle', t: Date.now() }
      result = { conversation: cid, content: '', usage }
    } else if (method === 'tool_call') {
      if (input.name === 'actions_api') {
        approvals++
        const cid = input.meta.conversation
        const c = conversations.get(cid)
        const action = c.messages.flatMap((m) => m.content).find((part) => part.type === 'Action')
        action.payload.status = 'approved'
        const output = {
          action_id: input.args.action_id,
          conversation: cid,
          status: 'approved',
          response: true,
          responded_at: Date.now()
        }
        ws.send(JSON.stringify({ id, result: { output, usage } }))
        return
      }
      let value = []
      if (input.name === 'list_cron_jobs') {
        value = [{ ...automation, job: automation.job.slice(0, 512) + '…' }]
      } else if (input.name === 'manage_cron_job' && input.args.action === 'get') {
        automationReads++
        value = { action: 'get', job: automation }
      } else if (input.name === 'update_cron_job') {
        updatedAutomation = input.args
        value = { ...automation, ...input.args }
      } else if (input.name === 'conversations_api') {
        const args = input.args
        if (args.type === 'ListSourceState') value = sources
        else if (args.type === 'GetSourceState') value = sources[input.meta?.source] || { c: 0 }
        else if (args.type === 'GetConversation') value = conversations.get(args._id)
        else if (args.type === 'GetConversationDelta') {
          const c = conversations.get(args._id)
          value = {
            ...c,
            messages: c.messages.slice(args.messages_offset),
            artifacts: []
          }
        } else if (args.type === 'SearchConversations') value = Array.from(conversations.values())
      } else if (input.name === 'bookmarks_api' && input.args.type === 'GetConversationBookmark')
        value = null
      result = { output: { result: value }, usage }
      if (input.name === 'transcribe_audio') {
        assert.ok(input.args.audio_base64.length > 100)
        result = { output: { text: 'Synthetic audio fixture' }, usage }
      }
      if (input.name === 'synthesize_speech')
        result = {
          output: {},
          usage,
          artifacts: [
            {
              name: 'test.wav',
              mime_type: 'audio/wav',
              // The daemon's ByteBufB64 wire form.
              blob: `b64:${Buffer.from(testTone()).toString('base64url')}`
            }
          ]
        }
    }
    if (method === 'submission/read') result = submissionReceipts.get(params.requestId) || null
    if (submissionId) {
      if (input.prompt === '/side Recovery check') {
        recoveryExecutions++
        result = {
          ...result,
          chat_history: [
            {
              role: 'assistant',
              content: [{ type: 'Text', text: 'Recovered side response' }],
              timestamp: Date.now()
            }
          ]
        }
      }
      result = { state: 'completed', source: input.meta.source, requestId: submissionId, result }
      submissionReceipts.set(submissionId, result)
      if (input.prompt === '/side Recovery check') {
        ws.terminate()
        return
      }
    }
    ws.send(JSON.stringify({ id, result }))
    if (method === 'agent_run' || (method === 'tool_call' && input.name === 'actions_api'))
      ws.send(
        JSON.stringify({
          jsonrpc: '2.0',
          method: 'state/changed',
          params: { instanceId: 'smoke', revision: String(Date.now()) }
        })
      )
  })
})
await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve))
const address = server.address()
const env = {
  ...process.env,
  ANDA_DESKTOP_TEST: '1',
  ANDA_DESKTOP_USER_DATA: directory,
  ANDA_DESKTOP_TEST_URL: `http://127.0.0.1:${address.port}`
}
delete env.ELECTRON_RUN_AS_NODE
let app
let userClipboard
let electronStderr = ''
try {
  app = await electron.launch({
    executablePath: electronPath,
    args: [resolve('.'), '--hidden'],
    env,
    timeout: 30_000
  })
  app.process().stderr?.on('data', (data) => {
    const text = data.toString()
    electronStderr = (electronStderr + text).slice(-64 * 1024)
    if (/audio|media|permission/i.test(text)) console.error(text.trim())
  })
  // Login starts without a renderer. The first menu action must survive creating it.
  await app.evaluate(async ({ Menu, BrowserWindow }) => {
    // Electron creates its default menu before Anda's async setup finishes.
    // Wait for our item, not merely a non-null application menu.
    const deadline = Date.now() + 10_000
    let settings
    while (!(settings = Menu.getApplicationMenu()?.getMenuItemById('anda-settings'))) {
      if (Date.now() >= deadline) throw new Error('Anda Settings menu did not become ready')
      await new Promise((resolve) => setTimeout(resolve, 20))
    }
    if (BrowserWindow.getAllWindows().length) throw new Error('Login start opened a window')
    settings.click()
  })
  let page = await app.firstWindow()
  const errors = []
  const pageError = (error) => {
    errors.push(error.message)
    console.error('Renderer error:', error.message)
  }
  page.on('pageerror', pageError)
  await page.locator('.settings-page').waitFor({ timeout: 30_000 })
  await page
    .locator('.settings-tabs')
    .getByRole('button', { name: 'Agent configuration', exact: true })
    .click()
  await page.getByRole('heading', { name: 'Runtime', exact: true }).waitFor()
  await page.getByRole('button', { name: /^Models/ }).click()
  await page.getByRole('heading', { name: 'Models', exact: true }).waitFor({ timeout: 5000 })
  assert.equal(
    await page.getByRole('button', { name: /^Models/ }).getAttribute('aria-current'),
    'page'
  )
  assert.equal(await page.getByRole('heading', { name: 'Runtime', exact: true }).count(), 0)
  const modelPanel = page.getByRole('region', { name: 'Models', exact: true })
  await modelPanel.getByRole('region', { name: 'ChatGPT plan', exact: true }).waitFor()
  const yamlSource = page.locator('textarea').last()
  assert.ok(
    !(await yamlSource.inputValue()).includes('auth:'),
    'Opening Models must not materialize absent auth fields'
  )
  // Edit a missing nested auth object, then return without losing the draft.
  const profileInput = modelPanel
    .locator('[data-provider-index="0"]')
    .getByText('ChatGPT account', { exact: true })
    .locator('..')
    .locator('input')
  await profileInput.fill('draft-profile')
  assert.ok((await yamlSource.inputValue()).includes('profile: draft-profile'))
  assert.ok((await yamlSource.inputValue()).includes('type: api_key'))
  assert.equal(
    await modelPanel.getByRole('button', { name: 'Use selected model', exact: true }).isDisabled(),
    true
  )
  await page.getByRole('button', { name: /^Runtime/ }).click()
  assert.equal(await page.getByRole('region', { name: 'ChatGPT plan', exact: true }).count(), 0)
  await page.getByRole('button', { name: /^Models/ }).click()
  await page.getByRole('heading', { name: 'Models', exact: true }).waitFor()
  assert.equal(await profileInput.inputValue(), 'draft-profile')

  await page.locator('.settings-tabs').getByRole('button', { name: 'General', exact: true }).click()

  // Capture the actual tray menu on its next refresh, without a production test hook.
  await app.evaluate(({ Tray }) => {
    const setContextMenu = Tray.prototype.setContextMenu
    Tray.prototype.setContextMenu = function (menu) {
      globalThis.smokeTrayMenu = menu
      Tray.prototype.setContextMenu = setContextMenu
      return setContextMenu.call(this, menu)
    }
  })
  await page.evaluate(() => window.anda.preferences({ language: 'en' }))
  // A tray-only session has no renderer to receive the initial progress/result.
  await app.evaluate(({ BrowserWindow }) => BrowserWindow.getAllWindows()[0].destroy())
  const updateWindow = app.waitForEvent('window')
  await app.evaluate(() => {
    globalThis.smokeTrayMenu.items.find((item) => item.label === 'Check for updates').click()
  })
  page = await updateWindow
  page.on('pageerror', pageError)
  const updateDialog = page.getByRole('dialog', { name: 'Check for updates' })
  await updateDialog.getByText('Update failed', { exact: true }).waitFor()
  // Mock tests disable native CLI commands: the real check must surface that failure.
  await updateDialog.getByText('Native daemon commands are disabled in mock tests').waitFor()
  // The same tray action must also report the desktop channel after a runtime failure.
  await updateDialog.getByText('Desktop updates are available only in release builds.').waitFor()
  assert.equal(
    await app.evaluate(({ BrowserWindow }) => BrowserWindow.getAllWindows()[0].isVisible()),
    true
  )
  await page.screenshot({ path: join(screenshotDir, '12-update-error.png') })
  // Exercise progress and result rendering through the native event contract.
  await app.evaluate(({ BrowserWindow }) => {
    BrowserWindow.getAllWindows()[0].webContents.send('anda:event', {
      type: 'update-status',
      value: { phase: 'running', message: 'Checking Anda Desktop updates…' }
    })
  })
  await updateDialog.getByText('Checking for updates…', { exact: true }).waitFor()
  assert.equal(
    await updateDialog.getByRole('button', { name: 'Check for updates', exact: true }).count(),
    0
  )
  await page.screenshot({ path: join(screenshotDir, '13-update-progress.png') })
  await app.evaluate(({ BrowserWindow }) => {
    BrowserWindow.getAllWindows()[0].webContents.send('anda:event', {
      type: 'update-status',
      value: { phase: 'complete', message: 'Anda Desktop is up to date.' }
    })
  })
  await updateDialog.getByText('Update result', { exact: true }).waitFor()
  await updateDialog.getByText('Anda Desktop is up to date.').waitFor()
  await updateDialog.getByRole('button', { name: 'Close', exact: true }).click()
  // The same action also reopens a hidden window and shows its next result.
  await app.evaluate(({ BrowserWindow }) => {
    BrowserWindow.getAllWindows()[0].close()
    globalThis.smokeTrayMenu.items.find((item) => item.label === 'Check for updates').click()
    delete globalThis.smokeTrayMenu
  })
  await updateDialog.getByText('Update failed', { exact: true }).waitFor()
  await updateDialog.getByRole('button', { name: 'Close', exact: true }).click()
  await app.evaluate(({ Menu }) => {
    Menu.getApplicationMenu().getMenuItemById('anda-new-chat').click()
  })
  await page.getByText('What would you like to do?', { exact: true }).waitFor({ timeout: 30_000 })
  await page.screenshot({ path: join(screenshotDir, '01-welcome.png') })
  const editor = page.locator('.composer-container textarea').first()
  await editor.fill('Test the native desktop connection')
  await editor.press('Enter')
  await page
    .getByText('No external model was called.', { exact: false })
    .waitFor({ timeout: 15_000 })
  assert.equal(conversations.size, 1)
  await page.screenshot({ path: join(screenshotDir, '02-chat.png') })
  // Message copy buttons write through navigator.clipboard, which Electron
  // grants only through the session's permission request handler. The user's
  // clipboard text is restored when the run ends.
  userClipboard = await app.evaluate(({ clipboard }) => clipboard.readText())
  const reply = page.locator('article').filter({ hasText: 'No external model was called.' })
  const copied = async (button, type) => {
    await app.evaluate(({ clipboard }) => clipboard.clear())
    await reply.getByRole('button', { name: button, exact: true }).click()
    for (let attempt = 0; attempt < 50; attempt++) {
      const value = await app.evaluate(async ({ clipboard }, type) => {
        const item = (await clipboard.read()).find((item) => item.types.includes(type))
        return item ? (await item.getType(type)).text() : ''
      }, type)
      if (value) return value
      await new Promise((resolve) => setTimeout(resolve, 100))
    }
    assert.fail(`${button} did not write ${type} to the system clipboard`)
  }
  assert.equal((await copied('Copy message', 'text/plain')).replace(/\r\n/g, '\n'), assistantReply)
  assert.match(
    await copied('Copy rich text', 'text/html'),
    /<strong[^>]*>No external model was called\.<\/strong>/
  )
  await editor.fill('An unsent draft')
  await page.waitForTimeout(450)
  await page.locator('.sidebar-primary').getByText('New chat', { exact: true }).click()
  await page
    .locator('.chat-title')
    .filter({ hasText: 'Test the native desktop connection' })
    .click()
  await page.waitForTimeout(300)
  assert.equal(await editor.inputValue(), 'An unsent draft')
  await editor.fill('/side Recovery check')
  await editor.press('Enter')
  await page.getByText('Recovered side response', { exact: true }).waitFor({ timeout: 35000 })
  await page.waitForFunction(async () => (await window.anda.bootstrap()).pending.length === 0)
  assert.equal(recoveryExecutions, 1)
  await editor.fill('/side Direct check')
  await editor.press('Enter')
  await page.getByText('Direct side reply', { exact: true }).waitFor()
  await page.waitForFunction(async () => (await window.anda.bootstrap()).pending.length === 0)
  assert.equal(await page.getByText('Direct side reply', { exact: true }).count(), 1)
  await editor.fill('/side Reload check')
  await editor.press('Enter')
  await page.waitForFunction(async () => (await window.anda.bootstrap()).pending.length === 1)
  await page.reload()
  await page.getByText('No external model was called.', { exact: false }).first().waitFor()
  assert.ok(finishReloadReply)
  finishReloadReply()
  await page.getByText('Side reply after renderer reload', { exact: true }).waitFor()
  await page.waitForFunction(async () => (await window.anda.bootstrap()).pending.length === 0)
  assert.equal(reloadExecutions, 1)
  assert.equal(await page.getByText('Side reply after renderer reload', { exact: true }).count(), 1)
  await page.locator('.sidebar-bottom').getByText('Settings', { exact: true }).click()
  await page.getByRole('heading', { name: 'General', exact: true }).waitFor()
  assert.equal(await page.getByRole('region', { name: 'ChatGPT plan', exact: true }).count(), 0)
  await page
    .locator('.settings-tabs')
    .getByRole('button', { name: 'Agent configuration', exact: true })
    .click()
  await page.getByRole('heading', { name: 'Runtime', exact: true }).waitFor()
  assert.equal(await page.getByRole('region', { name: 'ChatGPT plan', exact: true }).count(), 0)
  await page.getByRole('button', { name: /^Models/ }).click()
  await page.getByRole('heading', { name: 'Models', exact: true }).waitFor()
  const chatgptCard = page.getByRole('region', { name: 'ChatGPT plan', exact: true })
  await chatgptCard.getByRole('button', { name: 'Use selected model', exact: true }).waitFor()
  await chatgptCard.getByRole('button', { name: 'Use selected model', exact: true }).click()
  await chatgptCard.getByText('Using ChatGPT plan', { exact: true }).waitFor()
  assert.equal(chatgptSelected, true)
  await page.waitForFunction(
    () =>
      document.querySelector('#config-panel input[type="text"]')?.value ===
      'chatgpt:test-account:test-plan-model'
  )

  await page.screenshot({ path: join(screenshotDir, '15-chatgpt-settings.png') })
  // Narrow the real window. Playwright's viewport emulation outlives the check:
  // later "narrow" steps stayed at the emulated size, and on Windows the
  // screenshot after the language switch reloaded the page timed out.
  const fullWidth = await page.evaluate(() => window.innerWidth)
  const narrowWidth = 760
  const fullBounds = await app.evaluate(({ BrowserWindow }, width) => {
    const window = BrowserWindow.getAllWindows()[0]
    const bounds = window.getBounds()
    window.setContentSize(width, 740)
    return bounds
  }, narrowWidth)
  await page.waitForFunction((width) => window.innerWidth === width, narrowWidth)
  await page.getByRole('heading', { name: 'Models', exact: true }).scrollIntoViewIfNeeded()
  const narrowForm = await page.getByRole('region', { name: 'Models', exact: true }).boundingBox()
  assert.ok(
    narrowForm && narrowForm.height > 300,
    'Models form must not collapse in a narrow window'
  )
  assert.ok(
    narrowForm.x + narrowForm.width <= narrowWidth + 1,
    'Models form must fit the window width'
  )
  await chatgptCard
    .getByRole('button', { name: 'Use selected model', exact: true })
    .scrollIntoViewIfNeeded()

  await page.screenshot({ path: join(screenshotDir, '16-chatgpt-narrow.png') })
  await app.evaluate(
    ({ BrowserWindow }, bounds) => BrowserWindow.getAllWindows()[0].setBounds(bounds),
    fullBounds
  )
  await page.waitForFunction((width) => window.innerWidth === width, fullWidth)
  await chatgptCard.getByRole('button', { name: 'Sign out', exact: true }).click()
  await chatgptCard.getByRole('button', { name: 'Reconnect / enable plan', exact: true }).waitFor()
  assert.equal(chatgptConnected, false)
  await page.locator('.settings-tabs').getByRole('button', { name: 'General', exact: true }).click()
  await page.screenshot({ path: join(screenshotDir, '03-settings.png') })
  await page.locator('.settings-tabs').getByRole('button', { name: 'Audio', exact: true }).click()
  // The meter counts wall-clock time, but the recorder emits nothing until its
  // encoder has produced a first frame, which a loaded runner delays past the
  // meter's first tick. Stopping before that records zero bytes and the panel
  // reports "No audio was recorded", so stop only once a chunk has arrived.
  await page.evaluate(() => {
    const start = MediaRecorder.prototype.start
    window.recordedBytes = 0
    MediaRecorder.prototype.start = function (...args) {
      this.addEventListener('dataavailable', (event) => (window.recordedBytes += event.data.size))
      return start.apply(this, args)
    }
  })
  await page.getByRole('button', { name: 'Record test', exact: true }).click()
  await page.waitForFunction(
    () => document.querySelector('.audio-meter span')?.textContent !== '0.0 s'
  )
  await page.waitForFunction(() => window.recordedBytes > 0)
  await page.getByRole('button', { name: 'Stop', exact: true }).click()
  await page.waitForFunction(() =>
    document.querySelector('.audio-panel audio')?.src.startsWith('blob:')
  )
  await page.getByRole('button', { name: 'Transcribe', exact: true }).click()
  await page.waitForFunction(
    () => document.querySelector('.audio-panel textarea')?.value === 'Synthetic audio fixture'
  )
  await page.getByRole('button', { name: 'Speak', exact: true }).click()
  await page.waitForFunction(() => document.querySelector('.audio-panel audio')?.duration > 0)
  await page.getByRole('button', { name: 'Stop', exact: true }).click()
  await page.screenshot({ path: join(screenshotDir, '11-audio.png') })
  await page.locator('.settings-tabs').getByRole('button', { name: 'General', exact: true }).click()
  await page.locator('.sidebar-navigation').getByText('Skills', { exact: true }).click()
  await page.waitForTimeout(500)
  await page.locator('.sidebar-navigation').getByText('Memory', { exact: true }).click()
  await page.waitForTimeout(500)
  await page.locator('.sidebar-navigation').getByText('Automations', { exact: true }).click()
  await page.getByRole('heading', { name: 'Automations', exact: true }).waitFor()
  await page.getByRole('heading', { name: 'Long automation', exact: true }).click()
  const automationEditor = page.getByRole('dialog')
  await automationEditor.waitFor()
  assert.equal(await automationEditor.locator('textarea').inputValue(), automation.job)
  await automationEditor.getByLabel('Name', { exact: true }).fill('Renamed automation')
  await automationEditor.getByRole('button', { name: 'Save', exact: true }).click()
  await automationEditor.waitFor({ state: 'hidden' })
  assert.equal(automationReads, 1)
  assert.equal(updatedAutomation.name, 'Renamed automation')
  assert.equal(updatedAutomation.job, automation.job)
  await app.evaluate(({ dialog }, path) => {
    dialog.showOpenDialog = async () => ({ canceled: false, filePaths: [path] })
  }, project)
  // Projects are picked from a new chat's workspace button.
  await page.locator('.sidebar-primary').getByText('New chat', { exact: true }).click()
  await page.locator('.composer-context button').first().click()
  await page.waitForFunction(() =>
    document.querySelector('.composer-context')?.textContent.includes('smoke-project')
  )
  await page.locator('.composer-container textarea').fill('Approval check')
  await page.locator('.composer-container textarea').press('Enter')
  await page.getByRole('button', { name: 'Approve', exact: true }).waitFor()
  await page.screenshot({ path: join(screenshotDir, '04-approval.png') })
  await page.getByRole('button', { name: 'Approve', exact: true }).click()
  await page.getByText(/Shell command approval.*Approved/).waitFor()
  assert.equal(approvals, 1)
  assert.equal(lastWorkspace, project)
  // A chat's menu closes on a click elsewhere, and Rename keeps focus in its
  // dialog after the menu finishes closing.
  const approvalRow = page.locator('.chat-row').filter({ hasText: 'Approval check' })
  await approvalRow.hover()
  await approvalRow.getByRole('button', { name: 'Details' }).click()
  await page.getByRole('menuitem', { name: 'Rename' }).waitFor()
  const main = await page.locator('.main-column').boundingBox()
  await page.mouse.click(main.x + main.width / 2, main.y + main.height / 2)
  await page.getByRole('menu').waitFor({ state: 'detached' })
  await approvalRow.hover()
  await approvalRow.getByRole('button', { name: 'Details' }).click()
  await page.getByRole('menuitem', { name: 'Rename' }).click()
  await page.getByRole('menu').waitFor({ state: 'detached' })
  assert.ok(await page.evaluate(() => document.activeElement?.closest('.rename-dialog') !== null))
  await page.keyboard.press('Escape')
  // Dragging the sidebar's edge resizes it and saves the width; a double-click restores it.
  const sidebarWidth = async () => (await page.locator('.sidebar').boundingBox()).width
  const startWidth = await sidebarWidth()
  const edge = await page.locator('.sidebar-resizer').boundingBox()
  await page.mouse.move(edge.x + edge.width / 2, edge.y + edge.height / 2)
  await page.mouse.down()
  await page.mouse.move(edge.x + edge.width / 2 + 80, edge.y + edge.height / 2, { steps: 4 })
  await page.mouse.up()
  assert.ok(Math.abs((await sidebarWidth()) - (startWidth + 80)) <= 1)
  await page.waitForFunction(
    async (width) =>
      Math.abs((await window.anda.bootstrap()).preferences.sidebarWidth - width) <= 1,
    startWidth + 80
  )
  await page.locator('.sidebar-resizer').dblclick()
  await page.waitForFunction(
    async () => (await window.anda.bootstrap()).preferences.sidebarWidth === 242
  )
  await page.locator('.workspace-header').getByTitle('Resources').click()
  await page
    .locator('.resource-panel')
    .getByRole('button', { name: 'Changes', exact: true })
    .first()
    .click()
  await page.getByText('smoke.txt', { exact: true }).waitFor()
  await page.getByText('smoke.txt', { exact: true }).click()
  await page.locator('.git-diff').getByText('A local Git fixture', { exact: false }).waitFor()
  await page.screenshot({ path: join(screenshotDir, '08-git.png') })
  await page
    .locator('.resource-panel')
    .getByRole('button', { name: 'Terminal', exact: true })
    .click()
  await page.getByRole('button', { name: 'New terminal', exact: true }).click()
  await page
    .locator('.terminal-panel .workbench-tabs')
    .getByRole('button', { name: /1 ·/ })
    .waitFor()
  await page.waitForFunction(
    async (workspace) => (await window.anda.terminal({ action: 'list', workspace })).length === 1,
    project
  )
  const term = (
    await page.evaluate((workspace) => window.anda.terminal({ action: 'list', workspace }), project)
  )[0]
  await page.evaluate(({ id, data }) => window.anda.terminal({ action: 'input', id, data }), {
    id: term.id,
    data: process.platform === 'win32' ? 'echo ANDA_^PTY_OK\r' : "printf 'ANDA_%s_OK\\n' PTY\r"
  })
  await page.waitForFunction(
    async (workspace) =>
      (await window.anda.terminal({ action: 'list', workspace }))[0]?.output.includes(
        'ANDA_PTY_OK'
      ),
    project
  )
  await page.screenshot({ path: join(screenshotDir, '09-terminal.png') })
  await page.evaluate((id) => window.anda.terminal({ action: 'close', id }), term.id)
  // The workbench opens the folder of a chat started from the `anda` terminal,
  // named only by its source, and still refuses any other folder.
  sources[`cli:${terminalProject}`] = { c: 0, s: 'idle', t: Date.now() }
  for (const ws of wsServer.clients)
    ws.send(
      JSON.stringify({
        jsonrpc: '2.0',
        method: 'state/changed',
        params: { instanceId: 'smoke', revision: String(Date.now()) }
      })
    )
  await page.locator('.chat-title').filter({ hasText: 'terminal-project' }).click()
  assert.deepEqual(
    await page.evaluate(
      (workspace) => window.anda.terminal({ action: 'list', workspace }),
      terminalProject
    ),
    []
  )
  await page
    .locator('.resource-panel')
    .getByRole('button', { name: 'Changes', exact: true })
    .first()
    .click()
  await page.getByText('terminal.txt', { exact: true }).waitFor()
  await assert.rejects(
    page.evaluate((workspace) => window.anda.terminal({ action: 'list', workspace }), directory),
    /Select this folder as a project/
  )
  await page.locator('.chat-title').filter({ hasText: 'Approval check' }).click()
  const hiddenSession = [...browserConnections.keys()].at(-1)
  const hiddenTab = await browserAction(hiddenSession, {
    action: 'open_tab',
    url: `http://127.0.0.1:${address.port}/browser-fixture`,
    active: false
  })
  const hiddenSnapshot = await browserAction(hiddenSession, {
    action: 'snapshot',
    tab_id: hiddenTab.tab.id
  })
  assert.ok(JSON.stringify(hiddenSnapshot).includes('Native browser fixture'))
  await browserAction(hiddenSession, {
    action: 'type_text',
    selector: '#name',
    text: 'Hidden browser input',
    tab_id: hiddenTab.tab.id
  })
  await browserAction(hiddenSession, { action: 'click', selector: '#go', tab_id: hiddenTab.tab.id })
  assert.ok(
    JSON.stringify(
      await browserAction(hiddenSession, { action: 'extract_text', tab_id: hiddenTab.tab.id })
    ).includes('Hidden browser input')
  )
  await page
    .locator('.resource-panel')
    .getByRole('button', { name: 'Browser', exact: true })
    .click()
  await page.getByRole('button', { name: 'New tab', exact: true }).last().click()
  const addressInput = page.getByRole('textbox', { name: 'Address', exact: true })
  await addressInput.fill(`http://127.0.0.1:${address.port}/browser-fixture`)
  await addressInput.press('Enter')
  await page.getByRole('button', { name: 'Browser fixture', exact: true }).first().waitFor()
  const browserSession = [...browserConnections.keys()].at(-1)
  const snapshot = await browserAction(browserSession, { action: 'snapshot' })
  assert.ok(JSON.stringify(snapshot).includes('Native browser fixture'))
  await browserAction(browserSession, {
    action: 'type_text',
    selector: '#name',
    text: 'Anda browser input'
  })
  await browserAction(browserSession, { action: 'click', selector: '#go' })
  const text = await browserAction(browserSession, { action: 'extract_text' })
  assert.ok(JSON.stringify(text).includes('Anda browser input'), JSON.stringify(text))
  await browserAction(browserSession, { action: 'copy_to_clipboard', text: 'Anda browser copy' })
  assert.equal(await app.evaluate(({ clipboard }) => clipboard.readText()), 'Anda browser copy')
  const captured = await browserAction(browserSession, { action: 'screenshot' })
  assert.ok(captured.data_url.startsWith('data:image/png;base64,'))
  await writeFile(
    join(screenshotDir, '10-browser-content.png'),
    Buffer.from(captured.data_url.split(',')[1], 'base64')
  )
  const boundary = await browserAction(browserSession, {
    action: 'execute_javascript',
    code: '({ bridge: typeof window.anda, node: typeof require })'
  })
  assert.deepEqual(boundary.result, { bridge: 'undefined', node: 'undefined' })
  const bodyScript = await browserAction(browserSession, {
    action: 'execute_javascript',
    code: 'const title = document.title; return { title };'
  })
  assert.equal(bodyScript.result.title, 'Browser fixture')
  const resizedShot = await browserAction(browserSession, {
    action: 'screenshot',
    viewport_width: 800,
    viewport_height: 600,
    device_scale_factor: 1
  })
  const png = Buffer.from(resizedShot.data_url.split(',')[1], 'base64')
  assert.equal(png.readUInt32BE(16), 800)
  assert.equal(png.readUInt32BE(20), 600)
  await page.screenshot({ path: join(screenshotDir, '10-browser.png') })
  const nativeViews = await app.evaluate(
    ({ BrowserWindow }) => BrowserWindow.getAllWindows()[0].contentView.children.length
  )
  await browserAction(browserSession, { action: 'close_tab', tab_id: hiddenTab.tab.id })
  await page.waitForTimeout(100)
  assert.equal(
    await app.evaluate(
      ({ BrowserWindow }) => BrowserWindow.getAllWindows()[0].contentView.children.length
    ),
    nativeViews - 1
  )
  // Below 1150px the panel is a drawer over the header's Resources toggle, as
  // on 1024px-wide CI displays, so close it from the panel itself.
  await page
    .locator('.resource-panel > header')
    .getByRole('button', { name: 'Close', exact: true })
    .click()
  await app.evaluate(({ BrowserWindow }) =>
    BrowserWindow.getAllWindows()[0].setBounds({ width: 900, height: 700 })
  )
  await page.screenshot({ path: join(screenshotDir, '05-narrow.png') })
  await page.locator('.sidebar-bottom').getByText('Settings', { exact: true }).click()
  await page.locator('.setting-row').getByRole('button', { name: 'Appearance' }).click()
  await page.getByRole('menuitem', { name: 'Dark' }).click()
  await page.waitForFunction(() => document.documentElement.classList.contains('dark'))
  await page.screenshot({ path: join(screenshotDir, '06-dark.png') })
  await page.locator('.setting-row').getByRole('button', { name: 'Language' }).click()
  await page.getByRole('menuitem', { name: '简体中文' }).click()
  await page.getByText('最近', { exact: true }).waitFor({ timeout: 15_000 })
  await page.screenshot({ path: join(screenshotDir, '07-chinese.png') })
  await page.locator('.sidebar-bottom').getByText('设置', { exact: true }).click()
  await page.getByRole('button', { name: '检查更新', exact: true }).click()
  const chineseUpdate = page.getByRole('dialog', { name: '检查更新' })
  await chineseUpdate.getByText('更新失败', { exact: true }).waitFor()
  await page.screenshot({ path: join(screenshotDir, '14-update-chinese-narrow.png') })
  await chineseUpdate.getByRole('button', { name: '关闭', exact: true }).click()
  const secrets = await page.evaluate(() =>
    JSON.stringify({
      storage: { ...localStorage },
      windowKeys: Object.keys(window.anda)
    })
  )
  assert.ok(!secrets.includes('desktop-test-token'))
  // Exercise native window transitions after the renderer and browser checks.
  await app.evaluate(({ Menu }) => {
    Menu.getApplicationMenu().getMenuItemById('anda-new-chat').click()
  })
  await editor.fill('Draft survives closing the window')
  const closeModes =
    process.platform === 'darwin' ? ['windowed', 'fullscreen', 'reopen-during-exit'] : ['windowed']
  for (const mode of closeModes) {
    const closed = await app.evaluate(async ({ app, BrowserWindow }, mode) => {
      const window = BrowserWindow.getAllWindows()[0]
      const fullScreen = mode !== 'windowed'
      const waitFor = (event) =>
        new Promise((resolve, reject) => {
          const timer = setTimeout(() => {
            window.removeListener(event, done)
            reject(new Error(`Window did not emit ${event}`))
          }, 10_000)
          const done = () => {
            clearTimeout(timer)
            resolve()
          }
          window.once(event, done)
        })
      if (fullScreen) {
        const entered = waitFor('enter-full-screen')
        window.setFullScreen(true)
        await entered
      }
      // macOS also emits hide while animating between Spaces. Wait for the
      // fullscreen exit before checking the final visibility of the window.
      const closed = waitFor(fullScreen ? 'leave-full-screen' : 'hide')
      window.close()
      // A second click during the native transition must not hide it early.
      if (fullScreen) window.close()
      if (mode === 'reopen-during-exit') app.emit('activate')
      await closed
      const result = {
        hidden: !window.isVisible(),
        fullScreen: window.isFullScreen(),
        destroyed: window.isDestroyed()
      }
      // Dock activation must reopen the same renderer, preserving its draft.
      app.emit('activate')
      return {
        ...result,
        reopened: window.isVisible() && BrowserWindow.getAllWindows()[0] === window
      }
    }, mode)
    assert.deepEqual(
      closed,
      {
        hidden: mode !== 'reopen-during-exit',
        fullScreen: false,
        destroyed: false,
        reopened: true
      },
      `Close and reopen (${mode})`
    )
    assert.equal(await editor.inputValue(), 'Draft survives closing the window')
  }
  assert.deepEqual(errors, [])
  console.log(
    'PASS: hidden login and first menu action, tray/settings update dialog with progress and results, window close/reopen (including macOS fullscreen), Electron IPC/WS, receipt-backed chat including renderer reload, message and browser clipboard copy, model settings navigation and draft preservation, ChatGPT placement and narrow form sizing, full automation editing, approvals, chat menu and sidebar resizing, drafts, Git diff, PTY output, workbench folders of terminal-started chats, browser tools and isolation, synthetic audio recording/transcription/TTS, narrow layout, theme and locale. Screenshots: desktop/test-results'
  )
} catch (error) {
  // Startup may fail before any window exists. Diagnostics must not replace
  // that original failure with a second firstWindow() timeout.
  console.error('Electron smoke test failed:', error)
  await writeFile(
    join(screenshotDir, 'failure.txt'),
    `${error?.stack || error}\n\n${electronStderr}`
  ).catch(() => {})
  const page = app?.windows().find((window) => !window.isClosed())
  if (page) {
    await page
      .screenshot({ path: join(screenshotDir, 'failure.png'), timeout: 5000 })
      .catch(() => {})
    console.error(
      'Visible failure:',
      await page
        .locator('.status-banner, .workbench-error')
        .allTextContents()
        .catch(() => [])
    )
  }
  throw error
} finally {
  if (userClipboard !== undefined)
    await app
      .evaluate(({ clipboard }, text) => clipboard.writeText(text), userClipboard)
      .catch(() => {})
  await app?.close()
  for (const ws of wsServer.clients) ws.terminate()
  await new Promise((resolve) => wsServer.close(resolve))
  await new Promise((resolve) => server.close(resolve))
}
