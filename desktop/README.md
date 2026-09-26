# Anda Desktop

Electron + Svelte desktop client for the local Anda daemon. Chat messages, approval cards, attachments, memory, skills, bookmarks, configuration and voice reuse the Chrome extension's implementation. The extension remains independently buildable.

## Local installation / 本地安装

On macOS, open the generated `Anda-<version>-mac-arm64.dmg` and drag **Anda.app** into Applications. The application includes its matching Rust runtime; Node.js, pnpm and Rust are not required to run the installed app. This development package is ad-hoc signed for local use, not Developer ID notarized for public distribution.

macOS 上打开生成的 DMG，将 **Anda.app** 拖入“应用程序”。安装包包含配套 Rust runtime，运行应用不需要安装开发工具。当前是供本机使用的临时签名开发包，未进行公开分发所需的 Developer ID 公证。

- Existing `~/.anda` configuration, identity and conversations are reused. No browser token needs to be copied into the UI.
- If configuration is incomplete, open **Settings → Agent configuration**, enter the model/provider details, save, then reconnect. Offline saves are validated by the Rust parser before replacing the file; a private `config.yaml.desktop-backup` is preserved.
- An older running daemon can serve ordinary chat using polling. Application events, durable receipts, desktop browser routing and update coordination require the new runtime; after finishing active tasks, use **Settings → Restart daemon** to activate it. An explicit restart interrupts active tasks and requires confirmation in the app.
- Closing the window keeps the tray, terminals and notifications. **Quit Anda Desktop** leaves the daemon running, but asks before ending active user terminals; notifications stop until the app is opened again. An explicit daemon stop is remembered across desktop restarts until you reconnect.
- Imported IM channel conversations are read-only; start a local chat to respond without changing the original sender or reply route.
- Chrome page automation continues to require the Chrome extension. Electron does not share Chrome's tabs or login state.
- If the older `anda_launcher` is also running, quit its tray UI separately. This local build does not remove the existing launcher or rewrite its login registration.
- Application updates are installed manually from a new desktop package. The bundled runtime disables its standalone self-installer so it cannot replace files inside the app bundle. No public desktop update feed is configured yet.
- Before replacing an existing desktop installation that runs the bundled daemon, finish active work, stop the daemon from Settings, and quit the desktop app. Then install the new package and open it again.

已有配置和数据会继续使用。旧 daemon 可提供普通聊天；项目工作目录需要新版本，在设置中明确重启后生效。关闭窗口、退出桌面应用、停止后台服务是三个不同动作。当前本地版采用重新安装桌面包进行升级，不会自动覆盖现有 CLI 安装或旧 launcher 的开机启动设置。

## Build

Requires a current Rust toolchain and Node.js 22.12+ with pnpm. The repository manages JavaScript dependencies through its pnpm workspace.

```bash
pnpm install --filter @anda/desktop...
pnpm --dir desktop check
pnpm --dir desktop test
pnpm --dir desktop build
pnpm --dir desktop package
```

`package` builds `anda` with `cargo build --release --locked`, copies it into the application's resources, and creates the local platform's installers under `desktop/release/`. `package:dir` produces an unpacked application. For a CI-built runtime, set `ANDA_DESKTOP_RUNTIME` to that executable before packaging; its version and SHA-256 are recorded in a manifest. Do not commit runtime binaries, installers, or test screenshots.

For development:

```bash
pnpm --dir desktop dev
```

You can select a custom home with `--anda-home=/absolute/path`; the desktop profile is isolated by home. The default Electron profile stores window bounds, navigation metadata and text drafts under the OS application-data directory. Authoritative messages and memory remain in Anda's home. Attachments in unsent drafts survive chat switches in the current window, but are not persisted to disk when the application exits.

## Validation

Use `--anda-profile=/absolute/path` when an explicit, separate UI profile is needed. `scripts/packaged-smoke.mjs /path/to/Anda.app` uses temporary home/profile directories with the daemon stopped to verify the installed bundle's runtime digest, PTY native module and browser isolation without accessing your real identity or conversations.

```bash
pnpm --dir desktop test:e2e
pnpm --dir chrome-extension check
pnpm --dir chrome-extension test
pnpm --dir chrome-extension i18n
pnpm --dir chrome-extension build
RUST_MIN_STACK=16777216 cargo test -p anda_bot desktop_ -- --nocapture
RUST_MIN_STACK=16777216 cargo test -p anda_bot daemon_control_routes_serve_config_and_status -- --nocapture
RUST_MIN_STACK=16777216 cargo test -p anda_bot --test cli_validation -- --nocapture
```

