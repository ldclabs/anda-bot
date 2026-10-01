// Isolated desktop onboarding: no real credentials, daemon, or model requests.
import { _electron as electron } from 'playwright'
import electronPath from 'electron'
import { createServer } from 'node:http'
import { WebSocketServer } from 'ws'
import { mkdtemp, mkdir, readFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import assert from 'node:assert/strict'
import { parse, parseDocument } from 'yaml'

const template = await readFile('../anda_bot/assets/config.yaml', 'utf8')
const screenshots = resolve('test-results/onboarding')
await mkdir(screenshots, { recursive: true })
let config = template
let revision = 1
let issues = ['model.providers[0].api_key']
let configured = false
let rejectSave = false
let loginComplete = false
let accountConnected = false
let cancelled = 0
let submissions = 0
let modelReads = 0
const server = createServer(async (req, res) => {
  res.setHeader('Content-Type', 'application/json')
  if (req.url === '/daemon/status') {
    res.end(JSON.stringify({ needs_setup: !configured }))
    return
  }
  assert.equal(req.headers.authorization, 'Bearer desktop-test-token')
  let body = ''
  for await (const chunk of req) body += chunk
  if (req.url === '/daemon/config') {
    if (req.method === 'PUT') {
      const update = JSON.parse(body)
      if (rejectSave || update.expected_revision !== String(revision)) {
        res.writeHead(409).end('{}')
        return
      }
      config = update.content
      revision++
      configured = true
    }
    res.end(
      JSON.stringify({
        content: config,
        config: parse(config),
        revision: String(revision),
        setup_issues: issues
      })
    )
    return
  }
  if (req.url === '/daemon/chatgpt') {
    const request = JSON.parse(body)
    let result = {}
    if (request.method === 'accounts')
      result = {
        accounts: accountConnected
          ? [{ id: 'test-account', label: 'Test account', connected: true, plan_enabled: true }]
          : [],
        needs_setup: !configured
      }
    if (request.method === 'login_start') result = { flow_id: 'test-flow', status: 'pending' }
    if (request.method === 'login_cancel') {
      cancelled++
      result = { status: 'cancelled' }
    }
    if (request.method === 'login_status') {
      accountConnected = loginComplete
      result = {
        flow_id: 'test-flow',
        status: loginComplete ? 'completed' : 'pending',
        account_id: 'test-account'
      }
    }
    if (request.method === 'models')
      result = { models: [{ slug: 'test-model', display_name: 'Test model' }] }
    if (request.method === 'model_select') {
      const doc = parseDocument(config)
      doc.setIn(['model', 'active'], 'chatgpt:test-account:test-model')
      config = doc.toString()
      configured = true
    }
    res.end(JSON.stringify(result))
    return
  }
  res.writeHead(404).end('{}')
})
const sockets = new WebSocketServer({ server })
sockets.on('connection', (ws) =>
  ws.on('message', (raw) => {
    const { id, method, params } = JSON.parse(raw.toString())
    if (!method) return
    let result = {}
    if (method === 'model_names' || method === 'reload_models') {
      // First read deliberately lacks the requested model. Saving is not completion.
      modelReads++
      const active = modelReads === 1 ? 'previous-model' : parse(config).model.active
      result = { active_model: active, model_names: [active] }
    }
    if (method === 'agent_run') submissions++
    if (method === 'tool_call') {
      const args = params[0].args
      const value =
        args.type === 'ListSourceState' ? {} : args.type === 'GetSourceState' ? { c: 0 } : []
      result = { output: { result: value }, usage: {} }
    }
    ws.send(JSON.stringify({ id, result }))
  })
)
await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve))
let app
let page
const errors = []
async function launch(profile) {
  const env = {
    ...process.env,
    ANDA_DESKTOP_TEST: '1',
    ANDA_DESKTOP_USER_DATA: profile,
    ANDA_DESKTOP_TEST_URL: `http://127.0.0.1:${server.address().port}`
  }
  delete env.ELECTRON_RUN_AS_NODE
  app = await electron.launch({ executablePath: electronPath, args: [resolve('.')], env })
  page = await app.firstWindow()
  page.on('pageerror', (error) => errors.push(error.message))
  page.setDefaultTimeout(15_000)
}
const freshProfile = () => mkdtemp(join(tmpdir(), 'anda-onboarding-'))
try {
  const profile = await freshProfile()
  await launch(profile)
  const dialog = () => page.getByRole('dialog')
  await dialog().getByRole('heading', { name: 'Welcome to Anda' }).waitFor()
  await page.screenshot({ path: join(screenshots, '01-welcome.png') })
  await dialog().getByRole('button', { name: 'Set up later', exact: true }).click()
  await page.locator('.composer-textarea').fill('Keep my first message')
  await page.locator('.composer-textarea').press('Enter')
  await dialog().getByRole('heading', { name: 'Welcome to Anda' }).waitFor()
  assert.equal(submissions, 0)
  await dialog().getByRole('button', { name: 'Close', exact: true }).click()
  await page.reload()
  await page.locator('.composer-textarea:not(:disabled)').waitFor()
  assert.equal(await dialog().count(), 0, 'A dismissed wizard must stay dismissed after reload')
  assert.equal(await page.locator('.composer-textarea').inputValue(), 'Keep my first message')
  await page
    .locator('.status-banner')
    .getByRole('button', { name: 'Connect a model', exact: true })
    .click()
  await dialog()
    .getByRole('button', { name: /Use another model service/ })
    .click()
  const preset = parse(template).model.providers.at(-1)
  assert.equal(await dialog().getByRole('radio').count(), parse(template).model.providers.length)
  await dialog()
    .getByRole('radio', { name: new RegExp(preset.model) })
    .check()
  await dialog().getByLabel('API key', { exact: true }).fill('onboarding-test-key')
  await page.screenshot({ path: join(screenshots, '02-presets.png') })
  await page.setViewportSize({ width: 480, height: 640 })
  assert.ok(await dialog().evaluate((el) => el.scrollWidth <= el.clientWidth))
  await page.screenshot({ path: join(screenshots, '03-presets-narrow.png') })
  rejectSave = true
  await dialog().getByRole('button', { name: 'Connect and start', exact: true }).click()
  await dialog().getByRole('alert').waitFor()
  assert.equal(config, template, 'A revision conflict must preserve the existing config')
  assert.equal(
    await dialog().getByLabel('API key', { exact: true }).inputValue(),
    'onboarding-test-key'
  )
  rejectSave = false
  await dialog().getByRole('button', { name: 'Connect and start', exact: true }).click()
  await dialog().getByRole('heading', { name: 'Your model is ready' }).waitFor()
  const saved = parse(config)
  assert.equal(saved.model.active, preset.model)
  assert.deepEqual(saved.model.providers.at(-1), {
    ...preset,
    api_key: 'onboarding-test-key',
    disabled: false
  })
  assert.deepEqual(saved.model.providers.slice(0, -1), parse(template).model.providers.slice(0, -1))
  await page.screenshot({ path: join(screenshots, '04-ready.png') })
  await dialog().getByRole('button', { name: 'Start chatting', exact: true }).click()
  assert.equal(await page.locator('.composer-textarea').inputValue(), 'Keep my first message')
  assert.equal(submissions, 0)
  assert.equal(
    await page.locator('.composer-textarea').evaluate((el) => el === document.activeElement),
    true
  )
  await app.close()
  app = null
  await launch(profile)
  await page.locator('.composer-textarea:not(:disabled)').waitFor()
  assert.equal(await dialog().count(), 0, 'A configured profile must not be interrupted')
  assert.ok(
    !(await readFile(join(profile, 'desktop.json'), 'utf8')).includes('onboarding-test-key')
  )
  await page.evaluate(() => window.anda.preferences({ language: 'zh_CN', theme: 'dark' }))
  await page.reload()
  await page.locator('.sidebar-bottom').getByText('设置', { exact: true }).click()
  await page.getByRole('button', { name: '连接模型', exact: true }).click()
  await dialog()
    .getByRole('button', { name: /连接其他模型服务/ })
    .click()
  await page.setViewportSize({ width: 480, height: 640 })
  await dialog().getByRole('heading', { name: '选择模型', exact: true }).waitFor()
  await page.screenshot({ path: join(screenshots, '06-presets-chinese-dark.png') })
  await app.close()
  app = null

  // ChatGPT cancellation, then successful login + explicit model activation.
  configured = false
  config = template
  modelReads = 0
  await launch(await freshProfile())
  await dialog()
    .getByRole('button', { name: /Continue with ChatGPT/ })
    .click()
  await dialog().getByRole('button', { name: 'Cancel', exact: true }).waitFor()
  await dialog().getByRole('button', { name: 'Back', exact: true }).click()
  await page.waitForTimeout(100)
  assert.equal(cancelled, 1)
  loginComplete = true
  await dialog()
    .getByRole('button', { name: /Continue with ChatGPT/ })
    .click()
  await dialog().getByRole('button', { name: 'Use selected model', exact: true }).waitFor()
  assert.equal(configured, false, 'Login alone must not complete setup')
  await page.screenshot({ path: join(screenshots, '05-chatgpt.png') })
  await dialog().getByRole('button', { name: 'Use selected model', exact: true }).click()
  await dialog().getByRole('heading', { name: 'Your model is ready' }).waitFor()
  assert.equal(parse(config).model.active, 'chatgpt:test-account:test-model')
  await app.close()
  app = null

  // A channel problem must not open the model wizard.
  configured = false
  issues = ['channels.telegram[0].bot_token']
  await launch(await freshProfile())
  await page.getByText('Complete your configuration to start chatting.', { exact: true }).waitFor()
  assert.equal(await dialog().count(), 0)
  assert.deepEqual(errors, [])
  console.log(
    'Onboarding smoke passed: presets, revision conflict, drafts, dismissal, restart, ChatGPT, and non-model setup issues.'
  )
} catch (error) {
  if (page) await page.screenshot({ path: join(screenshots, 'failure.png') }).catch(() => {})
  throw error
} finally {
  await app?.close()
  for (const ws of sockets.clients) ws.terminate()
  await new Promise((resolve) => sockets.close(resolve))
  await new Promise((resolve) => server.close(resolve))
}
