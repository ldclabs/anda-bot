# Anda Bot Chrome Extension

This Chrome extension is the Svelte + TypeScript side panel client for Anda Bot. It talks to the local Anda daemon through a WebSocket RPC connection and exposes the current tab to the agent through the `chrome_browser` tool.

## Setup

1. Start Anda once so `~/.anda/keys/user.key` exists:

```bash
cargo run -p anda_bot --
```

2. Generate an extension token:

```bash
cargo run -p anda_bot -- chrome token --days 30
```

3. Build the extension:

```bash
pnpm --filter anda-bot-chrome-extension build
```

4. Open `chrome://extensions`, enable Developer mode, choose Load unpacked, and select `chrome-extension/dist`.

5. Open the Anda Bot side panel, paste the printed Gateway URL and Bearer token, then save.

Chrome 116 or newer is required because the extension keeps its Manifest V3 service worker alive with a WebSocket keepalive.

## Browser Actions

When a request starts from this Side Panel, Anda receives request metadata with a `browser_session` and can call `chrome_browser` to inspect and operate the active tab. Supported actions include:

- `snapshot`
- `extract_text`
- `click`
- `type_text`
- `press_key`
- `scroll`
- `navigate`
- `screenshot`
- `read_selection`

Chrome blocks extension scripts on some protected pages such as `chrome://` URLs and the Chrome Web Store.
