# Anda Desktop

Electron + Svelte desktop client for the local Anda daemon. Chat messages, approval cards, attachments, memory, skills, bookmarks, configuration and voice reuse the Chrome extension's implementation. The extension remains independently buildable.

## Local installation / 本地安装

On macOS, open the generated `Anda-<version>-mac-arm64.dmg` and drag **Anda.app** into Applications. The application includes its matching Rust runtime; Node.js, pnpm and Rust are not required to run the installed app. This development package is ad-hoc signed for local use, not Developer ID notarized for public distribution.

macOS 上打开生成的 DMG，将 **Anda.app** 拖入“应用程序”。安装包包含配套 Rust runtime，运行应用不需要安装开发工具。当前是供本机使用的临时签名开发包，未进行公开分发所需的 Developer ID 公证。

- Existing `~/.anda` configuration, identity and conversations are reused. No browser token needs to be copied into the UI.
- If configuration is incomplete, open **Settings → Agent configuration**, enter the model/provider details, save, then reconnect. Offline saves are validated by the Rust parser before replacing the file; a private `config.yaml.desktop-backup` is preserved.
- An older running daemon can serve ordinary chat. Desktop project workspaces require the new capability; use **Settings → Restart daemon** to activate the bundled runtime. Restarting explicitly interrupts active tasks and requires confirmation in the app.
- Closing the window keeps the tray and notifications. **Quit Anda Desktop** leaves the daemon running; notifications stop until the app is opened again. Explicitly stopping the daemon prevents background polling from restarting it.
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

This version uses the extension's established conversation-delta polling contract. Requests whose acknowledgement is lost are persisted as unconfirmed and never automatically resubmitted. The UI requires conversation review before allowing another message. It does not claim server-side exactly-once execution. Public update delivery, general token streaming, a full embedded browser, PTY terminals, and Git/worktree management remain separate roadmap work.

Windows build configuration is included, but macOS testing does not substitute for Windows signing, installation, notifications and audio-device validation. Microphone/TTS need configured providers and OS permission; automated tests do not validate physical audio devices or printers.
