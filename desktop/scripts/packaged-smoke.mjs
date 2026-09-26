import { _electron as electron } from 'playwright'
import { mkdtemp, mkdir, writeFile, readFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { resolve, join } from 'node:path'
import { createHash } from 'node:crypto'
import { createServer } from 'node:http'
import assert from 'node:assert/strict'

const bundle = resolve(process.argv[2])
const expectedVersion = JSON.parse(
  await readFile(new URL('../package.json', import.meta.url), 'utf8')
).version
const resources =
  process.platform === 'darwin' ? join(bundle, 'Contents/Resources') : join(bundle, 'resources')
const runtime = join(resources, 'runtime', process.platform === 'win32' ? 'anda.exe' : 'anda')
const manifest = JSON.parse(await readFile(join(resources, 'runtime/manifest.json'), 'utf8'))
assert.equal(
  manifest.sha256,
  createHash('sha256')
    .update(await readFile(runtime))
    .digest('hex')
)
const directory = await mkdtemp(join(tmpdir(), 'anda-packaged-smoke-'))
const profile = join(directory, 'profile'),
  home = join(directory, 'home'),
  workspace = join(directory, 'project')
await Promise.all([profile, home, workspace].map((path) => mkdir(path)))
await writeFile(
  join(profile, 'desktop.json'),
  JSON.stringify({
    daemonStopped: true,
    preferences: {
      language: 'en',
      projects: [{ id: 'test', path: workspace, name: 'Package test' }],
      chats: [],
      drafts: {}
    },
    storage: {},
    pending: []
  })
)
const env = { ...process.env }
delete env.ELECTRON_RUN_AS_NODE
delete env.ANDA_DESKTOP_TEST
const server = createServer((_request, response) => {
  response.setHeader('Content-Type', 'text/html')
  response.end('<!doctype html><title>Packaged browser</title><h1>Packaged native view</h1>')
})
await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve))
let application
try {
  application = await electron.launch({
    executablePath:
      process.platform === 'darwin'
        ? join(bundle, 'Contents/MacOS/Anda')
        : join(bundle, 'Anda.exe'),
    args: [`--anda-home=${home}`, `--anda-profile=${profile}`],
    env,
    timeout: 30000
  })
  const page = await application.firstWindow()
  await page.getByText('What would you like to do?', { exact: true }).waitFor({ timeout: 30000 })
  const boot = await page.evaluate(() => window.anda.bootstrap())
  assert.equal(boot.daemon.connected, false)
  assert.equal(boot.version, expectedVersion)
  const terminal = await page.evaluate(
    (workspace) => window.anda.terminal({ action: 'create', workspace, cols: 80, rows: 24 }),
    workspace
  )
  await page.evaluate(({ id, data }) => window.anda.terminal({ action: 'input', id, data }), {
    id: terminal.id,
    data:
      process.platform === 'win32' ? 'echo PACKAGED_^PTY_OK\r' : "printf 'PACKAGED_%s_OK\\n' PTY\r"
  })
  await page.waitForFunction(
    async (workspace) =>
      (await window.anda.terminal({ action: 'list', workspace }))[0]?.output.includes(
        'PACKAGED_PTY_OK'
      ),
    workspace
  )
  await page.evaluate((id) => window.anda.terminal({ action: 'close', id }), terminal.id)
  const browser = await page.evaluate(() =>
    window.anda.browser({ action: 'new', source: 'desktop:packaged-test' })
  )
  const url = `http://127.0.0.1:${server.address().port}/packaged`
  await page.evaluate(
    ({ id, url }) =>
      window.anda.browser({ action: 'navigate', source: 'desktop:packaged-test', id, url }),
    { id: browser.active, url }
  )
  await page.waitForFunction(
    async ({ id, url }) => {
      const state = await window.anda.browser({ action: 'state', source: 'desktop:packaged-test' })
      const tab = state.tabs.find((tab) => tab.id === id)
      return tab?.url === url && !tab.loading
    },
    { id: browser.active, url }
  )
  const boundary = await application.evaluate(async ({ webContents }, url) => {
    const page = webContents.getAllWebContents().find((w) => w.getURL() === url)
    return page.executeJavaScript(
      '({ text: document.body.innerText, bridge: typeof window.anda, node: typeof require })'
    )
  }, url)
  assert.equal(boundary.bridge, 'undefined')
  assert.equal(boundary.node, 'undefined')
  assert.ok(boundary.text.includes('Packaged native view'))
  await mkdir(resolve('test-results'), { recursive: true })
  await page.screenshot({ path: resolve('test-results/12-packaged.png') })
  console.log(
    'PASS: packaged application startup, final runtime digest, signed native PTY and isolated browser. Real daemon remained stopped.'
  )
} finally {
  await application?.close()
  await new Promise((resolve) => server.close(resolve))
}
