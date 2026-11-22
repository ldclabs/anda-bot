# Anda Desktop

Electron + Svelte desktop client for the local Anda daemon. Chat messages, approval cards, attachments, memory, skills, bookmarks, configuration and voice reuse the Chrome extension's implementation. The extension remains independently buildable.

## Installation / 安装

Download `Anda-mac-arm64.dmg` or `Anda-win-x64.exe` from the latest GitHub release (or `brew install --cask ldclabs/tap/anda-desktop`). New macOS releases support Apple Silicon only; Intel Mac users can build from source. On macOS drag **Anda.app** into Applications; on Windows run the installer. Node.js, pnpm and Rust are not required to use release packages.

从最新 GitHub Release 下载 `Anda-mac-arm64.dmg` 或 `Anda-win-x64.exe`（或 `brew install --cask ldclabs/tap/anda-desktop`）。新的 macOS 发布包仅支持 Apple Silicon；Intel Mac 用户可从源码构建。macOS 将 **Anda.app** 拖入“应用程序”，Windows 运行安装程序即可，使用发布包无需安装开发工具。

### One runtime / 同一个 runtime

The package carries an `anda` binary and the curated skills, but never runs that binary as the daemon. On every start the app runs it as `anda install`, which:

- keeps an existing install at the install-script location (`~/.local/bin`, `%LOCALAPPDATA%\Programs\AndaBot` or `ANDA_INSTALL_DIR`) or from Homebrew; replaces it only when the bundled release is newer; never touches a Homebrew install (`brew upgrade anda` updates it);
- otherwise installs the bundled `anda` there, adds the directory to `PATH` and copies the curated skills;
- retires the old `anda_launcher` tray: its login entry, `~/Applications/Anda Bot.app`, Windows shortcuts and sidecar. If the launcher started at login, the daemon keeps starting at login (`anda autostart`) and this app's tray starts at login.

All daemon commands then use that shared `anda`, so the desktop, terminals and the Chrome extension talk to one daemon with one version. **Settings → Runtime** shows its path and version; *Choose anda executable* overrides it. Existing `~/.anda` configuration, identity and conversations are reused.

安装包内带有 `anda` 和精选技能，但从不直接把它作为 daemon 运行。应用每次启动都会以 `anda install` 运行它：已有安装脚本或 Homebrew 安装的 anda 会被沿用（仅在内置版本更新时替换，Homebrew 安装只通过 `brew upgrade` 更新）；没有安装时，会安装到安装脚本的位置并加入 `PATH`；同时停用旧的 `anda_launcher` 托盘（登录项、`Anda Bot.app`、Windows 快捷方式）。之后所有 daemon 命令都通过这个共享的 `anda` 执行，桌面端、终端和 Chrome 扩展连接的是同一个 daemon。

### Tray and login / 托盘与登录启动

The app's tray is Anda's only tray. It shows whether the service runs, restarts it, installs `anda` updates, copies a 30-day Chrome extension token and opens the logs. The model connection guide offers a launch-at-login option (also in **Settings → Launch at login**); a login start stays in the tray without opening a window and starts the service.

- Closing the window keeps the tray, terminals and notifications. **Quit Anda Desktop** leaves the service running, but asks before ending active user terminals. An explicit stop in Settings is remembered across restarts until you reconnect.
- On macOS, closing a fullscreen window first exits fullscreen, then hides it. Reopen it from the Dock or tray to continue with the same chats and drafts.
- Automatic reconnects never start a service that was stopped elsewhere (for example with `anda stop`); opening the app, *Reconnect* or sending a message does.
- The app mirrors its UI language to `~/.anda/launcher/ui.json`, which the daemon uses for approval cards and the Chrome extension follows.
- When a model is missing, the connection guide opens automatically. Choose ChatGPT and confirm a model, or select a preset sourced directly from `anda_bot/assets/config.yaml` and enter its API key. Saving preserves unrelated YAML and uses revision checks; completion waits for the selected model to load, without a paid probe request. Dismissing the guide is remembered per Anda home. Chat keeps a connection button and your draft; reopen it from **Settings → General → Connect a model**.
- If other configuration is incomplete, open **Settings → Agent configuration**. Offline saves are validated by the Rust parser before replacing the file; a private `config.yaml.desktop-backup` is preserved.
- Imported IM channel conversations are read-only; start a local chat to respond without changing the original sender or reply route.
- Chrome page automation continues to require the Chrome extension. Electron does not share Chrome's tabs or login state.

