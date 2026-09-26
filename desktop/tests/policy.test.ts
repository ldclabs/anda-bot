import { resolve, join } from 'node:path'
import { describe, expect, it } from 'vitest'
import {
  appPermissionAllowed,
  externalUrl,
  loopbackBaseUrl,
  navigationSource,
  validateRpc,
  rendererAssetPath
} from '../src/main/policy'
import { desktopMessages } from '../src/renderer/labels'

it('permits application audio only in its own main frame, never camera or preview frames', () => {
  expect(appPermissionAllowed(1, 1, 'media', { isMainFrame: true, mediaType: 'audio' })).toBe(true)
  expect(appPermissionAllowed(1, 1, 'media', { isMainFrame: true, mediaType: 'video' })).toBe(false)
  expect(appPermissionAllowed(1, 1, 'media', { isMainFrame: false, mediaType: 'audio' })).toBe(
    false
  )
  expect(appPermissionAllowed(1, 2, 'media', { isMainFrame: true, mediaType: 'audio' })).toBe(false)
  expect(appPermissionAllowed(1, 1, 'media', { isMainFrame: true })).toBe(false)
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
  })
  it('limits deep links to navigation and external links to safe protocols', () => {
    expect(navigationSource('anda://chat?source=desktop%3Aone')).toBe('desktop:one')
    expect(navigationSource('anda://exec?command=rm')).toBeNull()
    expect(() => externalUrl('file:///etc/passwd')).toThrow()
    expect(() => externalUrl('javascript:alert(1)')).toThrow()
    expect(externalUrl('https://anda.bot')).toBe('https://anda.bot/')
  })
})
