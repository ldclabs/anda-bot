---
name: mcp-builder
description: Implement and test a Model Context Protocol server that exposes an API or local service to Anda Bot. Covers tool design, stdio and Streamable HTTP, and verifying discovery in Anda.
license: Complete terms in LICENSE.txt
compatibility: Python or Node.js and the matching MCP SDK. Live integration checks require an Anda daemon and the server's test credentials when applicable.
---

# Build an MCP server for Anda

Start with the operations the user needs, the data they may access and the expected results. Inspect the existing project and lockfile. Use stdio for a local process or Streamable HTTP for a service endpoint. On stdio, send diagnostics to stderr so stdout carries only protocol messages.

## Implement the tools

Read relevant package files through `skills_read` with `skill` set to `mcp-builder` and the relative `resource` path. Follow `next_cursor` until complete. Match examples to the SDK version in the project.

- [Tool and transport design](reference/mcp_best_practices.md)
- [Python SDK](reference/python_mcp_server.md)
- [TypeScript SDK v1](reference/node_mcp_server.md) and [TypeScript SDK v2](reference/node_mcp_server_v2.md)

Use action-oriented names and precise descriptions. Distinguish read-only and mutating operations in their annotations. Narrow the service credentials to the operations requested. For large results, return a bounded page and a continuation token.

Inspect the actual schema returned by `tools/list`. For Anda's model-facing inputs and structured outputs, use a single object, declare its properties and required fields, and set `additionalProperties` to `false` for strict objects. Represent optional inputs with required nullable fields, e.g. `"type": ["string", "null"]`. Express alternatives with an enum discriminator and validate the selected fields in the handler. Check for `oneOf`, `anyOf` and `allOf` anywhere in the emitted schema, including SDK-generated nested definitions.

## Verify the integration

Build the server and exercise initialization, tool listing, one representative read and an invalid request with an SDK client or MCP Inspector. Check error responses and pagination. Use a fixture or test account when exercising a write.

If the user wants the server registered in Anda, use an absolute executable or entrypoint path that the daemon can resolve and register the intended transport. For example:

```sh
anda mcp add example -- node /absolute/path/to/dist/index.js
# For a remote server instead:
anda mcp add example-remote --url https://example.com/mcp
anda mcp tools example
```

In chat, `tools_groups` and `tools_search` expose the tools that the caller may use. Verify the exact registered name before calling it. Keep credentials in the configured secret mechanism.

For repeated task-level evaluations, use realistic read-only questions with known answers from a stable fixture. The [evaluation guide](reference/evaluation.md) includes an optional Anthropic-specific runner that sends questions and tool results to that provider. Ordinary Anda integration checks use the configured Anda model and the MCP client.