macOS 全屏时点击关闭按钮，会先退出全屏再隐藏窗口；从 Dock 或托盘重新打开后，可继续使用原来的聊天和草稿。关闭窗口会保留后台服务、终端和通知。

### Updates / 更新

**Check for updates** in the tray or Settings opens a desktop dialog immediately and checks both the `anda` runtime and the desktop app. It looks up the desktop feed first: one GitHub release ships both, and the desktop package carries that release's `anda`, which the app installs on start. So when the shared `anda` came from the desktop (not Homebrew or a Settings-chosen executable), a desktop release is the whole update. It is downloaded without the CLI download, and one confirmation installs it: the service drains and stops, the app restarts, and the restarted app starts the service on the new `anda`. Otherwise the runtime is checked, downloaded and offered first, then the desktop update; installing or postponing the runtime, or a runtime failure, still continues to the desktop. A check downloads what it finds; only installing asks. The dialog shows check/download/install progress and retains the results, including any errors, until dismissed, even when the main window was hidden or had not been opened. Closing the dialog leaves the operation running; repeated clicks show its current progress. The tray always keeps **Check for updates**, alongside the install actions; a downloaded runtime that the desktop release carries has no separate action.

The bottom of the sidebar shows the next step: **Download update** for a found desktop release (it downloads in place, with progress), then **Restart to update** for a downloaded release, desktop or runtime. Clicking it is the confirmation, so it restarts without another dialog and opens the update dialog for the progress, or for the reason it cannot restart yet, such as open terminals or active tasks.

点击托盘或设置中的「检查更新」会立即打开桌面弹窗，检查 `anda` runtime 和桌面客户端。它先查询桌面客户端的更新源：同一个 GitHub Release 同时发布两者，桌面安装包内置该版本的 `anda`，客户端启动时会安装它。因此，如果共享的 `anda` 由桌面端安装（而不是 Homebrew 或在设置中指定的程序），桌面版本就是完整的更新：不再单独下载 CLI，只需确认一次即可安装——服务会排空任务并停止，客户端重启后用新的 `anda` 启动服务。否则先检查、下载并提示 runtime 更新，再处理桌面客户端更新；安装或暂缓 runtime 更新、或 runtime 失败，都不会跳过桌面客户端。检查时会直接下载找到的更新，只有安装需要确认。弹窗显示检查、下载、安装进度，并保留结果或失败原因，即使主窗口隐藏或尚未打开也能看到。关闭弹窗不会中断操作；重复点击会显示当前进度。托盘始终保留「检查更新」及安装入口；桌面版本已包含的 runtime 下载不再单独显示安装入口。

侧边栏底部会显示下一步操作：发现桌面新版本时显示「下载更新」（在原位下载并显示进度），下载完成后（桌面客户端或 runtime）显示「重启以完成更新」。点击即视为确认，不再弹出确认框，并打开更新弹窗显示进度，或说明暂时无法重启的原因（例如仍有打开的终端或活动任务）。

- **The `anda` runtime** updates through its own release channel. The app checks every six hours (and on *Check for updates*), except when the desktop release found first carries it; a downloaded release shows in the tray and the sidebar. Installing it pauses new tasks, waits for active ones, stops the service, replaces `anda` and starts the service again. The service stays stopped throughout installation, even if downloads outlast the maintenance lease. A busy service keeps the download for later, and a failed install brings the service back. A failed check, for example while offline, reports its error and keeps an already downloaded release in the tray; *Check for updates* does not offer to install it then, because installing also needs the release server. Homebrew installs report that `brew upgrade anda` is needed.
- **The desktop app** checks its GitHub release feed in the background one minute after launch, then every six hours after a check completes. This runs even when the window is closed or the runtime was stopped. A new version appears in the tray and is announced once per version per app session; checks with no update or a network failure stay quiet. Downloads start from *Check for updates*, the tray or the sidebar; installation needs a confirmation or the sidebar's restart. A desktop release that carries the runtime restarts the service after its tasks drain; otherwise the service keeps running. Open terminals must be closed before installing. Release builds have a desktop update feed (on macOS only when signed); development, local and unsigned macOS builds skip these checks and are updated manually.
- Uninstalling the desktop leaves the shared `anda`, its data and its login entry in place; remove them like a script install (`anda autostart uninstall`, then delete the install directory).

