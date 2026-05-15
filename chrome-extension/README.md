# Anda Bot Chrome Extension

This Chrome extension is the Svelte + TypeScript side panel client for Anda Bot. It talks to the local Anda daemon through a single WebSocket RPC connection per browser profile and exposes browser tabs to the agent through the `chrome_browser` tool.

## Setup

1. Start Anda once so `~/.anda/keys/user.key` exists:

```bash
cargo run -p anda_bot --
```

2. Generate an extension token:

```bash
cargo run -p anda_bot -- browser token --days 30
```

3. Build the extension:

```bash
pnpm --filter anda-bot-chrome-extension build
```

4. Open `chrome://extensions`, enable Developer mode, choose Load unpacked, and select `chrome-extension/dist`.

5. Open the Anda Bot side panel, paste the printed Gateway URL and Bearer token, then save.

Chrome 116 or newer is required because the extension keeps its Manifest V3 service worker alive with a WebSocket keepalive.

## Browser Actions

When a request starts from this Side Panel, Anda receives request metadata with a stable `browser_session`. The session stays the same as you switch tabs, while the current tab id, URL, and title are sent as metadata. The agent can call `chrome_browser` to inspect pages and manage tabs. Supported actions include:

- `snapshot`
- `extract_text`
- `click`
- `type_text`
- `press_key`
- `scroll`
- `navigate`
- `screenshot`
- `read_selection`
- `list_tabs`
- `switch_tab`
- `open_tab`
- `close_tab`
- `launch_browser`

Most page actions accept `tab_id` to target a specific tab returned by `list_tabs`. Chrome blocks extension scripts on some protected pages such as `chrome://` URLs and the Chrome Web Store.
