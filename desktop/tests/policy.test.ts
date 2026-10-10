import { mkdir, mkdtemp, realpath, rm, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { resolve, join, sep } from 'node:path'
import { afterEach, describe, expect, it } from 'vitest'
import {
  appPermissionAllowed,
  authorizeWorkspace,
  externalUrl,
  loopbackBaseUrl,
  mcpInstallLink,
  navigationSource,
  validateRpc,
  rendererAssetPath
} from '../src/main/policy'
import type { ChatEntry } from '../src/shared/contract'
import { desktopMessages } from '../src/renderer/labels'

it('permits application audio only in its own main frame, never camera or preview frames', () => {
  expect(appPermissionAllowed(1, 1, 'media', { isMainFrame: true, mediaType: 'audio' })).toBe(true)
  expect(appPermissionAllowed(1, 1, 'media', { isMainFrame: true, mediaType: 'video' })).toBe(false)
  expect(appPermissionAllowed(1, 1, 'media', { isMainFrame: false, mediaType: 'audio' })).toBe(
    false
  )
  expect(appPermissionAllowed(1, 2, 'media', { isMainFrame: true, mediaType: 'audio' })).toBe(false)
  expect(appPermissionAllowed(1, 1, 'media', { isMainFrame: true })).toBe(false)
  expect(appPermissionAllowed(1, 1, 'media', { isMainFrame: true, mediaTypes: ['audio'] })).toBe(
    true
  )
  expect(
    appPermissionAllowed(1, 1, 'media', { isMainFrame: true, mediaTypes: ['audio', 'video'] })
  ).toBe(false)
})

it('lets the application main frame write, but never read, the clipboard', () => {
  expect(appPermissionAllowed(1, 1, 'clipboard-sanitized-write', { isMainFrame: true })).toBe(true)
  expect(appPermissionAllowed(1, 1, 'clipboard-sanitized-write', { isMainFrame: false })).toBe(
    false
  )
  expect(appPermissionAllowed(1, 2, 'clipboard-sanitized-write', { isMainFrame: true })).toBe(false)
  expect(appPermissionAllowed(1, 1, 'clipboard-read', { isMainFrame: true })).toBe(false)
})

it('keeps all six desktop language dictionaries complete', () => {
  const keys = Object.keys(desktopMessages.en!).sort()
  expect(Object.keys(desktopMessages).sort()).toEqual(['ar', 'en', 'es', 'fr', 'ru', 'zh_CN'])
  for (const messages of Object.values(desktopMessages)) {
    expect(Object.keys(messages).sort()).toEqual(keys)
    expect(Object.values(messages).every((text) => text.trim().length > 0)).toBe(true)
  }
})

describe('desktop host boundaries', () => {
  it('serves only assets inside the application renderer directory', () => {
    expect(rendererAssetPath(resolve('renderer'), 'anda-app://app/assets/ui.js')).toBe(
      join(resolve('renderer'), 'assets/ui.js')
    )
    expect(() => rendererAssetPath('/app/renderer', 'anda-app://app/..%2f..%2fsecrets')).toThrow()
    expect(() => rendererAssetPath('/app/renderer', 'anda-app://other/index.html')).toThrow()
    expect(() => rendererAssetPath('/app/renderer', 'file:///etc/passwd')).toThrow()
  })
  it('never sends a daemon credential to a remote URL or URL containing credentials', () => {
    expect(loopbackBaseUrl('http://127.0.0.1:8042')).toBe('http://127.0.0.1:8042')
    for (const url of [
      'https://example.com',
      'http://localhost.example.com',
      'http://user:secret@localhost',
      'http://127.0.0.1/?token=x',
      'file:///etc/passwd'
    ])
      expect(() => loopbackBaseUrl(url)).toThrow()
  })
  it('restricts renderer RPC and tool selection', () => {
    expect(() =>
      validateRpc('tool_call', [{ name: 'shell', args: { command: 'anything' } }])
    ).toThrow()
    expect(() => validateRpc('auto_update_install_and_restart', [])).toThrow()
    expect(() =>
      validateRpc('agent_run', [
        { name: '', prompt: 'reply', meta: { source: 'wechat:reply_target:user:thread:family' } }
      ])
    ).toThrow('read-only')
    expect(() =>
      validateRpc('tool_call', [{ name: 'conversations_api', args: { type: 'ListSourceState' } }])
    ).not.toThrow()
    expect(() => validateRpc('memory_change_commit', [{ operation_id: 'op' }])).not.toThrow()
    expect(() => validateRpc('memory_entity', [{ id: null }])).not.toThrow()
    expect(() => validateRpc('memory_entity_search', [{ query: 'x' }])).not.toThrow()
    // The graph page is gone; its raw Brain reads stay closed to the renderer.
    expect(() => validateRpc('brain_kip_readonly', [{}])).toThrow('not available')
    // The MCP page manages the owner's servers; the daemon checks the owner.
    expect(() => validateRpc('mcp_apply', [{ change: { op: 'reload' } }])).not.toThrow()
    expect(() => validateRpc('mcp_secrets', [{}])).not.toThrow()
    expect(() => validateRpc('mcp_import_scan', [{}])).not.toThrow()
    expect(() => validateRpc('mcp_registry_search', [{ query: 'x' }])).not.toThrow()
    expect(() => validateRpc('mcp_events_list', [{}])).toThrow('not available')
  })
  it('reads MCP install links into a configuration to check, never more', () => {
    const config = Buffer.from(JSON.stringify({ url: 'https://docs.test/mcp' })).toString(
      'base64url'
    )
    expect(mcpInstallLink(`anda://mcp/install?name=docs&config=${config}`)).toEqual({
      name: 'docs',
      config: JSON.stringify({ url: 'https://docs.test/mcp' }, null, 2)
    })
    const list = Buffer.from('[1]').toString('base64url')
    for (const link of [
      `anda://mcp/install?name=docs&config=${list}`,
      'anda://mcp/install?name=docs&config=!!!',
      'anda://mcp/install?name=docs',
      `anda://mcp/remove?name=docs&config=${config}`,
      `anda://chat?source=x`,
      `anda://mcp/install?name=${'x'.repeat(129)}&config=${config}`
    ])
      expect(mcpInstallLink(link)).toBeNull()
  })
  it('limits deep links to navigation and external links to safe protocols', () => {
    expect(navigationSource('anda://chat?source=desktop%3Aone')).toBe('desktop:one')
    expect(navigationSource('anda://exec?command=rm')).toBeNull()
    expect(() => externalUrl('file:///etc/passwd')).toThrow()
    expect(() => externalUrl('javascript:alert(1)')).toThrow()
    expect(externalUrl('https://anda.bot')).toBe('https://anda.bot/')
  })
})

describe('workbench workspaces', () => {
  const directories: string[] = []
  afterEach(async () => {
    for (const path of directories.splice(0)) await rm(path, { recursive: true, force: true })
  })
  async function folders(...names: string[]): Promise<string[]> {
    const home = await realpath(await mkdtemp(join(tmpdir(), 'anda-workspace-test-')))
    directories.push(home)
    const paths = names.map((name) => join(home, name))
    for (const path of paths) await mkdir(path)
    return paths
  }
  const chat = (source: string, workspace?: string): ChatEntry => ({
    source,
    title: source,
    updatedAt: 0,
    ...(workspace ? { workspace } : {})
  })

  it('opens the folder a chat started in a terminal names in its source', async () => {
    const [cli, voice, plain, chosen, project] = await folders(
      'cli',
      'voice',
      'plain',
      'chosen',
      'project'
    )
    const preferences = {
      projects: [{ id: 'p', path: project, name: 'project' }],
      // Chats listed from the daemon carry their folder only in the source.
      chats: [
        chat(`cli:${cli}${sep}`),
        chat(`cli:voice:${voice}`),
        chat(plain),
        chat('desktop:chat-1', chosen)
      ]
    }

    for (const path of [cli, voice, plain, chosen, project])
      expect(await authorizeWorkspace(path, preferences)).toBe(path)
    // Another spelling of a listed folder resolves to that folder.
    expect(await authorizeWorkspace(`${cli}${sep}`, preferences)).toBe(cli)
  })

  it('never opens a folder that no project or chat names', async () => {
    const [listed, other] = await folders('listed', 'other')
    const file = join(listed, 'notes.txt')
    await writeFile(file, 'notes')
    const preferences = {
      projects: [],
      chats: [
        chat(`cli:${listed}`),
        chat(`cli:${file}`),
        // Channel and desktop sources are routes, not folders, even when they contain a path.
        chat(`telegram:personal:reply_target:${other}:thread:`),
        chat(`desktop:${other}`)
      ]
    }

    await expect(authorizeWorkspace(other, preferences)).rejects.toThrow(
      'Select this folder as a project'
    )
    await expect(authorizeWorkspace(file, preferences)).rejects.toThrow(
      'Select this folder as a project'
    )
    await expect(authorizeWorkspace(join(listed, 'missing'), preferences)).rejects.toThrow()
    await expect(authorizeWorkspace('', preferences)).rejects.toThrow('Choose a workspace first')
  })
})
