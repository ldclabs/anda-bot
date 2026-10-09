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

**Check for updates** in the tray or Settings opens a desktop dialog immediately and checks both the `anda` runtime and the desktop app. After installing or postponing a runtime update, it continues to the desktop update; a runtime check or installation failure also leaves the desktop update available. The desktop feed is queried while the runtime check runs, so a runtime download does not delay it; the updates are still offered one at a time. The dialog shows check/download/install progress and retains both results, including any errors, until dismissed, even when the main window was hidden or had not been opened. Closing the dialog leaves the operation running; repeated clicks show its current progress. The tray always keeps **Check for updates**, alongside any component-specific install actions.

点击托盘或设置中的「检查更新」会立即打开桌面弹窗，同时检查 `anda` runtime 和桌面客户端（runtime 下载不会拖慢桌面客户端的检查），再依次处理两者的更新。安装或暂缓 runtime 更新后会继续处理桌面客户端更新；runtime 检查或安装失败也不会跳过桌面客户端。弹窗显示检查、下载、安装进度，并保留两个组件的结果或失败原因，即使主窗口隐藏或尚未打开也能看到。关闭弹窗不会中断操作；重复点击会显示当前进度。托盘始终保留「检查更新」，并单独显示各组件可用的安装入口。

- **The `anda` runtime** updates through its own release channel. The app checks every six hours (and on *Check for updates*); a downloaded release shows in the tray. Installing it pauses new tasks, waits for active ones, stops the service, replaces `anda` and starts the service again. The service stays stopped throughout installation, even if downloads outlast the maintenance lease. A busy service keeps the download for later, and a failed install brings the service back. A failed check, for example while offline, reports its error and keeps an already downloaded release in the tray; *Check for updates* does not offer to install it then, because installing also needs the release server. Homebrew installs report that `brew upgrade anda` is needed.
- **The desktop app** checks its GitHub release feed in the background one minute after launch, then every six hours after a check completes. This runs even when the window is closed or the runtime was stopped. A new version appears in the tray and is announced once per version per app session; checks with no update or a network failure stay quiet. Downloads and installation require confirmation through the update dialog. The service keeps running; only open terminals must be closed before installing. Only signed release builds have a desktop update feed; development and unsigned builds skip these checks and are updated manually.
- Uninstalling the desktop leaves the shared `anda`, its data and its login entry in place; remove them like a script install (`anda autostart uninstall`, then delete the install directory).

`anda` runtime 通过自己的发布通道更新：托盘提示可安装时，会先暂停新任务、等待活动任务完成，停止服务后再替换 `anda` 并启动服务。即使下载超过维护租约期限，服务在整个安装期间也保持停止；安装失败时会恢复服务。检查失败（例如离线）时会报告错误，已下载的版本仍保留在托盘中；由于安装同样需要访问发布服务器，此时「检查更新」不会提示安装。桌面应用本身在签名版中通过 GitHub Release 更新，更新时服务继续运行。卸载桌面端不会删除共享的 `anda`、数据和登录项。

桌面客户端会在启动 1 分钟后自动检查更新，之后每次检查完成后间隔 6 小时再检查；关闭窗口或停止后台服务不会停止桌面客户端的检查。发现新版本时会添加托盘更新入口，并在每次启动期间对同一版本仅提醒一次；没有更新或网络检查失败时保持安静，等待下次检查。下载和安装需要用户在更新弹窗中确认。开发版和未配置签名更新通道的版本不进行桌面客户端自动检查，需要手动安装新版。

## Build

Requires a current Rust toolchain and Node.js 22.12+ with pnpm. The repository manages JavaScript dependencies through its pnpm workspace.

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

The chat title opens a menu to rename, pin, archive, copy the conversation as Markdown or show its folder; the chip beside it names the folder and its Git branch. A new chat's folder is chosen from the menu under the composer, keeping the draft; choosing a folder from a chat that has already started opens a new chat there. Message actions appear on hover, and consecutive tool calls fold into one expandable row. A pending approval also docks above the composer, where ⌘⇧↩ approves and ⌘⇧⌫ denies it (Ctrl+Shift on Windows). Long chats show a mark per prompt along the transcript's edge.

## Workbench

Open the right panel from the chat header's **Resources**, **Changes**, **Terminal** and **Browser** buttons, or with ⌘⇧A, ⌘⇧G, ⌘⇧T and ⌘⇧B (Ctrl+Shift on Windows and Linux; the View menu lists every shortcut). Drag the panel's edge to resize it (double-click restores the default width) or maximize it from its header. Changes and Terminal open only the chat's own folder: a project, the folder a new chat was started in, or the folder a chat started from the `anda` terminal ran in.

