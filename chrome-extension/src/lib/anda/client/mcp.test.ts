import { describe, expect, it, vi } from 'vitest'
import type { DaemonApi } from './daemon'
import {
  McpApi,
  McpApiError,
  localEntry,
  moveCredentialsToSecrets,
  parseMcpConfig,
  remoteEntry,
  secretReferences,
  splitCommandLine,
  suggestServerId
} from './mcp'

function createDaemon(reply: unknown, overrides: Partial<DaemonApi> = {}) {
  const rpc = vi.fn(async () => reply as never)
  const daemon: DaemonApi = {
    authorized: true,
    rpc,
    toolCall: vi.fn(async () => undefined as never),
    ...overrides
  }
  return { daemon, rpc }
}

describe('McpApi', () => {
  it('sends one parameter object and unwraps the result', async () => {
    const { daemon, rpc } = createDaemon({ result: { id: 'docs', tools: [] } })
    const api = new McpApi(daemon)

    expect(await api.get('docs')).toEqual({ id: 'docs', tools: [] })
    expect(rpc).toHaveBeenCalledWith('mcp_get', [{ id: 'docs' }])
  })

  it('throws refusals with their code', async () => {
    const { daemon } = createDaemon({
      error: { code: 'not_found', message: 'MCP server docs is not configured' }
    })
    const error = await new McpApi(daemon).get('docs').catch((err) => err)

    expect(error).toBeInstanceOf(McpApiError)
    expect(error.code).toBe('not_found')
    expect(error.message).toBe('MCP server docs is not configured')
  })

  it('announces changes, and passes the revision a change was based on', async () => {
    const { daemon, rpc } = createDaemon({ result: { revision: 'r2' } })
    const api = new McpApi(daemon)
    const changed = vi.fn()
    api.addEventListener('mcp-changed', changed)

    await api.apply({ op: 'set_enabled', id: 'docs', enabled: false }, 'r1')

    expect(rpc).toHaveBeenCalledWith('mcp_apply', [
      { change: { op: 'set_enabled', id: 'docs', enabled: false }, expected_revision: 'r1' }
    ])
    expect(changed).toHaveBeenCalledTimes(1)
  })

  it('lists nothing without a token', async () => {
    const { daemon, rpc } = createDaemon({}, { authorized: false })

    expect((await new McpApi(daemon).list()).servers).toEqual([])
    expect(rpc).not.toHaveBeenCalled()
  })

  it('tests an entry with secrets that are not stored yet', async () => {
    const { daemon, rpc } = createDaemon({ result: { status: 'ready', tools: [] } })
    await new McpApi(daemon).test({ id: 'docs', url: 'https://docs.test/mcp' }, { TOKEN: 't' })

    expect(rpc).toHaveBeenCalledWith('mcp_test', [
      { server: { id: 'docs', url: 'https://docs.test/mcp' }, secrets: { TOKEN: 't' } }
    ])
  })
})

describe('pasted configuration', () => {
  it('reads whole files from other clients', () => {
    const parsed = parseMcpConfig(
      JSON.stringify({
        mcpServers: { github: { url: 'https://api.githubcopilot.com/mcp/' } },
        servers: { context7: { command: 'npx', args: ['-y', '@upstash/context7-mcp'] } }
      })
    )
    expect(parsed.servers.map((server) => server.id)).toEqual(['github', 'context7'])
    expect(parsed.servers[1].args).toEqual(['-y', '@upstash/context7-mcp'])
  })

  it('takes one entry with the id it is given', () => {
    expect(parseMcpConfig('{"url":"https://docs.test/mcp"}', 'docs').servers).toEqual([
      { id: 'docs', url: 'https://docs.test/mcp' }
    ])
    expect(parseMcpConfig('{"url":"https://docs.test/mcp"}').error).toBe('needs_id')
    expect(parseMcpConfig('{').error).toBe('invalid_json')
    expect(parseMcpConfig('{"name":"x"}').error).toBe('no_servers')
  })

  it('builds entries from the form', () => {
    expect(remoteEntry('docs', ' https://docs.test/mcp ', 'Authorization: Bearer x\nbad')).toEqual({
      id: 'docs',
      type: 'http',
      url: 'https://docs.test/mcp',
      headers: { Authorization: 'Bearer x' }
    })
    expect(localEntry('fs', 'npx -y "@scope/my server"', 'ROOT=/tmp\n')).toEqual({
      id: 'fs',
      command: 'npx',
      args: ['-y', '@scope/my server'],
      env: { ROOT: '/tmp' }
    })
    expect(splitCommandLine(`uvx 'a b' "" c`)).toEqual(['uvx', 'a b', '', 'c'])
  })

  it('suggests short ids', () => {
    expect(suggestServerId('https://api.githubcopilot.com/mcp/')).toBe('githubcopilot')
    expect(suggestServerId('https://mcp.linear.app/sse')).toBe('linear')
    expect(suggestServerId('npx -y @upstash/context7-mcp@latest')).toBe('context7')
    expect(suggestServerId('')).toBe('mcp')
  })
})

describe('secrets', () => {
  it('moves plaintext credentials into secrets and keeps references', () => {
    const { entry, secrets } = moveCredentialsToSecrets({
      id: 'git-hub',
      url: 'https://api.test/mcp',
      headers: { Authorization: 'Bearer ghp_x', 'X-Org': '${ORG}' },
      bearer_token: '${secret:KEEP}'
    })
    expect(entry.headers).toEqual({
      Authorization: '${secret:GIT_HUB_AUTHORIZATION}',
      'X-Org': '${ORG}'
    })
    expect(entry.bearer_token).toBe('${secret:KEEP}')
    expect(secrets).toEqual({ GIT_HUB_AUTHORIZATION: 'Bearer ghp_x' })

    const again = moveCredentialsToSecrets(
      { id: 'git-hub', headers: { Authorization: 'Bearer ghp_y' } },
      new Set(['GIT_HUB_AUTHORIZATION'])
    )
    expect(again.secrets).toEqual({ GIT_HUB_AUTHORIZATION_2: 'Bearer ghp_y' })
  })

  it('finds the secrets a value references', () => {
    expect(secretReferences('Bearer ${secret:A} ${secret:B:-x} ${C} ${secret:A}')).toEqual([
      'A',
      'B'
    ])
  })
})
