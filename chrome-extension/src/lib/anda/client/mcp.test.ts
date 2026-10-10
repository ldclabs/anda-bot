import { describe, expect, it, vi } from 'vitest'
import type { DaemonApi } from './daemon'
import {
  McpApi,
  McpApiError,
  defaultRegistryChoice,
  eventArgumentFields,
  eventArguments,
  isCredential,
  localEntry,
  moveCredentialsToSecrets,
  parseMcpConfig,
  registryChoices,
  registryEntry,
  registryServerId,
  remoteEntry,
  secretReferences,
  splitCommandLine,
  suggestServerId
} from './mcp'
import type { McpRegistryServer } from './types'

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

  it('scans, imports and searches with one parameter object each', async () => {
    const { daemon, rpc } = createDaemon({ result: { revision: 'r2', imported: ['docs'] } })
    const api = new McpApi(daemon)
    const changed = vi.fn()
    api.addEventListener('mcp-changed', changed)

    await api.importScan(['cursor'])
    await api.import({ items: [{ key: 'cursor:/c#docs' }], secrets: { TOKEN: 't' } })
    await api.registrySearch('docs')
    await api.registrySearch('docs', 'next')

    expect(rpc.mock.calls).toEqual([
      ['mcp_import_scan', [{ sources: ['cursor'], workspaces: [] }]],
      ['mcp_import', [{ items: [{ key: 'cursor:/c#docs' }], secrets: { TOKEN: 't' } }]],
      ['mcp_registry_search', [{ query: 'docs' }]],
      ['mcp_registry_search', [{ query: 'docs', cursor: 'next' }]]
    ])
    expect(changed).toHaveBeenCalledTimes(1)
  })

  it('reads events and changes automations with one parameter object each', async () => {
    const { daemon, rpc } = createDaemon({ result: { id: 3 } })
    const api = new McpApi(daemon)
    const changed = vi.fn()
    api.addEventListener('mcp-changed', changed)

    await api.events('github')
    await api.triggers()
    await api.triggers('github')
    await api.trigger(3)
    await api.applyTrigger({ op: 'set_enabled', id: 3, enabled: false })

    expect(rpc.mock.calls).toEqual([
      ['mcp_events_list', [{ id: 'github' }]],
      ['mcp_triggers_list', [{}]],
      ['mcp_triggers_list', [{ server_id: 'github' }]],
      ['mcp_trigger_get', [{ id: 3 }]],
      ['mcp_trigger_apply', [{ change: { op: 'set_enabled', id: 3, enabled: false } }]]
    ])
    expect(changed).toHaveBeenCalledTimes(1)
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

  it("reads other clients' spellings as Anda writes them", () => {
    const parsed = parseMcpConfig(
      JSON.stringify({
        servers: {
          wiki: {
            serverUrl: 'https://wiki.test/mcp',
            headers: { Authorization: 'Bearer ${input:wiki-token}' }
          },
          search: {
            type: 'local',
            command: 'search-mcp',
            args: ['${env:HOME}/notes'],
            env: { KEY: '${env:KEY:-x}' }
          }
        }
      })
    )
    expect(parsed.servers).toEqual([
      {
        id: 'wiki',
        url: 'https://wiki.test/mcp',
        headers: { Authorization: 'Bearer ${secret:WIKI_TOKEN}' }
      },
      {
        id: 'search',
        type: 'stdio',
        command: 'search-mcp',
        args: ['${HOME}/notes'],
        env: { KEY: '${KEY:-x}' }
      }
    ])
    expect(parseMcpConfig('{"serverUrl":"https://wiki.test/mcp"}', 'wiki').servers[0].url).toBe(
      'https://wiki.test/mcp'
    )
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

  it('moves only env values that are credentials, and never a reference', () => {
    const { entry, secrets } = moveCredentialsToSecrets({
      id: 'db',
      command: 'db-mcp',
      env: {
        DB_PASSWORD: 'pw',
        DATABASE_URL: 'postgres://app:pw@db/app',
        MEMORY_FILE_PATH: '/tmp/memory.json',
        API_KEY: 'Bearer ${TOKEN}'
      }
    })
    expect(entry.env).toEqual({
      DB_PASSWORD: '${secret:DB_DB_PASSWORD}',
      DATABASE_URL: '${secret:DB_DATABASE_URL}',
      MEMORY_FILE_PATH: '/tmp/memory.json',
      API_KEY: 'Bearer ${TOKEN}'
    })
    expect(Object.keys(secrets)).toEqual(['DB_DB_PASSWORD', 'DB_DATABASE_URL'])
    expect(isCredential('GIT_AUTHOR_NAME', 'Ada')).toBe(false)
    expect(isCredential('OPENAI_API_KEY', 'sk')).toBe(true)
  })

  it('finds the secrets a value references', () => {
    expect(secretReferences('Bearer ${secret:A} ${secret:B:-x} ${C} ${secret:A}')).toEqual([
      'A',
      'B'
    ])
  })
})

// Shapes as the MCP Registry publishes them.
const context7: McpRegistryServer = {
  name: 'io.github.upstash/context7',
  title: 'Context7',
  version: '4.3.0',
  packages: [
    {
      registryType: 'npm',
      identifier: '@upstash/context7-mcp',
      version: '4.3.0',
      transport: { type: 'stdio' },
      environmentVariables: [
        { name: 'CONTEXT7_API_KEY', description: 'API key for authentication', isSecret: true }
      ]
    },
    {
      registryType: 'mcpb',
      identifier: 'https://github.com/upstash/context7/releases/download/mcpb-v4.3.0/context7.mcpb',
      version: '4.3.0',
      fileSha256: 'e351',
      transport: { type: 'stdio' }
    }
  ],
  remotes: [
    {
      type: 'streamable-http',
      url: 'https://mcp.context7.com/mcp',
      headers: [{ name: 'Authorization', isSecret: true, description: 'API key' }]
    }
  ]
}

describe('MCP Registry installs', () => {
  it('offers remotes first and says why a package cannot be installed', () => {
    const choices = registryChoices(context7)
    expect(choices.map((choice) => [choice.kind, choice.type, choice.unsupported])).toEqual([
      ['remote', 'remote', undefined],
      ['package', 'npm', undefined],
      ['package', 'mcpb', 'package']
    ])
    expect(defaultRegistryChoice(choices)).toBe(choices[0])
    expect(choices[0].fields).toEqual([
      {
        key: 'header:Authorization',
        name: 'Authorization',
        description: 'API key',
        required: false,
        secret: true,
        default: undefined,
        choices: undefined
      }
    ])
    expect(registryServerId(context7.name)).toBe('context7')
    expect(registryServerId('com.notion/mcp')).toBe('notion')
    expect(registryServerId('io.github.bytedance/mcp-server-filesystem')).toBe('filesystem')
  })

  it('builds a remote entry with its secrets stored apart', () => {
    const [remote] = registryChoices(context7)
    const built = registryEntry(
      context7,
      remote,
      'context7',
      { 'header:Authorization': ' ctx-key ' },
      new Set(['CONTEXT7_AUTHORIZATION'])
    )
    expect(built.entry).toEqual({
      id: 'context7',
      type: 'http',
      url: 'https://mcp.context7.com/mcp',
      headers: { Authorization: '${secret:CONTEXT7_AUTHORIZATION_2}' }
    })
    expect(built.secrets).toEqual({ CONTEXT7_AUTHORIZATION_2: 'ctx-key' })
  })

  it('fills header templates and URL variables', () => {
    const server: McpRegistryServer = {
      name: 'ai.smithery/github',
      remotes: [
        {
          type: 'streamable-http',
          url: 'https://{tenant}.example.com/mcp',
          variables: { tenant: { description: 'Your tenant', isRequired: true } },
          headers: [
            { name: 'Authorization', value: 'Bearer {api_key}', isSecret: true, isRequired: true },
            { name: 'X-Client', value: 'anda' }
          ]
        }
      ]
    }
    const [remote] = registryChoices(server)
    expect(remote.fields.map((field) => [field.key, field.required, field.secret])).toEqual([
      ['url:tenant', true, false],
      ['header:Authorization:api_key', true, true]
    ])
    expect(registryEntry(server, remote, 'github', {}).missing).toEqual(['tenant', 'api_key'])
    const built = registryEntry(server, remote, 'github', {
      'url:tenant': 'acme',
      'header:Authorization:api_key': 'k1'
    })
    expect(built.entry).toEqual({
      id: 'github',
      type: 'http',
      url: 'https://acme.example.com/mcp',
      headers: { Authorization: 'Bearer ${secret:GITHUB_API_KEY}', 'X-Client': 'anda' }
    })
  })

  it('runs packages with their runner and arguments, without the whole environment', () => {
    const npm = registryChoices(context7)[1]
    expect(registryEntry(context7, npm, 'context7', { 'env:CONTEXT7_API_KEY': 'k' }).entry).toEqual(
      {
        id: 'context7',
        command: 'npx',
        args: ['-y', '@upstash/context7-mcp@4.3.0'],
        env: { CONTEXT7_API_KEY: '${secret:CONTEXT7_CONTEXT7_API_KEY}' },
        inherit_env: false
      }
    )

    const server: McpRegistryServer = {
      name: 'com.example/files',
      packages: [
        {
          registryType: 'pypi',
          identifier: 'files-mcp',
          version: '0.1.3',
          runtimeHint: 'uvx',
          transport: { type: 'stdio' },
          packageArguments: [
            { type: 'named', name: 'allowed-directories', isRequired: true },
            { type: 'named', name: '--read-only', format: 'boolean', default: 'true' },
            { type: 'positional', valueHint: 'root', default: '/srv' }
          ]
        },
        {
          registryType: 'oci',
          identifier: 'ghcr.io/example/files:1.0.2',
          transport: { type: 'stdio' },
          environmentVariables: [{ name: 'FILES_ROOT', default: '/data' }]
        },
        {
          registryType: 'npm',
          identifier: 'files-http',
          transport: { type: 'streamable-http', url: 'http://127.0.0.1:{port}/mcp' }
        }
      ]
    }
    const [pypi, oci, http] = registryChoices(server)
    expect(http.unsupported).toBe('transport')
    expect(registryEntry(server, pypi, 'files', {}).missing).toEqual(['allowed-directories'])
    expect(registryEntry(server, pypi, 'files', { 'arg:package:0': '/a,/b' }).entry).toEqual({
      id: 'files',
      command: 'uvx',
      args: ['files-mcp==0.1.3', '--allowed-directories', '/a,/b', '--read-only', '/srv'],
      inherit_env: false
    })
    expect(registryEntry(server, oci, 'files', {}).entry).toEqual({
      id: 'files',
      command: 'docker',
      args: ['run', '-i', '--rm', '-e', 'FILES_ROOT', 'ghcr.io/example/files:1.0.2'],
      env: { FILES_ROOT: '/data' },
      inherit_env: false
    })
  })
})

describe('event automation arguments', () => {
  const schema = {
    type: 'object',
    properties: {
      repo: { type: 'string', description: 'owner/name' },
      state: { enum: ['open', 'closed'] },
      limit: { type: 'integer' },
      drafts: { type: 'boolean' }
    },
    required: ['repo']
  }

  it('turns a flat schema into fields', () => {
    expect(eventArgumentFields(schema)).toEqual([
      { name: 'repo', type: 'string', required: true, description: 'owner/name' },
      { name: 'state', type: 'string', required: false, options: ['open', 'closed'] },
      { name: 'limit', type: 'integer', required: false },
      { name: 'drafts', type: 'boolean', required: false }
    ])
    expect(eventArgumentFields({ type: 'object' })).toEqual([])
  })

  it('leaves nested or mixed schemas to JSON', () => {
    expect(eventArgumentFields({ properties: { filter: { type: 'object' } } })).toBeNull()
    expect(eventArgumentFields({ properties: { ids: { type: 'array' } } })).toBeNull()
    expect(eventArgumentFields({ properties: { n: { enum: [1, 2] } } })).toBeNull()
    expect(eventArgumentFields({ type: 'string' })).toBeNull()
    expect(eventArgumentFields(null)).toBeNull()
  })

  it('parses values and names the field that is wrong', () => {
    const fields = eventArgumentFields(schema)!
    expect(
      eventArguments(fields, { repo: ' o/r ', limit: '5', drafts: 'false', state: '' })
    ).toEqual({ repo: 'o/r', limit: 5, drafts: false })
    expect(() => eventArguments(fields, {})).toThrow('repo')
    expect(() => eventArguments(fields, { repo: 'o/r', limit: '1.5' })).toThrow('limit')
  })
})