`anda` runtime 通过自己的发布通道更新：托盘提示可安装时，会先暂停新任务、等待活动任务完成，停止服务后再替换 `anda` 并启动服务。即使下载超过维护租约期限，服务在整个安装期间也保持停止；安装失败时会恢复服务。检查失败（例如离线）时会报告错误，已下载的版本仍保留在托盘中；由于安装同样需要访问发布服务器，此时「检查更新」不会提示安装。桌面应用本身在正式发布版中通过 GitHub Release 更新（macOS 需已签名）；如果更新包内置的 runtime 比运行中的服务新，服务会在任务排空后随客户端重启，否则服务继续运行。卸载桌面端不会删除共享的 `anda`、数据和登录项。

桌面客户端会在启动 1 分钟后自动检查更新，之后每次检查完成后间隔 6 小时再检查；关闭窗口或停止后台服务不会停止桌面客户端的检查。发现新版本时会添加托盘更新入口，并在每次启动期间对同一版本仅提醒一次；没有更新或网络检查失败时保持安静，等待下次检查。下载可从「检查更新」、托盘或侧边栏开始；安装需要确认，或点击侧边栏的「重启以完成更新」。正式发布版带有桌面更新通道（macOS 需已签名）；开发版、本机打包和未签名的 macOS 版本不进行桌面客户端自动检查，需要手动安装新版。

## Build

Use the workspace Rust toolchain, Node.js 24 and pnpm 11.7.0 to match CI. The repository manages JavaScript dependencies through its pnpm workspace.

```bash
pnpm install --filter @anda/desktop...
pnpm --dir desktop check
pnpm --dir desktop test
pnpm --dir desktop build
pnpm --dir desktop package
```

`package` builds `anda` with `cargo build --release --locked`, copies it and the curated `skills/` into the application's resources, and creates the local platform's installers under `desktop/release/`. `package:dir` produces an unpacked application. To reuse a prebuilt runtime, set `ANDA_DESKTOP_RUNTIME` to that executable before packaging; its version and SHA-256 are recorded in a manifest. CI requires this variable and fails if it is missing instead of rebuilding Rust. Local packaging still builds the runtime when the variable is unset. Do not commit runtime binaries, installers, or test screenshots.

For development:

```bash
pnpm --dir desktop dev
```

You can select a custom home with `--anda-home=/absolute/path`; the desktop profile is isolated by home. The default Electron profile stores window bounds, navigation metadata and text drafts under the OS application-data directory. Authoritative messages and memory remain in Anda's home. Attachments in unsent drafts survive chat switches in the current window, but are not persisted to disk when the application exits; text drafts of a new chat that was never sent are dropped at the next start.

## Validation

Use `--anda-profile=/absolute/path` when an explicit, separate UI profile is needed. `scripts/packaged-smoke.mjs /path/to/Anda.app` uses temporary home/profile directories with the daemon stopped to verify the installed bundle's runtime digest, bundled skills, PTY native module and browser isolation without accessing your real identity or conversations or installing anything.

```bash
pnpm --dir desktop test:e2e
pnpm --dir desktop test:onboarding
pnpm --dir chrome-extension check
pnpm --dir chrome-extension test
pnpm --dir chrome-extension i18n
pnpm --dir chrome-extension build
RUST_MIN_STACK=16777216 cargo test -p anda_bot desktop_ -- --nocapture
RUST_MIN_STACK=16777216 cargo test -p anda_bot daemon_control_routes_serve_config_and_status -- --nocapture
RUST_MIN_STACK=16777216 cargo test -p anda_bot --test cli_validation -- --nocapture
```

