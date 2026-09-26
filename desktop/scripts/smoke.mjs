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
const sources = {}
const conversations = new Map()
let next = 1
let approvals = 0
let lastWorkspace = null
const project = join(directory, 'smoke-project')
await mkdir(project)
await promisify(execFile)('git', ['init', '-b', 'main'], { cwd: project })
await writeFile(join(project, 'smoke.txt'), 'A local Git fixture\n')
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
const server = createServer((req, res) => {
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
      content: 'addr: 127.0.0.1:8042\n',
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
        active_model: 'Local test model',
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
          config_revision: true,
          managed_runtime: false
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
        content: [
          {
            type: 'Text',
            text: 'Hello from the local daemon.\n\nYour desktop connection is working. **No external model was called.**'
          }
        ],
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
              blob: Buffer.from(testTone()).toString('base64')
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
try {
  app = await electron.launch({
    executablePath: electronPath,
    args: [resolve('.')],
    env,
    timeout: 30_000
  })
  const page = await app.firstWindow()
  app.process().stderr?.on('data', (data) => {
    if (/audio|media|permission/i.test(data.toString())) console.error(data.toString().trim())
  })
  const errors = []
  page.on('pageerror', (error) => {
    errors.push(error.message)
    console.error('Renderer error:', error.message)
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
  await page.screenshot({ path: join(screenshotDir, '03-settings.png') })
  await page.locator('.settings-tabs').getByRole('button', { name: 'Audio', exact: true }).click()
  await page.getByRole('button', { name: 'Record test', exact: true }).click()
  await page.waitForFunction(
    () => document.querySelector('.audio-meter span')?.textContent !== '0.0 s'
  )
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
  await page.locator('.add-project').click()
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
  await page.locator('.workspace-header').getByTitle('Resources').click()
  await app.evaluate(({ BrowserWindow }) =>
    BrowserWindow.getAllWindows()[0].setBounds({ width: 900, height: 700 })
  )
  await page.screenshot({ path: join(screenshotDir, '05-narrow.png') })
  await page.locator('.sidebar-bottom').getByText('Settings', { exact: true }).click()
  await page.locator('.setting-row select').first().selectOption('dark')
  await page.waitForFunction(() => document.documentElement.classList.contains('dark'))
  await page.screenshot({ path: join(screenshotDir, '06-dark.png') })
  await page.locator('.setting-row select').nth(1).selectOption('zh_CN')
  await page.getByText('最近', { exact: true }).waitFor({ timeout: 15_000 })
  await page.screenshot({ path: join(screenshotDir, '07-chinese.png') })
  const secrets = await page.evaluate(() =>
    JSON.stringify({
      storage: { ...localStorage },
      windowKeys: Object.keys(window.anda)
    })
  )
  assert.ok(!secrets.includes('desktop-test-token'))
  assert.deepEqual(errors, [])
  console.log(
    'PASS: Electron IPC/WS, receipt-backed chat including renderer reload, full automation editing, approvals, drafts, Git diff, PTY output, browser tools and isolation, synthetic audio recording/transcription/TTS, narrow layout, theme and locale. Screenshots: desktop/test-results'
  )
} catch (error) {
  if (app) {
    const page = await app.firstWindow()
    await page.screenshot({ path: join(screenshotDir, 'failure.png') }).catch(() => {})
    console.error(
      'Visible failure:',
      await page.locator('.status-banner, .workbench-error').allTextContents()
    )
  }
  throw error
} finally {
  await app?.close()
  for (const ws of wsServer.clients) ws.terminate()
  await new Promise((resolve) => wsServer.close(resolve))
  await new Promise((resolve) => server.close(resolve))
}
