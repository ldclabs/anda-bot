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

When a request starts from this Side Panel, Anda receives request metadata with a stable `browser_session`. The session stays the same as you switch tabs, while the current tab id, URL, and title are sent as metadata. The service worker refreshes that metadata as tabs are activated, updated, or navigated through `webNavigation` events.

The agent can use the split browser tools below. Page, input, and script tools intentionally target the active tab; use `chrome_tabs.switch_tab` first when another tab is needed. The public schemas expose the common browser actions while keeping lower-level browser-state handlers such as cookies and cache internal for compatibility.

`chrome_tabs` actions:

- `get_current_tab`
- `list_tabs`
- `switch_tab`
- `open_tab`
- `open_file`
- `close_tab`
- `navigate`
- `wait_for_navigation`
- `wait_for_history_change`
- `get_frames`
- `go_back`
- `go_forward`
- `reload`
- `launch_browser`
- `download`
- `list_downloads`
- `cancel_download`
- `open_download`

`chrome_page` actions:

- `snapshot`
- `extract_text`
- `screenshot`
- `read_selection`
- `get_full_page_html`
- `get_structured_data`
- `get_accessibility_tree`
- `print_to_pdf`
- `annotate_viewport`
- `clear_annotations`
- `get_element_info`
- `get_viewport_size`
- `find_in_page`
- `wait_for_element`
- `handle_dialog`

`chrome_input` actions:

- `click`
- `type_text`
- `press_key`
- `scroll`
- `scroll_to`
- `hover`
- `drag_and_drop`
- `select_dropdown`
- `upload_file`
- `copy_to_clipboard`

`chrome_script` actions:

- `execute_javascript`

`execute_javascript` accepts either a JavaScript expression or a function body. Bare expressions such as `document.title` return automatically; multi-statement code should use `return`. By default it uses a CSP-resistant debugger bridge so it can evaluate in the page context even on sites with strict CSP.

Chrome blocks extension scripts on some protected pages such as `chrome://` URLs and the Chrome Web Store.
