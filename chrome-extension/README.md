# Anda Bot Chrome Extension

This Chrome extension is the Svelte + TypeScript side panel client for Anda Bot. It talks to the local Anda daemon through a single WebSocket RPC connection per browser profile and exposes browser tabs to the agent through the split Chrome browser tools.

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

When a request starts from this Side Panel, Anda receives request metadata with a stable `browser_session`. The session stays the same as you switch tabs, while the current tab id, URL, and title are sent as metadata. The service worker refreshes that metadata as tabs are activated or updated.

The agent can use the split browser tools below. Page, input, and script tools intentionally target the active tab; use `chrome_tabs.switch_tab` first when another tab is needed. The legacy `chrome_browser` tool remains available for older prompts.

`chrome_tabs` actions:

- `get_current_tab`
- `list_tabs`
- `switch_tab`
- `open_tab`
- `close_tab`
- `navigate`
- `go_back`
- `go_forward`
- `reload`
- `launch_browser`

`chrome_page` actions:

- `snapshot`
- `extract_text`
- `get_full_page_html`
- `get_structured_data`
- `get_element_info`
- `get_viewport_size`
- `wait_for_element`
- `find_in_page`
- `screenshot`
- `read_selection`

`chrome_input` actions:

- `click`
- `type_text`
- `press_key`
- `scroll`
- `scroll_to`
- `hover`
- `drag_and_drop`
- `select_dropdown`
- `copy_to_clipboard`

`chrome_script` actions:

- `execute_javascript`

`execute_javascript` accepts either a JavaScript expression or a function body. Bare expressions such as `document.title` return automatically; multi-statement code should use `return`.

Chrome blocks extension scripts on some protected pages such as `chrome://` URLs and the Chrome Web Store.
