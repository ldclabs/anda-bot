import { _electron as electron } from 'playwright'
import electronPath from 'electron'
import { createServer } from 'node:http'
import { WebSocketServer } from 'ws'
import { mkdtemp, mkdir } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import assert from 'node:assert/strict'

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
const server = createServer((req, res) => {
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
    const { id, method, params } = JSON.parse(raw.toString())
    const input = params[0] || {}
    let result = {}
    if (method === 'model_names')
      result = {
        active_model: 'Local test model',
        model_names: ['Local test model']
      }
    else if (method === 'capabilities')
      result = {
        transcription: [],
        tts: [],
        desktop: {
          protocol: 1,
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
      if (input.name === 'conversations_api') {
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
    }
    ws.send(JSON.stringify({ id, result }))
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
  const errors = []
  page.on('pageerror', (error) => errors.push(error.message))
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
  await page.locator('.sidebar-bottom').getByText('Settings', { exact: true }).click()
  await page.getByRole('heading', { name: 'General', exact: true }).waitFor()
  await page.screenshot({ path: join(screenshotDir, '03-settings.png') })
  await page.locator('.sidebar-navigation').getByText('Skills', { exact: true }).click()
  await page.waitForTimeout(500)
  await page.locator('.sidebar-navigation').getByText('Memory', { exact: true }).click()
  await page.waitForTimeout(500)
  await page.locator('.sidebar-navigation').getByText('Automations', { exact: true }).click()
  await page.getByRole('heading', { name: 'Automations', exact: true }).waitFor()
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
    'PASS: native Electron startup, authenticated IPC/WS chat, draft switching, management navigation, project authorization, approval response, narrow layout, dark theme, Chinese locale, credential boundary. Screenshots: desktop/test-results'
  )
} catch (error) {
  if (app) {
    const page = await app.firstWindow()
    await page.screenshot({ path: join(screenshotDir, 'failure.png') }).catch(() => {})
    console.error('Visible failure:', await page.locator('.status-banner').allTextContents())
  }
  throw error
} finally {
  await app?.close()
  for (const ws of wsServer.clients) ws.terminate()
  await new Promise((resolve) => wsServer.close(resolve))
  await new Promise((resolve) => server.close(resolve))
}