The onboarding smoke test covers preset selection, configuration conflicts, draft retention, dismissal/restart, ChatGPT cancellation and activation, and non-model configuration errors. It saves screenshots under `desktop/test-results/onboarding/`.

The Electron smoke test uses a temporary profile and authenticated local mock daemon. It never opens the real Anda database or calls a paid model. Screenshots are written to `desktop/test-results/`. GUI tests need a graphical session and may need to run outside a command sandbox.

With a current runtime, `/ws/app/v1` sends caller-scoped state invalidations after persistence. The shared conversation client reads authoritative deltas on notification, coalesces changes and recovers from a new snapshot on reconnect; older runtimes retain polling. Accepted submissions get durable receipts under `~/.anda/desktop-submissions/`. A lost reply is reconciled by request ID; completed responses (including side replies) retain their local recovery references until the renderer acknowledges applying them, so a window reload during a request can recover its result; an uncertain crash window remains unknown and is never automatically replayed. This does not promise exactly-once external tool effects or token streaming.

## Chats

The sidebar filters chat titles as you type (Enter searches conversation history, as does ⌘K), keeps pinned chats in their own section, and can group the rest by date or by project from the filter button, which also shows archived chats. Each row leads with its state: working, waiting for your approval, failed or unread. Back and forward (⌘[ / ⌘], Alt+←/→ on Windows) walk the chats and pages visited in the window; Ctrl+Tab and Ctrl+Shift+Tab step through the sidebar's chats.

Goal evaluations, context compaction summaries and handoffs use the same compact, collapsed rows as tool details. Expand a row to read its full content.

目标评估、上下文压缩摘要和交接消息与工具详情一样，默认显示为紧凑的折叠行，展开后可查看完整内容。

The chat title opens a menu to rename, pin, archive, copy the conversation as Markdown or show its folder; the chip beside it names the folder and its Git branch. A new chat's folder is chosen from the menu under the composer, keeping the draft; choosing a folder from a chat that has already started opens a new chat there. Replies stay in the transcript. A single expandable row holds consecutive reasoning, tool calls and background updates and says what the work did, such as “Ran 3 commands and read 2 files”. Open it to read the reasoning or inspect individual calls. The plan sits in the transcript under the latest message, above the working line, which opens into the calls still running. Completed plans fold to one line. Scroll up to read earlier messages and use **Jump to latest** to return. Copy and bookmark are available on each reply; rich-text copy and print are in **More actions**. A pending approval or choice docks above the composer, as in the Chrome extension: a click, its number key or Enter answers a row, the last row replies in chat instead, and ⌘⇧↩ approves and ⌘⇧⌫ denies an approval (Ctrl+Shift on Windows); the transcript keeps the answer. Long chats show a mark per prompt along the transcript's edge.

## Settings and management / 设置与管理

Agent configuration keeps edits in memory while you visit other pages in the same window. Save and Discard changes are available below every section. In a compact window, open the list above a management page to choose a different item.

在同一窗口切换页面时，「Agent 配置」会保留草稿。每个配置分区底部都可以保存或放弃更改。在窄窗口中，打开管理页面上方的列表即可选择其他项目。

## Workbench

Open panels from the chat header's **Resources**, **Changes**, **Terminal** and **Browser** buttons, or with ⌘⇧A, ⌘⇧G, ⌘⇧T and ⌘⇧B (Ctrl+Shift on Windows and Linux; the View menu lists every shortcut); a second press closes one. Panels share a canvas with the chat and stay open together. The first opens beside the chat and the next ones stack in its column. Panels float on the canvas as cards. Drag one by the grab bar along its top, or by its title, onto another panel or the chat: near an edge it splits that pane on that side, in the middle the two swap places, and dropped outside the window it opens in a window of its own. Drag the line between two panes to resize them; a double-click shares the room evenly. The layout button in a panel's header moves it along an edge of the canvas (left, right, top or bottom), fills the canvas with it, or opens it in its own window. A panel's own window follows the chat open in the main window, and its layout menu puts it back on the canvas. The arrangement is kept across restarts, and so is which panels open in their own windows. At 900px wide and below, panels open one at a time as a drawer over the chat. Changes and Terminal open only the chat's own folder: a project, the folder a new chat was started in, or the folder a chat started from the `anda` terminal ran in.