- **Resources** lists the chat's attachments. A file path a reply puts in inline code (`src/app.ts:42`) opens a read-only preview here, at that line; previews stay inside the chat's folder, read at most 512 KB and never open the file in another application (*Show in folder* reveals it instead).
- **Changes** requires the selected project to be a Git repository root. View staged/working-tree changes with line counts and coloured diffs, and history; stage, unstage and commit from explicit controls. *This chat* narrows the list to files the agent edited in the chat, and each finished turn that edited files ends with a card linking to their diffs. Operations reject a changed repository snapshot. Git hooks, helpers and repository filters run with your user permissions; use repositories you trust.
- **Worktrees** creates branches in desktop-managed directories. Archiving another managed worktree saves a snapshot first, then removes its checkout; restore recreates its contents at the original path on a detached snapshot commit. It preserves content rather than the original staging layout. Ignored files, submodules and nested repositories must be handled separately before archiving. Stop other processes using the checkout before confirming archive. Your primary or externally managed worktrees cannot be archived here.
- **Terminal** runs your shell in the selected project; the panel header's + starts a session and ⌘F (Ctrl+Shift+F) finds in its output. Its colours follow the app theme. Sessions survive panel and chat switches; quitting ends them after confirmation. Output and scrollback are bounded. These are user terminals; agent commands continue through the daemon's existing approval rules.
- **Browser** has its own persistent login profile and per-chat tabs. Multiple chats keep their browser sessions registered concurrently, including after reconnect. It does not import Chrome cookies or install Chrome extensions. With the new runtime, that chat's agent uses its own desktop browser session through the existing browser tools. Pages accept HTTP/HTTPS URLs; local attachments use Resources. Remote pages have no application bridge or Node access. Website microphone access requires explicit permission; camera access stays disabled. Agent file uploads require local confirmation; downloads use a save dialog. Browser tabs currently last for the desktop process lifetime; website cookies persist.
- **Settings → Audio** selects devices for a recording/playback self-test, displays an input meter and tests configured transcription/TTS providers. Stop cancels playback and suppresses late test results. Physical microphones, Bluetooth routing, audio quality and real provider behavior still need device testing.

The Automations editor loads the complete saved task before editing; task previews in the list never replace the full prompt or shell command.

Normal chat speech can also be stopped from the composer. Input drafts are preserved independently of browser, terminal and Git panels.

## Release

`.github/workflows/release.yml` builds the desktop for macOS arm64 and Windows x64 from the same release's `anda` binaries and attaches `Anda-mac-arm64.dmg/.zip`, `Anda-win-x64.exe`, blockmaps, checksums and update metadata. The desktop job installs only desktop/Chrome extension dependencies and reuses the downloaded runtime through the job-wide `ANDA_DESKTOP_RUNTIME`; it never compiles Rust. With a single macOS architecture, `latest-mac.yml` is published directly. `scripts/publish-homebrew.sh` reads the release's local checksums and publishes the Apple Silicon `anda-desktop` cask, which depends on the `anda` formula; the formula supports macOS arm64 and Linux arm64/x86_64.

`.github/workflows/desktop.yml` checks the desktop app and the Chrome extension on macOS and Windows for pull requests and pushes to `main`: type checks, unit tests, native Electron tests, an unsigned local package and NSIS install/uninstall checks. That package bundles an unoptimized `anda` (`cargo build --profile ci`, passed through `ANDA_DESKTOP_RUNTIME`) to keep the job short, so the workflow uploads only the Electron test screenshots, never the installers; the Rust tests run in `test.yml`.

For signed builds, configure the protected `desktop-release` environment: `MAC_CSC_LINK` / `MAC_CSC_KEY_PASSWORD`, `WIN_CSC_LINK` / `WIN_CSC_KEY_PASSWORD`, `APPLE_ID`, `APPLE_APP_SPECIFIC_PASSWORD`, `APPLE_TEAM_ID`, and the public variable `ANDA_WINDOWS_PUBLISHER`. The macOS certificate and Apple credentials also sign and notarize the `anda-macos-*` and `anda_launcher-macos-*` release binaries (`scripts/sign-macos-binaries.sh`) before the desktop bundles them. Without certificates the release job builds unsigned packages with the local configuration and no update feed.

`electron-builder.release.cjs` requires signatures and macOS notarization and publishes update metadata for the GitHub release feed. `scripts/seal-runtime.cjs` signs the bundled runtime like the release binary, with the identifier `ai.anda.anda-bot` and `anda_bot/assets/anda.entitlements.plist` (microphone only, for voice input once it is installed at the shared CLI location), and records its digest after that final signature, before sealing the outer app; the app verifies platform, architecture and digest before running `anda install`. The default local configuration remains ad-hoc signed.

The NSIS installer only stops processes started from its own directory (`resources/installer.nsh`): the default electron-builder check matches `$INSTDIR` as a bare prefix, and `...\Programs\Anda` is a prefix of `...\Programs\AndaBot`, where the shared `anda` and its daemon run.

App protocol types are generated from Rust and checked in tests:

```bash
ANDA_EXPORT_PROTOCOL=1 RUST_MIN_STACK=16777216 cargo test -p anda_bot desktop_protocol_types --bin anda
```

Windows build configuration is included, but macOS testing does not substitute for Windows signing, installation, notifications and audio-device validation. Microphone/TTS need configured providers and OS permission; automated tests do not validate physical audio devices or printers.
