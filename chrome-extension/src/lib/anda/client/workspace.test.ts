import { describe, expect, it } from 'vitest'
import {
  isAbsoluteWorkspacePath,
  normalizeAbsoluteWorkspace,
  normalizeWorkspaceChannelSource,
  workspaceFromCliSource
} from './workspace'

describe('isAbsoluteWorkspacePath', () => {
  it('accepts POSIX, Windows drive, and UNC paths', () => {
    expect(isAbsoluteWorkspacePath('/srv/app')).toBe(true)
    expect(isAbsoluteWorkspacePath('C:\\app')).toBe(true)
    expect(isAbsoluteWorkspacePath('c:/app')).toBe(true)
    expect(isAbsoluteWorkspacePath('\\\\host\\share')).toBe(true)
  })

  it('rejects relative and empty paths', () => {
    expect(isAbsoluteWorkspacePath('app')).toBe(false)
    expect(isAbsoluteWorkspacePath('./app')).toBe(false)
    expect(isAbsoluteWorkspacePath('~/app')).toBe(false)
    expect(isAbsoluteWorkspacePath('')).toBe(false)
  })
})

describe('normalizeAbsoluteWorkspace', () => {
  it('trims whitespace and trailing separators', () => {
    expect(normalizeAbsoluteWorkspace('  /srv/app/  ')).toBe('/srv/app')
    expect(normalizeAbsoluteWorkspace('/srv/app///')).toBe('/srv/app')
    expect(normalizeAbsoluteWorkspace('C:\\app\\')).toBe('C:\\app')
  })

  it('keeps a bare root intact', () => {
    expect(normalizeAbsoluteWorkspace('/')).toBe('/')
    expect(normalizeAbsoluteWorkspace('C:\\')).toBe('C:\\')
    expect(normalizeAbsoluteWorkspace('C:/')).toBe('C:/')
  })

  it('answers empty for anything not absolute', () => {
    expect(normalizeAbsoluteWorkspace('app')).toBe('')
    expect(normalizeAbsoluteWorkspace(null)).toBe('')
    expect(normalizeAbsoluteWorkspace(undefined)).toBe('')
    expect(normalizeAbsoluteWorkspace(42)).toBe('')
  })
})

describe('workspaceFromCliSource', () => {
  it('reads the workspace from both CLI source shapes', () => {
    expect(workspaceFromCliSource('cli:/srv/app')).toBe('/srv/app')
    expect(workspaceFromCliSource('cli:voice:/srv/app/')).toBe('/srv/app')
    expect(workspaceFromCliSource('cli: /srv/app ')).toBe('/srv/app')
  })

  it('answers empty for other channels or relative paths', () => {
    expect(workspaceFromCliSource('browser:chrome:1700000000000')).toBe('')
    expect(workspaceFromCliSource('cli:app')).toBe('')
    expect(workspaceFromCliSource('')).toBe('')
  })
})

describe('normalizeWorkspaceChannelSource', () => {
  it('re-normalizes the path while preserving the prefix', () => {
    expect(normalizeWorkspaceChannelSource('  cli:/srv/app//  ')).toBe('cli:/srv/app')
    expect(normalizeWorkspaceChannelSource('cli:voice:/srv/app/')).toBe('cli:voice:/srv/app')
  })

  it('rejects a source whose workspace is unusable', () => {
    expect(normalizeWorkspaceChannelSource('cli:app')).toBe('')
    expect(normalizeWorkspaceChannelSource('cli:voice:app')).toBe('')
    expect(normalizeWorkspaceChannelSource('browser:chrome:1')).toBe('')
  })
})