- **Resources** lists the chat's attachments. A file path a reply puts in inline code (`src/app.ts:42`) opens a read-only preview here, at that line; previews stay inside the chat's folder, read at most 512 KB and never open the file in another application (*Show in folder* reveals it instead).
- **Changes** requires the selected project to be a Git repository root. View staged/working-tree changes with line counts and coloured diffs, and history; stage, unstage and commit from explicit controls. *This chat* narrows the list to files the agent edited in the chat, and each finished turn that edited files ends with a card linking to their diffs. Operations reject a changed repository snapshot. Git hooks, helpers and repository filters run with your user permissions; use repositories you trust.
- **Worktrees** creates branches in desktop-managed directories. Archiving another managed worktree saves a snapshot first, then removes its checkout; restore recreates its contents at the original path on a detached snapshot commit. It preserves content rather than the original staging layout. Ignored files, submodules and nested repositories must be handled separately before archiving. Stop other processes using the checkout before confirming archive. Your primary or externally managed worktrees cannot be archived here.
- **Terminal** runs your shell in the selected project; the panel header's + starts a session and ⌘F (Ctrl+Shift+F) finds in its output. Its colours follow the app theme. Sessions survive panel and chat switches; quitting ends them after confirmation. Output and scrollback are bounded. These are user terminals; agent commands continue through the daemon's existing approval rules.
- **Browser** has its own persistent login profile and per-chat tabs. Multiple chats keep their browser sessions registered concurrently, including after reconnect. It does not import Chrome cookies or install Chrome extensions. With the new runtime, that chat's agent uses its own desktop browser session through the existing browser tools, and the panel opens when the agent opens a page in the foreground. The address bar accepts HTTP/HTTPS URLs; local attachments use Resources. The agent can also open a local file here (`open_file`, not folders): that page may load only the file itself and the stylesheets, scripts, images, fonts and media in its folder or below, never other local files. Remote pages have no application bridge or Node access. Website microphone access requires explicit permission; camera access stays disabled. Agent file uploads require local confirmation; downloads use a save dialog. Browser tabs currently last for the desktop process lifetime; website cookies persist.
- **Settings → Audio** selects devices for a recording/playback self-test, displays an input meter and tests configured transcription/TTS providers. Stop cancels playback and suppresses late test results. Physical microphones, Bluetooth routing, audio quality and real provider behavior still need device testing.

The Automations editor loads the complete saved task before editing; task previews in the list never replace the full prompt or shell command.

Normal chat speech can also be stopped from the composer. Input drafts are preserved independently of browser, terminal and Git panels.

## Release

`.github/workflows/release.yml` builds the desktop for macOS arm64 and Windows x64 from the same release's `anda` binaries and attaches `Anda-mac-arm64.dmg/.zip`, `Anda-win-x64.exe`, blockmaps, checksums and update metadata. The desktop job installs only desktop/Chrome extension dependencies and reuses the downloaded runtime through the job-wide `ANDA_DESKTOP_RUNTIME`; it never compiles Rust. For the macOS bridge, both `latest-mac.yml` and `stable-mac.yml` describe the same signed ZIP. `scripts/publish-homebrew.sh` reads the release's local checksums and publishes the Apple Silicon `anda-desktop` cask, which depends on the `anda` formula; the formula supports macOS arm64 and Linux arm64/x86_64.

`.github/workflows/desktop.yml` checks the desktop app and the Chrome extension on macOS and Windows for pull requests and pushes to `main`: type checks, unit tests, native Electron tests, an unsigned local package and NSIS install/uninstall checks. That package bundles an unoptimized `anda` (`cargo build --profile ci`, passed through `ANDA_DESKTOP_RUNTIME`) to keep the job short, so the workflow uploads only the Electron test screenshots, never the installers; the Rust tests run in `test.yml`.

### macOS signing bridge (v0.15.0)

Keep the LDC Developer ID Application certificate and its export password in `MAC_CSC_LINK` and `MAC_CSC_KEY_PASSWORD` in the protected `desktop-release` environment. Use the LDC notarization credentials (`APPLE_ID`, `APPLE_APP_SPECIFIC_PASSWORD` and `APPLE_TEAM_ID=S9Z2WAMAV8`). Release packaging requires these credentials. The separate Desktop CI and ordinary local builds continue to use ad hoc signing.