The Electron smoke test uses a temporary profile and authenticated local mock daemon. It never opens the real Anda database or calls a paid model. Screenshots are written to `desktop/test-results/`. GUI tests need a graphical session and may need to run outside a command sandbox.

With a current runtime, `/ws/app/v1` sends caller-scoped state invalidations after persistence. The shared conversation client reads authoritative deltas on notification, coalesces changes and recovers from a new snapshot on reconnect; older runtimes retain polling. Accepted submissions get durable receipts under `~/.anda/desktop-submissions/`. A lost reply is reconciled by request ID; completed responses (including side replies) are delivered to the renderer before their local recovery references are cleared; an uncertain crash window remains unknown and is never automatically replayed. This does not promise exactly-once external tool effects or token streaming.

## Workbench

Open the right panel from a chat's header, then choose **Changes**, **Terminal**, **Browser** or **Resources**.

- **Changes** requires the selected project to be a Git repository root. View staged/working-tree changes and history; stage, unstage and commit from explicit controls. Operations reject a changed repository snapshot. Git hooks, helpers and repository filters run with your user permissions; use repositories you trust.
- **Worktrees** creates branches in desktop-managed directories. Archiving another managed worktree saves a snapshot first, then removes its checkout; restore recreates its contents at the original path on a detached snapshot commit. It preserves content rather than the original staging layout. Ignored files, submodules and nested repositories must be handled separately before archiving. Stop other processes using the checkout before confirming archive. Your primary or externally managed worktrees cannot be archived here.
- **Terminal** runs your shell in the selected project. Sessions survive panel and chat switches; quitting ends them after confirmation. Output and scrollback are bounded. These are user terminals; agent commands continue through the daemon's existing approval rules.
- **Browser** has its own persistent login profile and per-chat tabs. It does not import Chrome cookies or install Chrome extensions. With the new runtime, that chat's agent uses its own desktop browser session through the existing browser tools. Pages accept HTTP/HTTPS URLs; local attachments use Resources. Remote pages have no application bridge or Node access. Website microphone access requires explicit permission; camera access stays disabled. Agent file uploads require local confirmation; downloads use a save dialog. Browser tabs currently last for the desktop process lifetime; website cookies persist.
- **Settings → Audio** selects devices for a recording/playback self-test, displays an input meter and tests configured transcription/TTS providers. Stop cancels playback and suppresses late test results. Physical microphones, Bluetooth routing, audio quality and real provider behavior still need device testing.

Normal chat speech can also be stopped from the composer. Input drafts are preserved independently of browser, terminal and Git panels.

## Release and update coordination

Local packages have no public update feed and continue to use manual installation. `.github/workflows/desktop.yml` builds and tests macOS and Windows, including native Electron tests, bundled runtime and NSIS install/uninstall checks. The workflow has not been run on Windows from this macOS development session.

For a signed build, configure the protected `desktop-release` environment: `MAC_CSC_LINK` / `MAC_CSC_KEY_PASSWORD`, `WIN_CSC_LINK` / `WIN_CSC_KEY_PASSWORD`, `APPLE_ID`, `APPLE_APP_SPECIFIC_PASSWORD`, `APPLE_TEAM_ID`, and the public variables `ANDA_UPDATE_URL` / `ANDA_WINDOWS_PUBLISHER`. Certificate variables are supplied through `CSC_LINK` and `CSC_KEY_PASSWORD` to each platform build. Trigger the workflow with `signed: true`; it uploads artifacts and does **not** publish them. Publish complete installers, ZIPs, blockmaps and update metadata to the configured HTTPS directory together, making the metadata available last.

`electron-builder.release.cjs` requires signatures and macOS notarization. `scripts/seal-runtime.cjs` records the runtime digest after its final signature, before sealing the outer app. The desktop checks platform, architecture and digest before launching the bundled runtime. The default local configuration remains ad-hoc signed.

Signed builds support **Check for updates** → download → explicit install/restart. For the matching bundled runtime, a renewable owner-only maintenance lease stops new task admission, delays cron claims, sends an explicit retry response to the original IM route, and waits for active work to finish. A busy runtime keeps the downloaded update for later. External runtimes are not stopped or replaced; unverifiable ownership blocks installation until the service is stopped explicitly. An interrupted update leaves a recovery record in the desktop profile. If the client disappears before shutdown, its maintenance lease expires after 90 seconds.

App protocol types are generated from Rust and checked in tests:

```bash
ANDA_EXPORT_PROTOCOL=1 RUST_MIN_STACK=16777216 cargo test -p anda_bot desktop_protocol_types --bin anda
```

Windows build configuration is included, but macOS testing does not substitute for Windows signing, installation, notifications and audio-device validation. Microphone/TTS need configured providers and OS permission; automated tests do not validate physical audio devices or printers.