The release signs the outer app (`org.ldclabs.anda.desktop`) and runtime (`ai.anda.anda-bot`) with LDC. Each embedded designated requirement pins its own identifier and an Apple Developer ID Application certificate from LDC or Yiwen (`63G8VNQPXB`). Electron helpers and the retired launcher keep their component-specific signatures. The desktop embeds the exact separately signed and notarized CLI and records its SHA-256 in the runtime manifest. Standalone CLI updates follow their existing release checksum checks. The release job checks both signatures, both designated requirements, the runtime bytes and the stapled ticket in the final update ZIP.

1. Run **Release** manually on the branch to obtain signed artifacts. On a test Mac, compare the installed LDC app's requirement with the extracted bridge using the commands below. Check its identity and microphone with a test profile.
2. Publish `v0.15.0` as the stable GitHub Latest release. Existing Macs read `latest-mac.yml`. The installed bridge reads `stable-mac.yml`. Ship both manifests and keep the signed DMG, ZIP and blockmap available. [Bridge release](https://github.com/ldclabs/anda-bot/releases/tag/v0.15.0)
3. At the subsequent Yiwen cutover, replace the bridge-only signing policy with Yiwen-only signing. In the new GitHub Latest release, point `stable-mac.yml` to the Yiwen ZIP. Keep `latest-mac.yml` at version `0.15.0` for Macs that missed the bridge. Attach the original LDC bridge ZIP and matching blockmap under a distinct name in that release. Use that filename and the original size and SHA-512 in the legacy manifest. Continue serving both feeds while supporting pre-bridge clients. This lets an older Mac take both updates in order. Windows and standalone CLI updates keep following GitHub Latest.
4. Test both hops on a test Mac. Confirm that the original Anda identity and microphone work. Existing Keychain items retain their access controls, and macOS may ask for permission again. If an automatic update fails, install the original bridge DMG over the app and check for updates again. Keep the existing Anda home and keys.

```sh
node scripts/macos-bridge.cjs verify-package /path/to/bridge/Anda.app /path/to/anda-macos-arm64
previous_requirement=$(codesign -d -r- /Applications/Anda.app 2>&1 | sed -n 's/^designated => //p')
test -n "$previous_requirement" && codesign --verify --deep --strict -R "=$previous_requirement" /path/to/bridge/Anda.app
```

On 2026-11-20 the designated requirements from an installed v0.14.10 app and its runtime accepted the local LDC-signed arm64 bridge app and CLI. The two update manifests described the same ZIP, and the embedded CLI matched the standalone binary. This local run used a development Rust build and omitted Apple notarization. An installed update, a Yiwen-signed successor, Keychain access and microphone permission remain release acceptance checks.

Windows signing uses `WIN_CSC_LINK`, `WIN_CSC_KEY_PASSWORD` and the `ANDA_WINDOWS_PUBLISHER` variable. Without `WIN_CSC_LINK` the Windows package is unsigned but still uses the release configuration and its update feed: `app-update.yml` then has no `publisherName`, so electron-updater skips the signature check and verifies the download against the sha512 in the release's `latest.yml`, the same trust as `anda update` and `scripts/install.ps1`. A browser-downloaded unsigned installer can trigger SmartScreen on first install; in-app updates do not, because electron-updater's download carries no Mark of the Web.

The NSIS installer only stops processes started from its own directory (`resources/installer.nsh`): the default electron-builder check matches `$INSTDIR` as a bare prefix, and `...\Programs\Anda` is a prefix of `...\Programs\AndaBot`, where the shared `anda` and its daemon run.

App protocol types are generated from Rust and checked in tests:

```bash
ANDA_EXPORT_PROTOCOL=1 RUST_MIN_STACK=16777216 cargo test -p anda_bot desktop_protocol_types --bin anda
```

Windows build configuration is included, but macOS testing does not substitute for Windows signing, installation, notifications and audio-device validation. Microphone/TTS need configured providers and OS permission; automated tests do not validate physical audio devices or printers.
