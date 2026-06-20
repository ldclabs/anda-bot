# Changelog

All notable changes to Anda Bot.

## [Unreleased]

### Added

### Changed

- **Installers restart the daemon after upgrades**: Unix, Windows, and Homebrew install flows now guide users to restart the Anda daemon so upgraded binaries are picked up by already-running sessions.

### Fixed

- **macOS launcher restores the tray item when reopened**: the launcher now finishes AppKit startup explicitly, avoids duplicate reactivation observers, and rebuilds the status-bar item when macOS reopens the app.

## [0.10.1] — 2026-06-20

### Added

- **Media-understanding tools advertise a shared capability group**: grouped image, audio, video, and generic attachment understanding agents under one discovery bundle with shared usage guidance for file, URL, and attached-resource workflows.
- **Auto-research skill for long-horizon work**: bundled an `auto-research` skill that defines a file-backed ledger, stall/pivot rules, worker prompt shape, and patrol boundaries for autonomous research or unattended follow-up loops.
- **Handoff skill for session transfer**: bundled a `handoff` skill that writes a redacted temporary handoff document with focus-specific context, suggested skills, and references to existing artifacts instead of duplicating them.
- **Learn skill for concept anatomy**: bundled a `learn` skill for one-shot deep concept dissection with anchors, eight analytical cuts, introspection, compression, org-mode output, and optional saved notes.

### Changed

- **Session compaction now uses reusable engine handoffs**: replaced bot-local compaction continuation/retry plumbing with `CompletionRunner::handoff`, keeping long sessions in child conversations while preserving accumulated usage, artifacts, tool usage, discovered-tool settings, and runner state.
- **Version synchronized for the 0.10.1 daemon release**: updated the `anda_bot` crate and Cargo lock metadata to advertise `0.10.1`, and aligned bundled Anda dependencies with `anda_brain` `0.8.1`, `anda_core` `0.13.7`, and `anda_engine` `0.13.9`.

### Fixed

- **Idle compaction keeps active sessions alive**: compaction now refreshes the session activity clock and continues in a child conversation even without immediately pending user work, including sessions that still have background tasks.
- **Pending follow-up and steering batches are compacted before attachment**: queued follow-up and steer content now participates in the engine compaction threshold check before it is attached to the runner, avoiding oversized next requests.
- **Goal activation is restricted to the main Anda Bot session**: subagents that inherit session state now receive a clear error and must report requested goal changes back to their caller instead of mutating the main session goal directly.

## [0.10.0] — 2026-06-20

### Added

- **Chat messages can be bookmarked from the browser extension**: assistant messages now offer local bookmarks that are saved in the daemon, can be organized into folders, and can jump back to the original conversation from the side panel or dashboard.
- **Bookmarks API and dashboard workspace**: added the daemon-side bookmark store/tooling plus a dedicated browser extension dashboard workspace for browsing and managing bookmarked messages.
- **Quick prompt shortcuts in the browser composer**: user messages can be saved as reusable quick inputs, shown as composer chips, inserted with one click, removed individually, or cleared together with localized confirmation text.
- **Prompt command suggestions follow the active UI language**: slash-command suggestion descriptions and details now use localized extension strings instead of fixed English copy.
- **Teaching and grilling skill definitions**: bundled new `teach` and `grilling` skills, including teaching workspace formats for missions, resources, learning records, and glossaries.
- **Edge browser extension guidance**: added Edge-specific browser extension installation and store-description guidance across the docs and localized store copy.

### Changed

- **Browser extension dashboard entrypoint replaces the old Brain/config split**: the extension now builds a dashboard page that hosts bookmark management while preserving access to existing configuration and Brain views.
- **Version synchronized for the 0.10.0 release**: updated the `anda_bot` crate, Cargo lock metadata, and browser extension package/manifests to advertise `0.10.0`.

### Fixed

- **Launcher tray icons recover more reliably**: macOS launcher single-instance locking now lives under the app-owned home directory instead of a purgeable temp directory, and relaunches can ask the running instance to rebuild a dropped status-bar item.
- **Launcher relaunch recovery avoids unnecessary kickstarts**: macOS only force-restarts the LaunchAgent when `launchctl` reports a running managed job, falling back to in-process status-bar reactivation for manually started launchers.
- **Multimodal text summaries preserve grapheme boundaries**: long text summaries now truncate head and tail excerpts on Unicode grapheme boundaries so emoji and ZWJ sequences are not split.

## [0.9.16] — 2026-06-18

### Added

- **Built-in tools now advertise capability groups**: cron scheduler tools and IM channel messaging tools now report shared `ToolGroupInfo` metadata so tool selection can present related tools as coherent bundles with usage guidance.

### Changed

- **Agent instructions now expose capability group discovery**: the runtime prompt and default skill tool set now surface `tools_groups`, so agents can list bundles before expanding them through `tools_select`.
- **Version synchronized for the 0.9.16 release**: updated the `anda_bot` crate and Cargo lock metadata to advertise `0.9.16`, and aligned bundled Anda dependencies with `anda_core`/`anda_engine` `0.13.6`.
- **Shared HTTP requests can run longer before timing out**: increased the shared HTTP client timeout from 120 seconds to 300 seconds for slow network or long-running remote API calls.

## [0.9.15] — 2026-06-18

### Added

- **MCP servers can be exposed as dynamic Anda tools**: the daemon now loads optional MCP server definitions from `ANDA_HOME/mcp.json`, registers `anda_engine` MCP tool providers, and makes remote MCP tools discoverable through the normal tool-selection flow.
- **Runtime MCP server registration tool**: added `add_mcp_server` so agents can connect stdio or Streamable HTTP MCP servers at runtime, optionally persist them to `mcp.json`, and use include/exclude filters for remote tools.
- **MCP configuration documentation**: documented the standalone `mcp.json` format, supported transports, environment/path expansion, persistence behavior, and examples across the README and runtime configuration docs.

### Changed

- **Version synchronized for the 0.9.15 release**: updated the `anda_bot` crate and Cargo lock metadata to advertise `0.9.15`, and aligned bundled Anda dependencies with `anda_core`/`anda_engine` `0.13.5`.

## [0.9.14] — 2026-06-17

### Changed

- **Version synchronized for the 0.9.14 release**: updated the `anda_bot` crate and Cargo lock metadata to advertise `0.9.14`, and aligned the bundled Anda engine dependency with `0.13.4`.
- **Default Codex context budget is more conservative**: reduced the bundled Codex `context_window` from `400000` to `320000` so session compaction triggers before provider-side limits are hit.

### Fixed

- **Oversized input batches compact before being attached**: session follow-up and steering inputs are now sized as a batch before queueing, so bursts of background results cannot bypass idle compaction and overflow the next model request.
- **Context-length recovery compacts committed history instead of replaying the failing request**: when a model reports a context-length error, the runner rebuilds around the committed history and compacts that smaller context, preserving background tasks when recovery succeeds.
- **Failed sessions no longer keep background work alive or retain stale errors**: unrecoverable model failures now stop background tasks, failed conversations stop their runner loop, and successful turns clear leftover `failed_reason` state.

## [0.9.13] — 2026-06-17

### Changed

- **Version synchronized for the 0.9.13 release**: updated the `anda_bot` crate and Cargo lock metadata to advertise `0.9.13`.

### Fixed

- **Pending user input is preserved during session compaction**: context compaction now includes the active pending prompt, content, and documents in the handoff, then continues in a child conversation so long-running sessions do not lose the user request when the model context window is reached.

## [0.9.12] — 2026-06-16

### Changed

- **CLI updates now keep launcher sidecars synchronized**: `anda update` now downloads and installs the macOS or Windows `anda_launcher` sidecar when it is already present, including already-current CLI runs where only the launcher may need refreshing.
- **Launcher self-updates restart into the refreshed binary**: macOS and Windows launchers now relaunch themselves after successfully installing an update so the new launcher binary is used immediately.
- **Version synchronized for the 0.9.12 release**: updated the `anda_bot` crate and Cargo lock metadata to advertise `0.9.12`.

### Fixed

- **HTTP clients no longer panic when multiple rustls crypto providers are linked**: startup and shared reqwest client construction now install the rustls `aws-lc` default provider before TLS handshakes.
- **Telegram bot tokens are redacted from request and API error logs**: Telegram polling, sending, attachment, and health-check paths now scrub bot tokens from logged request URLs and API responses.
- **Active session listing is stable under concurrent activity updates**: session summaries now snapshot activity timestamps before sorting, avoiding comparator panics while sessions update their `active_at` values.

## [0.9.11] — 2026-06-15

### Added

- **Loop command is now a first-class long-running command**: `/loop` now has dedicated parsing, slash completion, TUI help, self-instruction guidance, and docs for recurring or self-paced follow-ups instead of acting as a `/goal` alias.
- **Trusted user key management CLI**: added `anda user list`, `anda user create`, `anda user import`, and `anda user pubkey` to generate, inspect, and register Ed25519 keys for channel-bound trusted users.
- **Multi-user and Discord setup documentation**: documented the `anda user` workflow, channel `user` binding, and Discord bot installation/configuration steps across the README and localized docsite channel guides.

### Changed

- **Stop and cancel commands now have distinct behavior**: `/stop` stops the current task and leaves the conversation idle, while `/cancel` exits the active conversation session; the TUI, browser extension, README, and docsite command help now document the split semantics.
- **COSE key and token handling now uses `cose2` directly**: replaced `ic_cose_types` helpers with `cose2` and `ed25519-dalek` for Ed25519 COSE key import/export and CWT Sign1 token generation.
- **Makefile fix target formats before applying clippy fixes**: `make fix` now runs `cargo fmt --all` before `cargo clippy --fix`.
- **Version synchronized for the 0.9.11 release**: updated the `anda_bot` crate, Cargo lock metadata, and browser extension package/manifests to advertise `0.9.11`.

### Fixed

- **Model tool discovery compatibility is reused per model**: the agent now applies known `merge_discovered_tools` policies for DeepSeek and GPT-family models and caches successful probe results for unknown models across future completion runners.
- **Stopped background tasks no longer keep sessions alive**: session state now counts only running background tasks, clears stopped task progress output, and ignores late background results after `/stop`.

## [0.9.10] — 2026-06-14

### Added

- **Runtime model reload**: added `anda models reload`, a `POST /daemon/models/reload` daemon control endpoint, launcher menu entries, and browser extension wiring so running daemons can refresh model configuration from `config.yaml` without a full restart.
- **Media workspace dependencies**: added the package workspace wiring needed for media-related extension tooling.

### Changed

- **Model configuration saves now reload in place**: saving `config.yaml` attempts to refresh the running model registry instead of asking users to restart the daemon when only model settings changed.
- **Browser extension update controls moved out of the extension UI**: removed in-app update polling and install/restart controls because updates are handled by the launcher.
- **Version synchronized for the 0.9.10 release**: updated the `anda_bot` crate, Cargo lock metadata, and browser extension package/manifests to advertise `0.9.10`.

### Fixed

- **Channel message delivery is more robust**: Discord REST requests now include the required DiscordBot user agent, long message splitting respects continuation-marker budgets, and attachment resource tags are matched case-insensitively across channels.
- **Cron due jobs no longer starve later scheduled work**: due-job polling now preserves scheduling progress so one ready job cannot prevent other eligible jobs from being considered.
- **Homebrew release publishing checks out the repository first**: the release workflow now ensures the repository is available before running the Homebrew publishing step.

## [0.9.9] — 2026-06-13

### Changed

- **Dependencies aligned with Anda 0.13 and Brain 0.8**: updated the bot workspace to the latest Anda runtime, engine, object store, database, Web3, Brain, COSE, and auth crates.
- **CBOR and COSE key handling now use `cbor2`**: replaced direct `ciborium`/`coset` usage with `cbor2` and `ic_cose_types` while preserving Ed25519 COSE key import/export behavior.
- **Release automation publishes the Homebrew formula**: the GitHub release workflow now invokes the Homebrew publishing script after attaching release artifacts.

### Fixed

- **Proxy-aware HTTP clients explicitly disable ambient proxy defaults**: shared reqwest client builders now start from `no_proxy()` before re-adding filtered environment proxies, keeping local and private-network traffic off configured proxies.

## [0.9.8] — 2026-06-12

### Added

- **Resource blobs can be downloaded for local processing**: the resources API now offers a `DownloadResource` operation that writes a persisted resource blob to a safe local filename in a requested directory or the system temp directory.
- **IM channel tools**: agents can now list configured IM channels and send messages through configured IM channel recipients using strict-schema tools.
- **Skills in slash completions**: slash command completions now surface available skills so they are easier to discover and invoke.
- **Browser extension Brain and config page localization**: the Brain Graph and config editor pages now use bundled localized strings across supported locales.
- **Idle Brain sleep maintenance**: when all bot sessions and background tasks have been continuously idle, the daemon now triggers a full Brain maintenance cycle if the previous maintenance start is more than 12 hours old.

### Changed

- **Agent runtime split into focused modules**: separated instruction rendering, request metadata, session state, session running, and startup recovery logic from the monolithic agent implementation.
- **Launcher language follows the extension UI**: launcher-facing text now stays aligned with the browser extension language choice.

### Fixed

- **HTTP clients bypass local and private addresses when proxies are configured**: shared reqwest clients now honor standard proxy environment variables for external hosts while always exempting loopback and private-network traffic, so daemon, Brain, and local mock requests are not accidentally routed through a proxy.
- **Browser extension polling, socket teardown, and browser actions are more robust**: hardened reconnect and browser-action paths to avoid stale state and missed updates.
- **Hand-edited config YAML is preserved**: config saves now keep manually edited YAML structure and comments where possible instead of unnecessarily rewriting the file.
- **Brain Graph sync no longer stalls**: graph synchronization now recovers from stalled sync state instead of leaving the page stuck.
- **Optimistic chat sends are more reliable**: pending chat messages now reconcile more cleanly with accepted server history.
- **Follow-up poll output is delivered**: queued follow-up results are now surfaced instead of being lost during polling.
- **Daemon exits after HTTP shutdown**: HTTP shutdown now propagates through the daemon cancellation tree and no longer waits indefinitely on the OS signal handler.

### Dependencies

- Refreshed lockfile dependencies, including `anda_brain` 0.7.2, `anda_engine` 0.12.37, `liteparse` 2.0.8, `liteparse-pdfium` 1.1.0, `memchr` 2.8.2, and `smallvec` 1.15.2.

## [0.9.7] — 2026-06-11

### Changed

- **Launcher downloaded updates install only from explicit menu action**: automatic update polling now records downloaded updates without immediately opening restart prompts, and manual update checks use the tray/menu item as the single explicit install-and-restart entrypoint.
- **Launcher update menu state clears after successful restart handoff**: successful update installation now clears the pending downloaded-update state so the menu returns to the normal update-check label.
- **Launcher downloaded-update label is action-oriented**: updated English and Chinese menu text from passive download status to “Install and restart” wording.
- **Version synchronized for the 0.9.7 release**: updated the `anda_bot` crate and Cargo lock metadata to advertise `0.9.7`.

### Fixed

- **Daemon config saves are atomic and serialized**: config updates now write through a same-directory temp file with fsync and rename, and concurrent updates are serialized, so a crash mid-save can no longer leave a truncated config that blocks the next daemon start.
- **Skill loading failures no longer block daemon startup**: errors scanning skill directories (such as permission problems in the shared `~/.agents/skills`) are logged and the daemon continues without skills instead of refusing to start.
- **Daemon status failures surface their cause**: `/daemon/status` errors are now logged and the underlying error detail is returned in the response instead of a generic message.

## [0.9.6] — 2026-06-10

### Added

- **Browser extension config.yaml editor**: added a dedicated configuration page for editing daemon runtime, model, voice, transcription, channel, and user settings, backed by authenticated local daemon config read/write endpoints that validate YAML and back up existing files before saving.
- **Browser extension Brain Graph visualizer**: added a dedicated Brain Graph page with searchable, expandable cognitive graph visualization, saved graph settings, and a side-panel shortcut that opens it from the extension.
- **Browser extension read-only Brain RPC bridge**: added local websocket RPC methods for Brain status and read-only KIP queries so the default `anda_bot` graph can be explored without exposing write operations.
- **Browser extension appearance theme control**: added a localized settings control for System, Light, and Dark themes, persists the preference, applies it to the side panel root, and follows system color-scheme changes when System is selected.

### Changed

- **Browser extension collapsed channels expand on hover**: the collapsed channel sidebar now opens a floating channel list on hover or keyboard focus, then closes after selecting a channel or leaving the sidebar.
- **Browser extension dark-mode styling is explicit**: message and channel UI styles now rely on the applied `.dark` theme class instead of implicit media-query overrides, so manual Light/Dark choices override the operating system preference.
- **Browser extension follow-ups appear inline**: follow-up prompts sent while a conversation is working now render as pending user messages inside the active conversation instead of a separate queued follow-ups panel, then merge with accepted server history when it returns.
- **Version synchronized for the 0.9.6 release**: updated the `anda_bot` crate, Cargo lock metadata, and browser extension package/manifests to advertise `0.9.6`.

## [0.9.5] — 2026-06-10

### Changed

- **Version synchronized for the 0.9.5 release**: updated the `anda_bot` crate and Cargo lock metadata to advertise `0.9.5`.

### Fixed

- **Channel workspace directory names are readable and portable**: channel ids now map invalid path characters to underscores on every platform, keeping workspace paths compatible with Windows and easier to inspect while preserving legacy workspace migration.
- **macOS launcher menu bar item stays alive reliably**: the macOS launcher now installs its status item from the application launch callback and retains the menu, status item, button, and icon for the delegate lifetime so the menu bar entry is not dropped prematurely.
- **Launcher update checks avoid duplicate work and prompts**: automatic update polling now shares the manual update-check gate, tracks downloaded update state, and suppresses duplicate restart prompts while one is already active.
- **Downloaded updates are easier to apply from the launcher**: manual update checks now immediately offer to restart and apply a previously downloaded update instead of starting another check.

## [0.9.4] — 2026-06-10

### Changed

- **Browser tool names are browser-neutral**: renamed the split browser tools from `chrome_tabs`, `chrome_page`, `chrome_input`, and `chrome_script` to `browser_tabs`, `browser_page`, `browser_input`, and `browser_script` because the same extension bridge can run in Chromium-family browsers beyond Chrome.
- **Version synchronized for the 0.9.4 release**: updated the `anda_bot` crate and Cargo lock metadata to advertise `0.9.4`.

### Fixed

- **Channel workspace names are portable across platforms**: channel workspaces now use the same percent-encoded safe directory layout on every platform, preventing macOS Finder from displaying channel ids containing `:` as nested paths.
- **Existing channel workspaces migrate to the safe layout**: non-Windows startup now moves or merges legacy channel workspace directories into their safe names so existing state is preserved after the layout change.
- **Notes primer compatibility restored**: system instruction rendering now reads the current notes payload shape and falls back to legacy note storage when needed.

### Dependencies

- Refreshed lockfile dependencies, including `anda_brain` 0.6.11, `anda_engine` 0.12.35, `regex` 1.12.4, and `zerocopy` 0.8.52.

## [0.9.3] — 2026-06-09

### Changed

- **Default model streaming enabled**: bundled model configuration now enables streaming for the default provider entries so supported model replies can start returning output sooner.
- **Voice reply playback feels faster and more natural**: browser extension voice replies now use shorter sentence-aware TTS chunks and synthesize the next segment while the current segment is playing, reducing pauses in long spoken replies.
- **Browser voice recording format handling improved**: voice recording normalization now validates audio formats before daemon upload, derives missing filename extensions from MIME types, accepts MP4/M4A browser recording aliases, and uses browser-neutral speech permission wording.
- **Homebrew macOS launcher packaging improved**: the Homebrew formula generator now publishes and installs the macOS `anda_launcher` resource alongside `anda`, validates launcher presence in formula tests, and adds caveats that explain how to start the menu bar launcher and refresh `~/Applications/Anda Bot.app`.
- **Installation docs synchronized for Homebrew launcher installs**: README and docsite install pages now clarify that Homebrew installs both `anda` and `anda_launcher` on macOS and that running `anda_launcher` refreshes the application entrypoint.
- **Version synchronized for the 0.9.3 release**: updated the `anda_bot` crate, Cargo lock metadata, and browser extension package/manifests to advertise `0.9.3`.

### Fixed

- **Session recovery around background tasks improved**: sessions now keep listening for queued inputs and background task results after a runner stop or model error instead of prematurely ending work while asynchronous tool results are still pending.
- **Voice capture reliability improved**: browser page speech recognition now cleans up timed-out permission prompts and stopped sessions without unexpectedly restarting after delayed `onend` events.
- **CLI voice recording robustness improved**: CLI voice capture no longer drops input events when audio callbacks outpace processing, encodes negative full-scale WAV samples correctly, and checks command availability with `where` on Windows.

### Dependencies

- Refreshed lockfile dependencies, including `anda_engine` 0.12.33 and related Rust/Wasm package updates.

## [0.9.2] — 2026-06-09

### Changed

- **Daemon status observability expanded**: `anda status --json` and the local daemon status endpoint now expose conversation and memory graph counts, and desktop launcher status menus display those counts alongside PID and Gateway URL details.
- **Launcher daemon status and token UX improved**: desktop launcher menus now show cached daemon PID and Gateway URL details, refresh status in the background, and provide a dedicated Browser Extension token copy action while still copying the full pairing output for setup.
- **Launcher update UX refined**: desktop launcher update menus now show the current version, display a localized checking state with spinner feedback, remember the latest update state, prevent duplicate manual update checks, and make downloaded updates clearer with "restart to apply" wording across Windows and macOS.
- **Windows installer download streamlined**: the website install page now links Windows users directly to the latest `AndaBotSetup-windows-x86_64.exe` release asset and marks it as a download instead of sending them to the generic releases page.
- **Version synchronized for the 0.9.2 release**: updated the `anda_bot` crate and Cargo lock metadata to advertise `0.9.2`.

### Fixed

- **Model completion retry handling**: retryable model provider errors now wait for the advertised retry-after delay (or a safe default) before retrying session turns, compaction, and multimodal understanding requests, improving recovery from transient rate limits and provider failures.
- **Launcher update version comparison**: update-state handling now normalizes version tags before comparing current and latest versions, avoiding mismatches between bare versions and `v`-prefixed release tags.

## [0.9.1] — 2026-06-08

### Changed

- **Website and documentation positioning refreshed**: updated the website landing page and multilingual docsite home pages around Anda Bot's memory-first local assistant experience, added a browser/side-panel product preview, generalized Chrome-specific wording to Browser Extension, and refreshed installation and quick-start copy.
- **Version synchronized for the 0.9.1 release**: updated the `anda_bot` crate and Cargo lock metadata to advertise `0.9.1`, and changed release builds to use `panic = "unwind"`.

### Fixed

- **Windows launcher responsiveness and single-instance behavior**: launching `anda_launcher` now activates a responsive existing tray instance, waits briefly for a starting instance, can continue when an old mutex holder has no responsive launcher window, and runs tray commands off the Windows message thread so the tray UI stays responsive.
- **Windows PDF attachment extraction**: PDF parsing on Windows now explicitly locates and loads `pdfium.dll` from `PDFIUM_LIB_PATH`, the `anda.exe` directory, the current directory, or `PATH`, and returns actionable setup guidance when PDFium is unavailable.

### Dependencies

- Added `liteparse-pdfium-sys` for Windows PDFium dynamic loading and refreshed lockfile dependencies including `http` 1.4.2 and `prost`/`prost-derive` 0.14.4.

## [0.9.0] — 2026-06-08

### Added

- **Desktop launcher for first-run setup, daemon control, browser pairing, and updates**: added an `anda_launcher` companion binary for Windows and macOS. It starts the daemon after setup, provides tray/menu-bar actions for opening Anda, checking status, restarting the daemon, generating a browser extension Gateway URL/Bearer token and copying it to the clipboard, opening logs, editing model settings, toggling launch-at-login, and checking for updates. The launcher also checks for updates automatically, downloads release assets through the shared auto-updater, and prompts to install and restart when an update is ready.
- **Windows graphical installer**: added a GitHub Release packaging job and `scripts/build-windows-installer.ps1` to produce `AndaBotSetup-windows-x86_64.exe`, bundling `anda.exe`, `anda_launcher.exe`, curated skills, Start Menu shortcuts, login autostart, uninstall support, and a GUI setup wizard.
- **macOS launcher install flow**: the shell installer now downloads, verifies, installs, registers, and starts `anda_launcher` on macOS so release installs provide a menu-bar launcher by default.
- **Localized launcher and workspace picker UI**: added English and Simplified Chinese launcher strings through `rust-i18n`, plus localized native workspace picker titles that follow system locale tags across macOS, Windows, and Linux.
- **Optional Windows artifact signing**: GitHub Release builds can now sign Windows binaries and installers through Microsoft Artifact Signing / Authenticode when signing configuration is present, while continuing to publish unsigned artifacts when it is not configured.
- **Edge store listing copy**: added Edge-specific localized store descriptions alongside the Chrome Web Store copy.

### Changed

- **Version synchronized for the 0.9.0 release**: updated the `anda_bot` package, Cargo lock metadata, and Chrome extension package/manifests to advertise `0.9.0`.
- **Release artifacts now include launcher binaries and app icons**: native Windows and macOS release builds produce checksummed `anda_launcher` artifacts alongside the CLI binary, embed proper launcher icons (`.icns` on macOS and real-dimension ICO data on Windows), and generate checksums after signing so hashes match final release assets. Linux releases continue shipping only the CLI binary.
- **Launcher menus refined**: desktop menus now promote logs, group model settings and launch-at-login under a Settings submenu, keep status/restart actions focused, expose browser extension token generation, and retain tray/menu-bar image resources so status icons stay visible.
- **Installer and desktop entrypoint UX improved**: Windows installers wait for child process output, register launcher autostart through the registry, improve browser launching and onboarding paths, and create more reliable desktop/start-menu entrypoints. macOS installs refresh application entrypoints so Finder notices updated icons and labels.
- **Installation docs refreshed for desktop installers**: README and docsite installation pages now recommend the Windows graphical installer for normal users and describe the macOS menu-bar launcher behavior.
- **Browser extension labels generalized**: user-facing extension copy now uses browser-generic wording instead of Chrome-specific labels where the extension can run in multiple Chromium-based browsers.
- **Website landing page repositioned**: refreshed the landing page around Anda Bot's memory-first local assistant thesis, clarifying how it differs from focused coding agents and broad tool-platform agents.

### Fixed

- **macOS launcher status icon and app icons**: fixed the menu-bar status item to use the status button image path, stay visible, and fall back to text when the icon is unavailable. Release installers now prefer the real `.icns` asset and only fall back to PNG conversion when needed.
- **Windows launcher and installer reliability**: fixed launcher installation, registry autostart handling, browser integration, setup onboarding, and ICO generation from PNG headers instead of hardcoded icon dimensions.
- **Native workspace picker behavior**: escaped localized picker prompts for AppleScript and PowerShell, added a top-most owner window for the Windows folder picker, and kept Linux picker titles localized for both `zenity` and `kdialog`.
- **Local file URI handling**: normalized file URI parsing across channel attachments, browser operations, multimodal handling, and document skill helpers so local file paths are handled consistently.
- **WeChat cached context token recovery**: stale cached context tokens expire after two hours, send failures clear the cached token and retry without it, and context token metadata refreshes whenever a token is observed.

### Removed

- **IRC channel support**: removed the IRC channel implementation, configuration paths, CLI wiring, docs, and extension/store references so the runtime focuses on the actively supported channel set.

### Dependencies

- Added launcher/localization support dependencies including `rust-i18n`, `objc2`, `objc2-app-kit`, `objc2-foundation`, and `png`; removed no-longer-used workspace TLS dependencies from the launcher-era code path.

## [0.8.14] — 2026-06-07

### Added

- **Bundled document skills expanded**: added `doc-coauthoring`, `pptx`, and `xlsx` skills so Anda Bot can guide structured document co-authoring and handle PowerPoint and spreadsheet workflows from the built-in skill set.
- **Cross-platform login autostart management**: added `anda autostart install|uninstall|status` for Windows Task Scheduler, macOS Launch Agents, and Linux user systemd/XDG autostart, with release installers registering login autostart by default and opt-out switches for PowerShell and shell installs.
- **Daemon status command**: added `anda status` to report whether the background daemon process and gateway are running, including pid, gateway URL, and log path when available.
- **Multi-user channel ownership**: `config.yaml` now supports top-level trusted `users` with Ed25519 public keys, and Telegram, WeChat, Discord, and Lark/Feishu channel entries can set `user` to choose which trusted Anda caller owns conversations, resources, and memory context.

### Changed

- **Bundled skill tooling refreshed**: updated `skill-creator`, `docx`, and `deep-research` guidance and helper scripts for Anda Bot skill packaging, validation, eval runs, and Office document workflows.
- **Daemon stop/restart improved**: `anda stop` and `anda restart` now first request an authenticated graceful shutdown through the local daemon gateway before falling back to process termination, and Windows daemon background launch/stop support was added.
- **Release installer startup flow**: after installation, PowerShell and shell installers can start the daemon immediately and now print daemon management commands instead of only the interactive UI command.
- **Channel runtime caller selection**: incoming channel messages now run under the configured channel user when present, falling back to the local owner, and the engine/brain managers include all configured trusted user public keys.
- **Dependencies bumped**: `anda_core` 0.12.7 → 0.12.8 and `anda_engine` 0.12.30 → 0.12.32.

### Fixed

- **Legacy text decoding**: local text files and text-like attachments now decode through the shared platform-aware text helper, allowing Windows legacy encodings such as GBK for config-adjacent files, prompt files, WeChat token caches, and text attachments instead of requiring strict UTF-8.
- **Windows channel workspaces**: channel workspace directories now escape Windows-invalid channel id characters such as `:`, while preserving the existing non-Windows layout.

## [0.8.13] — 2026-06-06

### Added

- **Blocking `anda agent run` CLI mode**: `anda agent run` now waits for the complete conversation result, follows child conversation chains, supports prompt files, session ids, workspace metadata, request metadata JSON, output JSON files, polling interval control, and optional wait timeouts.
- **Configured daemon workspaces**: `config.yaml` now accepts a `workspaces` list. Configured paths are resolved before the default workspace, sandbox, channels, and skills directories, and duplicate paths are removed.
- **Chrome extension stop control**: the side-panel composer now shows a stop button while a task is sending, submitted, or working, and sends `/stop` without being blocked by the normal prompt-send guard.
- **Chrome extension prompt steering and follow-up cancellation**: queued follow-up prompts can now be cancelled before dispatch, `/new`, `/steer`, and `/stop` commands can bypass busy-state send guards, and stale detached outputs are ignored after a new session starts.

### Changed

- **Terminal quick-start documentation clarified**: README and docsite command references now state that `anda agent run --prompt "..."` waits for the complete result instead of only submitting a request.
- **Chrome extension message surface refreshed**: chat panels, composer controls, markdown blocks, attachments, and external/system/tool bubbles now use shared light/dark message-surface tokens for a cleaner neutral theme.
- **Chrome extension release metadata and dependencies synchronized**: extension manifests and package metadata now advertise `0.8.13`, with frontend dependency updates recorded in the lockfile.

### Fixed

- **WeChat cached context token recovery**: stale cached context tokens are now expired after two hours, session/context-token send failures clear the cached token and retry without it, and context token metadata is refreshed whenever a token is observed.

## [0.8.12] — 2026-06-05

### Added

- **Scoped external-user identity for IM channels**: external IM participants are now identified with channel, discussion space, and sender scope (for example `$external_user:"wechat:agents/room-7/agent-a"`), keeping memory formation and counterparty attribution separated across channels, rooms, and senders.
- **Discussion-space metadata in external-user prompts**: external IM prompts now include `space="..."` in the `$external_user` header when a thread or room is available, making multi-participant WeChat and other channel conversations clearer to the agent.
- **Queued follow-up prompts in the Chrome extension**: when the active conversation is still working, plain follow-up prompts now appear in a dedicated composer queue instead of being inserted into the transcript prematurely; accepted follow-ups are removed once conversation updates arrive.
- **External-user and runtime message rendering in the Chrome extension**: external IM messages now render as distinct external-user bubbles with sender/channel context, while embedded runtime `$system` output is split into standalone tool-output cards instead of being mixed into the user prompt.
- **Repository-wide coding-agent guidance**: added a top-level `AGENTS.md` with project structure, subsystem rules, channel identity invariants, and focused verification commands for coding agents working in this repository.

### Changed

- **WeChat reply/thread normalization**: WeChat messages now trim reply targets, ignore empty session ids, persist context tokens by the normalized reply target, and store source `from`, `session_id`, and `space` metadata for downstream routing and attribution.
- **Chrome extension release metadata synchronized**: the extension manifests and package version now advertise `0.8.12`.

### Fixed

- **Chrome extension prompt visibility during stale syncs**: locally sent idle prompts now stay visible if an older channel sync finishes after the send starts, preventing a fresh prompt from disappearing until the next poll.
- **Chrome extension service-worker edge cases**: install/update handling now uses Chrome's structured `onInstalled` details, development logs redact settings tokens and omit prompt payload bodies, missing management permissions no longer break startup logging checks, and tab id `0` is handled as a valid tab id for side panel and voice actions.

## [0.8.11] — 2026-06-04

### Added

- **Secondary Agent Skills directory support**: the built-in `skills_manager` now loads skills from both the configured Anda skills directory and `~/.agents/skills`, making reusable Agent Skills installed by other agent tooling available without copying them into `~/.anda/skills`.

### Changed

- **Dependencies bumped**: `anda_brain` 0.6.8 → 0.6.9, `anda_core` 0.12.6 → 0.12.7, `anda_engine` 0.12.28 → 0.12.30, plus `anda_cognitive_nexus` 0.7.19 → 0.7.20, `anda_db` 0.7.28 → 0.7.29, `anda_kip` 0.7.13 → 0.7.14, `anda_object_store` 0.3.3 → 0.3.4, `chrono` 0.4.44 → 0.4.45, and `log` 0.4.31 → 0.4.32.
- **Multimodal HTTP fetch test isolation**: the URL media-understanding regression test now uses a proxy-free `reqwest` client, avoiding interference from developer proxy environment variables when fetching the local test server.

## [0.8.10] — 2026-06-04

### Added

- **Cron job origin propagation**: `update_cron_job` tool now accepts `origin=true` to replace the saved `CronJobOrigin` with the current caller and request metadata (user, source, reply target, conversation id, etc.). The engine resolves caller identity from `SessionRequestMeta` and constructs a fresh origin, enabling cron jobs created in one context to be reassigned to another.
- **`origin` field written into cron job database patches**: `cron_job_update_patch` now includes the `origin` column so origin changes persist correctly through `update_job_with_origin`.

### Changed

- **WeChat context token persistence refactored**: the bulk `save_context_tokens` call at the end of the WeChat channel event loop is replaced by per-message `save_context_token_to_workspace` — each incoming message with a `context_token` is merged into the existing token map immediately, removing the race between message receipt and loop-iteration bulk flush.
- **`fallback` label removed from default model config**: the MiniMax-M3 model entry in `config.yaml` no longer carries `labels: ["fallback"]`, aligning with anda_engine v0.12.28's removal of the `fallback` model concept (`fallback` is now just an ordinary label).
- **Dependencies bumped**: `anda_brain` 0.6.7 → 0.6.8, `anda_core` 0.12.5 → 0.12.6, `anda_engine` 0.12.27 → 0.12.28, plus `bitflags` 2.11.1 → 2.12.1, `liteparse` 2.0.4 → 2.0.5, `log` 0.4.30 → 0.4.31, `rustls-native-certs` 0.8.3 → 0.8.4, `unicode-segmentation` 1.13.2 → 1.13.3, `yoke` 0.8.2 → 0.8.3, and `liteparse-pdfium-sys` 1.1.0 → 1.1.1.

## [0.8.9] — 2026-06-03

### Added

- **Native OS folder picker for CLI workspace channels**: new `pick_workspace` WebSocket method opens a native folder picker dialog — `osascript` on macOS, PowerShell `FolderBrowserDialog` on Windows, `zenity` or `kdialog` on Linux. The Chrome extension persists chosen workspace channels to local storage and adds a new "Open folder" button (FolderOpen icon) in the channel sidebar, available even when no channels exist yet.
- **`open_file` browser-launch fallback**: when the Chrome extension lacks `file://` URL access, the engine now falls back to launching the file directly in the native browser application — no configuration required. The result includes a `warning` message guiding users to enable "Allow access to file URLs" for full extension inspection.
- **File URL access detection in the Chrome extension**: `open_tab` and `navigate` actions on `file://` URLs now check `chrome.extension.isAllowedFileSchemeAccess()` before executing. When disabled, the service worker returns a `local_file_access_disabled` error code, enabling the engine's browser-launch fallback path. Both actions have new unit tests verifying the rejection path.

### Changed

- **Browser launch now respects session scope**: `launch_browser()` prioritises the browser matching the active session (chrome, edge, or chromium) across macOS, Windows, and Linux — so file fallback launches and fresh browser sessions open in the same browser the channel is bound to.
- **Channel sidebar title shows real browser name**: sidebar channel titles now display the actual browser (Brave, Edge, Opera, Vivaldi, Arc, etc.) via `titleCase()` instead of the previous hardcoded "Chrome" / "Incognito" labels.
- **CLI workspace sent in channel metadata**: `cli:` channel sources now parse and forward their workspace path to the engine via `requestExtra`, giving the engine workspace context for CLI-originated channels opened in the extension.
- **`error_code` propagation in browser action results**: `BrowserActionResult` now carries an optional `error_code` field. The service worker propagates `BrowserActionError.code` to WebSocket responses, letting the engine handle specific error classes (e.g. fallback on `local_file_access_disabled`) instead of treating all failures the same way.
- **Docsite updated**: added a note to the browser extension quick-start page explaining how to enable `Allow access to file URLs` for `open_file` functionality with local files and folders.

## [0.8.8] — 2026-06-01

### Added

- **Cross-browser detection in Chrome extension**: new async `getCurrentBrowser()` in `service-worker/chrome.ts` that uses User-Agent Client Hints (`navigator.userAgentData.getHighEntropyValues(['brands'])`), Brave's `navigator.brave.isBrave()`, and UA string matching to return `chrome | edge | brave | opera | vivaldi | arc | chromium`. Powers the new session scoping below.
- **Window id in side-panel tab payload**: `requestExtra` now includes the active tab's `windowId`, letting the engine reason about and correlate tabs across multi-window Chrome sessions.

### Changed

- **Media size limits tightened**: `MAX_MEDIA_FILE_SIZE_BYTES` reduced from 25 MB to 10 MB and `MAX_OTHER_TEXT_SUMMARY_BYTES` from 512 KB to 256 KB — keeps a single attachment well under the per-request model context budget.
- **Browser session scope now names the browser**: `browserSessionScope` returns `chrome | incognito_chrome | brave | incognito_brave | edge | ...` (was a binary `chrome | incognito`). Isolates persistent session state across browsers, so switching from Chrome to Brave no longer collides with the prior session.
- **Active model resolution in `other_understanding` agent**: the agent call now sets `model: Some(ACTIVE_MODEL_LABEL.into())` (empty string) instead of relying on the engine default — the active model label is picked from runtime config the same way the main agent does, keeping routing consistent.
- **`requestExtra` always refreshes active tab**: the conditional `if (!this.tab) await this.refreshActiveTab()` was replaced with an unconditional refresh, so the engine always sees the tab the user is actually looking at, not a stale cache from a previous focus.
- **Other media agent error string uses constant name**: the "requires an attached resource..." error is now formatted with `OTHER_UNDERSTANDING_AGENT_NAME` for consistency with other agent error paths.

### Fixed

- **`rememberActiveTab` null/undefined handling**: switched the guard from `=== null || === undefined` to a single `== null` check, so the remembered tab id is correctly cleared on any "no value" input. The previous branch missed `undefined` in some callers and let stale ids linger.

### Removed

- **Pre-populated `initial_messages` in new conversations**: the engine no longer injects a synthetic user message containing the prompt into a fresh `Conversation`'s `messages` field — the prompt is still sent to the LLM, but the conversation record now starts with an empty `messages` array, matching what the rest of the engine actually observes.
- **"Active sessions" hint from browser tool description**: the live `Vec<BrowserSession>` debug dump was appended to `description()` on every call, polluting the LLM's tool prompt. The hint is gone; the description now contains only the stable tool contract.
- **Redundant `max_output_tokens: Some(2048)` on multimodal LLM calls**: removed from both the `other_understanding` and other media summary calls — letting the active model's default token budget apply.
- **`requestMeta()` call on the extension prompt path**: the channel no longer pre-fetches meta before `agentRun`, and `meta` is no longer threaded through the `agentRun` argument list.

## [0.8.7] — 2026-06-01

### Added

- **URL and data-URL support for media understanding agents**: `url` parameter alongside existing `path` in `MediaUnderstandingArgs` — supports `http`/`https`/`data` URLs, streaming HTTP responses with size limit enforcement, base64 and percent-encoded data URL decoding. Shared `reqwest::Client` injected via `with_http_client` builder.
- **Resource persistence and attachment handling**: new `ResourceStore` for persisting resources (images, files) in the database. Integrated into the AndaBot engine with full attachment lifecycle management — from media understanding through to chat message display.
- **Attachment download in Chrome extension**: one-click download for message attachments directly from the chat UI, with progress tracking and concurrent download management.
- **Multi-format attachment support**: new `Other` media type for non-image/audio/video attachments — supports text, PDF, document, spreadsheet, and other file formats. `liteparse` integration for PDF parsing.
- **Pending local attachment sync**: Chrome extension synchronizes locally queued attachments with the server on reconnect, ensuring no attachment is lost during offline usage.
- **Resource API tool**: `resources_api` tool exposed to agents for loading persisted resources by ID, with caching and lazy loading in the Chrome extension.
- **Rich text copy in Chrome extension**: `copyRichMessage()` copies both plain text and rendered HTML to the clipboard via `ClipboardItem`, preserving formatting when pasted into rich-text editors.
- **Print message in Chrome extension**: `printMessage()` opens a styled print window with rendered Markdown content and image attachments, ready for printing or saving as PDF.
- **Comprehensive unit tests for Rust core modules**: 300+ new tests covering attachments, channel types, session formatting, channel/transcription/TTS config, daemon paths, logger, and Ed25519 key operations — all following Rust testing conventions.

### Changed

- **Engine model resolution via active model label**: `ACTIVE_MODEL_LABEL` (`""`) constant.
- **MIME detection improved**: media MIME detection now prefers inferred media MIME, then `Content-Type` header, then filename extension, with a normalized fallback chain for all media understanding paths.
- **Chrome extension message actions refactored**: shared `messageActionButtonClass` extracted; action button row gains Copy, Clipboard, and Printer icons, with consistent hover/focus animations.
- **MiniMax model upgraded to M3**: default config swaps `MiniMax-M2.7` (200K context, `lite` label) for `MiniMax-M3` (400K context, `flash`/`memory`/`image`/`video` labels) — doubles context window and unlocks image, video, and memory-class routing on the MiniMax provider.

### Fixed

- **Musl build compatibility**: `liteparse` default features disabled at workspace level; `tesseract` OCR feature conditionally enabled only on non-musl targets. PDF OCR automatically disabled on musl targets via `cfg!(target_env = "musl")`, enabling static musl builds (Alpine Linux) that previously failed due to unavailable tesseract system libraries.
- **Workspace derivation from source prefix**: when only the `source` field is provided (e.g. from browser extension), extract workspace from the `"cli:"` prefix. Conversations created via non-CLI channels now correctly resolve a valid workspace directory.
- **Browser action active tab tracking**: `get_current_tab` and `list_tabs` no longer return stale data — a remembered active tab id is updated on every `tabs.onActivated`, `tabs.onUpdated`, `windows.onFocusChanged`, and `chrome.action.onClicked` event, so `activeTab()` and per-window `active_tab_id` resolution stay in sync with the real focused tab across multi-window sessions.
- **`type_text` verification and scripted fallback**: native `Input.insertText` is now followed by a `verifyNativeTextInput` check that compares the actual input value to the expected text; when verification fails the action returns a `null` result that triggers a `chrome.scripting.executeScript` fallback so typed text is never silently lost. The result also exposes a `verified` field for caller introspection. New test covers the scripted fallback path.
- **Browser action request serialization**: `service_worker.ts` now queues `browser_action` requests through `queueBrowserActionRequest`, ensuring only one browser action runs at a time and the active tab context cannot drift between adjacent calls.
- **Viewport dimension validation**: `validate_viewport_options` no longer requires `viewport_width` when only `device_scale_factor` is provided — width/height are only enforced as a pair, and `device_scale_factor` alone is now accepted. Two new unit tests cover the relaxed contract.
- **Clipboard write timeout**: `copy_to_clipboard` now races `navigator.clipboard.writeText` against a 2 s timeout so a hung clipboard write no longer blocks the whole extension message loop.
- **Generic script-result error**: when a script-mode action returns no `result` the error message now reflects the actual action name (not just `execute_javascript`), surfacing the failure for `click`/`type_text`/`scroll_to`/etc. as well.

### Dependencies

- `anda_brain` 0.6.5 → 0.6.7.
- `anda_core` 0.12.4 → 0.12.5.
- `anda_engine` 0.12.24 → 0.12.26 (`Models::resolve()` simplification — delegates to `get_model()`).
- `anda_db_tfs` 0.1.0 (new, for resource tokenization).
- `liteparse` 2 (new, for PDF parsing).

## [0.8.6] — 2026-05-29

### Added

- **StepFun step-3.7-flash model**: new `step-3.7-flash` entry in `config.yaml` (StepFun API, 256K context, 32K max output, labels: flash/image/video).
- **`effort: high` default for all models**: every model entry in `config.yaml` now specifies `effort: high`, aligning with the brain default `ModelEffort::High`.

### Changed

- **Completion hook reads `SessionRequestMeta` first**: `completion_meta()` now checks agent context state for `SessionRequestMeta` before falling back to `ctx.meta()`, restoring correct cron job route recovery when the runtime meta lacks route info.
- **Direct async send replaces `tokio::spawn`**: `on_completion` now awaits `try_send` directly instead of spawning a detached task, simplifying error handling and ensuring channel bindings are visible immediately.
- **`completion_message` takes `&RequestMeta`**: decoupled from `&AgentCtx`, making it easier to test and reuse.

### Fixed

- **TUI test**: `ContentPart::ToolOutput` in `tui/mod.rs` test now includes `is_error: None` to match the updated struct definition.

### Dependencies

- `anda_brain` 0.6.4 → 0.6.5.
- `anda_cognitive_nexus` 0.7.18 → 0.7.19.
- `anda_core` 0.12.3 → 0.12.4.
- `anda_engine` 0.12.23 → 0.12.24.
- `anda_kip` 0.7.12 → 0.7.13.
- `hyper` 1.9.0 → 1.10.0.
- `candid` 0.10.28 → 0.10.29.
- `zerocopy` 0.8.48 → 0.8.49.
- `displaydoc` 0.2.5 → 0.2.6.
- `socket2` 0.6.3 → 0.6.4.
- `mio` 1.2.0 → 1.2.1.
- `cmov` 0.5.3 → 0.5.4.

## [0.8.5] — 2026-05-27

### Added

- **MinerU document extraction skill**: new `mineru-document-extractor` skill for converting PDFs, scanned documents, images, Office formats (Word DOC/DOCX, PowerPoint PPT/PPTX, Excel XLS/XLSX), and web pages into clean Markdown, HTML, LaTeX, or DOCX via the official `mineru-open-api` CLI. Two extraction modes: flash-extract for instant zero-config conversion (no token, table/formula recognition, OCR) and precision extract with VLM-based layout analysis and batch processing. Covers 80+ languages including Chinese, English, Japanese, Korean, and Arabic.

- **`update_cron_job` tool**: new tool for modifying existing cron jobs without losing their origin metadata or run history. Supports partial updates — pass `null` for fields that should stay unchanged. Changing the schedule recalculates `next_run`; updating only non-schedule fields (job, name) preserves the existing schedule.
- **Cron run ID tracking**: `run_id` is now carried through the full cron execution pipeline — agent job prompts, shell job result notifications, and all log messages — making it straightforward to correlate a specific execution with its conversation and error traces.


## [0.8.4] — 2026-05-25

### Changed

- **Browser script action simplified**: `action` is now optional on `chrome_script` -- defaults to `execute_javascript` implicitly, removing the redundant enum from the tool schema. Models can pass `code` directly.
- **Browser result deduplication**: `tab` is no longer nested inside `page_ready` in browser action results. All navigation actions (`open_tab`, `navigate`, `reload`, `go_back`, `go_forward`, `switch_tab`) use a shared `withTopLevelTab` helper, and `compactPageReadyInfo` ensures `page_ready` never carries duplicate tab data. Updated test assertions to match.
- **`wait_for_navigation` and `wait_for_history_change` removed**: navigation and page-changing actions now rely entirely on built-in `page_ready` (included in `navigate`, `open_tab`, `open_file`, `reload`, `go_back`, `go_forward`, and page-changing `click`/`press_key` actions). The explicit wait actions were redundant and prone to timing races with fast navigations. Removed `expected_url` and `wait_until` fields from `BrowserActionArgs`.

## [0.8.3] — 2026-05-25

### Changed

- **Lightweight UI replaces shadcn-svelte**: removed `shadcn-svelte` and `tailwind-variants` dependencies. 175+ component library files replaced with a single `ui.ts` module exporting shadcn-compatible class generator functions (`buttonClass`, `badgeClass`, `itemClass`, `cardClass`, `inputClass`, `dialogContentClass`, etc.) using Tailwind classes directly. All Anda app components (`App`, `ChatComposer`, `ChatMessageItem`, `ChatSettings`, `AttachmentList`, `PromptCommandPanel`, `VoicePanel`) refactored accordingly. Net deletion: ~3,900 lines.

### Fixed

- **Table rendering**: tables now use `width:max-content` with `min-width:100%` instead of fixed `100%` with `overflow:hidden`, enabling proper responsive behavior.
- **Message spacing**: grid gap increased from 2 to 4 for better visual separation.
- **MiniMax model name**: corrected from `M2.7-highspeed` to `M2.7` in default config.

## [0.8.2] — 2026-05-24

### Highlights

- **shadcn-svelte UI**: the Chrome extension UI has been rebuilt on the shadcn-svelte component library — 30+ component families (accordion, alert, avatar, command, context-menu, dialog, dropdown-menu, field, hover-card, input-group, item, kbd, native-select, popover, select, skeleton, spinner, switch, tabs, tooltip) replacing ad-hoc Svelte markup across ChatComposer, ChatMessageItem, ChatSettings, ChatChannelsSidebar, AttachmentList, PromptCommandPanel, and VoicePanel.
- **$skill-name shorthand**: `$skill-name prompt` is now equivalent to `/skill skill-name prompt` — quick skill routing without typing the leading slash. Both the Rust engine and the Chrome extension completion panel handle the dollar prefix.
- **open_file action**: `chrome_tabs` can now open local files via `file://` URLs. Paths are resolved relative to the workspace and percent-encoded for URLs containing spaces and special characters. Returns metadata including the mime type guessed from file extension.
- **Viewport screenshot controls**: `chrome_page.screenshot` now accepts `viewport_width`, `viewport_height`, and `device_scale_factor` — fixed-size screenshots via CDP device metrics emulation, independent of the browser window size.

### Changed

- **Dependencies**: version 0.8.1→0.8.2.

## [0.8.1] — 2026-05-23

### Browser Extension

- **Debugger reconnection**: commands transparently retry up to 2× on transient "not attached" errors; removed the unconditional detach-before-attach that caused excessive detach calls.
- **Native text input**: `type_text` on editable elements now uses CDP `Input.insertText` (selectAll → Backspace → insert) instead of key-by-key dispatching — faster and more reliable.
- **Mobile touch support**: click dispatches touchStart/touchEnd on pages with mobile user agents or `maxTouchPoints > 1`, matching real device behavior.
- **goBack/goForward fallback**: when the native tabs API throws (e.g. localized errors like "无法在历史记录中找到下一页"), falls back to injecting `pageActionDispatcher` via `scripting.executeScript`.
- **Smarter element resolution**: `deepQuerySelector` uses `preferredMatch()` to select the most interactable element from all `querySelectorAll` matches — `interactable()` checks visibility + `elementFromPoint` hit-test at element center, with fallback through `visible()`, then first match.
- **Key aliases**: Esc→Escape, Return→Enter, Space→Space; added Space key definition.
- **`preferredMatch` for `type_text`**: prefers editable text inputs over other visible elements; falls back to `document.activeElement` when no selector is provided.

### Engine

- **GitHub API version checking**: auto-updater now queries `api.github.com/repos/{REPO}/releases/latest` with anti-cache headers, falling back to the releases page when the API is unavailable.
- **`/stop` stores failed_reason**: the prompt is always stored as `failed_reason` (not just when non-empty) and appended to the conversation as a user message before cancellation.

### Changed

- **Dependencies**: `anda_brain` 0.6.1→0.6.2, `anda_engine` 0.12.19→0.12.20, `wasm-bindgen` 0.2.121→0.2.122, `serde_json` 1.0.149→1.0.150, `lucide-svelte`, `tailwindcss`, `svelte`, `vite`, `vitest`, `katex`, `chrome-types`, `postcss`, `prettier-plugin-svelte`.

## [0.8.0] — 2026-05-22

The Chrome Extension release — Anda Bot now lives in your browser.

### Highlights

- **Chrome Extension (Anda Bot)**: a full-featured browser side panel that connects to the Anda Bot daemon via WebSocket. Chat with your agent, manage conversations and channels, browse files, run slash commands, and control browser tabs — all without leaving Chrome. Published to the Chrome Web Store.
- **Browser Tools**: four focused tools (`chrome_tabs`, `chrome_page`, `chrome_input`, `chrome_script`) give the agent complete control over the browser — navigation, page inspection, screenshots, accessibility trees, PDF printing, form interaction, JavaScript execution, downloads, cookies, cache management, viewport annotations, file uploads, and dialog handling.
- **Auto-Update System**: end-to-end self-update spanning daemon, TUI, Chrome extension, and gateway — checks GitHub releases, downloads platform assets, verifies SHA256 checksums, and installs with restart.
- **Multimodal Media Understanding**: image, video, and audio understanding via model-label routing, with automatic model dispatch based on `image_understanding`, `video_understanding`, or `audio_understanding` labels.
- **Voice in the Browser**: voice input/output, thinking detail display, and voice orb UI in the extension side panel.

### Chrome Extension — Features

- **Side panel UI**: SvelteKit-based side panel with chat, channels, tasks, and settings views.
- **Multi-conversation history**: browse, switch, and manage conversations with real-time polling and local message reconciliation.
- **Channel management**: create, switch, and delete channels with alert dialog confirmation.
- **Slash command palette**: `/new`, `/stop`, and skill commands accessible via keyboard shortcut.
- **Voice I/O**: speech-to-text input, text-to-speech playback, and voice orb visualization.
- **Thinking display**: expandable reasoning traces with configurable detail level.
- **Submit key modes**: choose between "Enter sends" or "Ctrl/Cmd+Enter sends" in settings.
- **Onboarding flow**: guided first-run experience for new users.
- **i18n**: 6 languages — English, Chinese (Simplified), Russian, Arabic, French, Spanish.
- **Runtime model switching**: model selector with live daemon model list and refresh.
- **Auto-update UI**: notification banner when an update is downloaded, with install-and-restart button.
- **WebSocket transient recovery**: automatic reconnection on transient network errors.
- **Vitest test suite**: unit tests for polling, side panel, and voice modules.

### Browser Tools — Capabilities

- **Tab management**: list, open, close, switch, navigate, go back/forward, reload.
- **Page inspection**: snapshot (with links/forms), extract text, screenshot (viewport and full-page), get full HTML, accessibility tree, find in page, viewport size.
- **Viewport annotations**: highlight elements visually via CDP Overlay for annotated screenshots.
- **PDF printing**: generate PDFs from any tab via CDP `Page.printToPDF`.
- **Input interaction**: click, type, press key, scroll, scroll-to (selector or coordinates), hover, drag-and-drop, select dropdown, upload files, copy to clipboard.
- **JavaScript execution**: CSP-resistant execution via Chrome Debugger API bridge, supporting both isolated and main worlds.
- **Downloads**: trigger downloads, list active downloads with state filtering, cancel, open completed.
- **Cookies**: get, set, delete with full attribute support (domain, path, secure, httpOnly, sameSite, expiration).
- **Cache management**: clear browsing data (cache, cacheStorage, indexedDB, localStorage, service workers) with optional origin filtering.
- **Dialog handling**: accept or dismiss JavaScript alerts/confirms/prompts with optional prompt text.
- **Screenshot materialization**: screenshots auto-saved to disk and paths injected into results for downstream `image_understanding`.

### Engine

- **Multimodal media understanding**: new `multimodal.rs` module (894 lines) handles media resource extraction, model dispatch by label, and content injection into the system prompt.
- **OpenAI strict mode compliance**: all tool schemas pass OpenAI strict validation — `additionalProperties: false`, optional fields use `["type","null"]`, no unsupported keywords. Added schema-validation tests.
- **Codex OAuth token auto-loading**: reads `~/.codex/auth.json` and injects `access_token` when `api_base` points to Codex backend.
- **Datetime context injection**: RFC 3339 `Current datetime` field in implicit context for temporal awareness.
- **ContentPart migration**: steering and follow-up messages use `Vec<ContentPart>` instead of string concatenation, enabling multimodal content passthrough.
- **Hippocampus renamed to Brain**: all internal modules, file paths, i18n, and documentation updated to align with the `anda-brain` standalone crate.
- **Conversations API**: status tracking, agent-aware formatting, and enhanced metadata.
- **Cron metadata refactored**: `cron_job_id` replaces full job content string, reducing payload size.
- **System extra content injection**: structured `ContentPart` with `[$system: ...]` prefix replaces ad-hoc string formatting.
- **Compaction threshold simplified**: 80% of context window with 100K token minimum.
- **Config context windows**: reduced from 1,000,000 to 400,000 to match actual provider limits.

### Auto-Update System

- **Daemon**: `AutoUpdater` checks GitHub releases API, downloads platform-specific assets, verifies SHA256 checksums, installs binaries, and restarts.
- **REST endpoints**: `/auto_update`, `/auto_update/check`, `/auto_update/install_and_restart` with bearer-token auth.
- **WebSocket RPC**: `auto_update_status`, `auto_update_check`, `auto_update_install_and_restart` for the extension.
- **TUI integration**: async background check on chat init, notice banner in status area.
- **Extension UI**: amber notification banner with version tag and install button.
- **State machine**: persistent update state in `AndaDB` across daemon restarts.

### CLI & TUI

- **`/new` command**: `/new [prompt]` starts a fresh conversation across CLI, TUI, and extension. Stale conversation detection prefixes output with `[Previous conversation #N]`.
- **TUI scrollback purge**: `ClearType::Purge` clears terminal scrollback on `/new`.
- **Website pages**: privacy, terms, and support pages added.

### Changed

- **Browser tool split**: monolithic `chrome_browser` replaced by four focused tools with minimal schemas and independent timeout handling.
- **Chrome extension modularized**: `client.ts` (2,099 lines) split into channel, side-panel, conversations, polling, voice, types, commands, and chrome modules. `service_worker.ts` (1,884 lines) split into browser-actions, speech, voice, audio, TTS, settings, and types.
- **Manifest permissions**: `browsingData`, `cookies`, `downloads`, `webNavigation` added.
- **Dependencies**: `anda_brain` 0.6.0→0.6.1, `anda_core` 0.12.2→0.12.3, `anda_engine` 0.12.16→0.12.19, `weixin-agent` 0.1.0→0.2.0 (git→crates.io).

### Stats

- 198 files changed, +25,377 / -1,913 lines
- 32 commits across 8 patch releases (0.7.1 → 0.7.8)
- Chrome extension: ~5,000+ lines of Svelte/TypeScript
- Browser tools: ~3,000+ lines across Rust backend and TypeScript service worker

## [0.7.8] — 2026-05-22

### Added

- **Browser downloads**: `download`, `list_downloads`, `cancel_download`, and `open_download` actions added to `chrome_tabs`, enabling the agent to download files, list active downloads with state filtering, cancel in-progress downloads, and open completed downloads through Chrome. Types: `ChromeDownloadItem` with fields for id, url, filename, state, bytes, and timestamps.
- **Browser cookies**: `get_cookies`, `set_cookie`, and `delete_cookie` actions for full cookie management through the Chrome extension. Supports domain, path, secure, httpOnly, sameSite, expirationDate, and storeId fields. Types: `ChromeCookieInfo` and `ChromeCookieSameSite`.
- **Browser cache clearing**: `clear_browser_cache` action via `chrome.tabs` — accepts optional `since_ms` Unix timestamp and `origins` array to selectively clear cache, cacheStorage, indexedDB, localStorage, and service workers through `chrome.browsingData.remove`.
- **Page PDF printing**: `print_to_pdf` action (`chrome_page`) generates PDFs from the active tab via CDP `Page.printToPDF`. Data URL handling extended to accept `application/pdf` MIME type with `.pdf` file extension.
- **Accessibility tree**: `get_accessibility_tree` action (`chrome_page`) returns the page's accessibility tree via CDP `Accessibility.getFullAXTree`, configurable node limit (default 500).
- **Viewport annotations**: `annotate_viewport` and `clear_annotations` actions (`chrome_page`) for visual element highlighting in screenshots via CDP `Overlay` domain.
- **File upload**: `upload_file` action (`chrome_input`) uploads local files through file input elements via CDP `DOM.setFileInputFiles`. Validates non-empty file paths.
- **Dialog handling**: `handle_dialog` action (`chrome_page`) accepts or dismisses JavaScript dialogs (alert/confirm/prompt) via CDP `Page.handleJavaScriptDialog`, with optional `prompt_text` for prompt dialogs.
- **Scroll-to coordinates**: `scroll_to` now accepts viewport x/y coordinates in addition to CSS selectors, matching the `scroll_to` behavior when targeting a specific screen position.
- **TypeText without selector**: `type_text` now works without a selector — when omitted, types into the currently focused/active element, enabling keyboard-focused workflows.
- **Full-page screenshot**: `screenshot` action now supports `full_page` parameter to capture the entire scrollable page instead of just the viewport.
- **New extension permissions**: `browsingData`, `cookies`, `downloads`, and `webNavigation` added to manifest for the new capabilities.
- **Navigation-ready loading**: `goBack`, `goForward`, and `reload` now wait for the page to finish loading before returning, matching the behavior of `navigate`.
- **WebNavigation and debugger events**: `chrome.webNavigation.onCommitted`/`onCompleted` and `chrome.debugger.onEvent` typed in `ChromeApi` for reliable page-ready detection after navigation.

### Changed

- **Cargo.toml version**: bumped from 0.7.7 to 0.7.8.
- **Tool descriptions updated**: `chrome_tabs` description now mentions downloads, cookies, and cache; `chrome_page` mentions accessibility tree, PDF, annotations; `chrome_input` mentions upload_file.
- **`select_dropdown` extended**: description now notes it can target same-origin iframes containing selects, matching the improved iframe-handling in the service worker.
- **browser-actions.ts (+2002 lines)**: service worker refactored with page-ready waiting (`waitForPageReady`), CDP-based actions (printToPdf, accessibility tree, annotations, dialog handling, upload file), downloads/cookies/cache management, and ArrowCaster mappings for all new BrowserAction values.

## [0.7.7] — 2026-05-22

### Added

- **Runtime model switching in Chrome extension**: the Chat Settings panel now shows a model selector dropdown with available models fetched from the daemon, plus a refresh button. Switching the active model happens live without restarting the daemon — backed by new `model_names` and `set_model` WebSocket RPC methods in `browser_ws.rs`.
- **Model state TypeScript types**: added `ModelState` and `DaemonModelState` interfaces to the extension client, with `normalizeModelState` deduplication.
- **i18n strings for model UI**: `activeModel`, `refreshModels`, `modelListEmpty`, and `modelUpdated` added to all 6 supported locales.

### Fixed

- **WebSocket poll transient-error handling**: `isTransientWebSocketError` now correctly returns `true` for poll failures (previously `false`), so polling retries rather than bailing on transient disconnects. The detection pattern was also broadened to catch more transient WS error messages (timeout, disconnected, etc.).

### Changed

- **Model state refreshed on connection lifecycle**: the extension fetches model state after init, settings save, and connection test — clearing model state when the token is removed.

- **Auto-update system**: full self-update mechanism spanning daemon, TUI, Chrome extension, and gateway. The `AutoUpdater` checks the GitHub releases API for new versions, downloads the correct platform asset, verifies SHA256 checksums, and can install the new binary and restart the daemon — all through a persistent state machine stored in `AndaDB`. Backend exposes three REST endpoints (`/auto_update`, `/auto_update/check`, `/auto_update/install_and_restart`) with bearer-token auth, plus equivalent WebSocket RPC methods (`auto_update_status`, `auto_update_check`, `auto_update_install_and_restart`) for the browser extension.
- **Auto-update CLI updater refactor**: `ReleaseTarget`, `UpdateFinish`, and constants promoted to `pub(crate)` so the auto-updater reuses the same platform detection, asset naming, and download logic as the manual `anda update` CLI command.
- **Auto-update Chrome extension UI**: an amber notification banner appears in the side panel when an update is downloaded, showing the latest version tag and an "install & restart" button with a confirmation dialog. Update state is refreshed on init and after settings save — cleared when the token is removed.
- **Auto-update TUI integration**: the TUI fires an async auto-update check on chat init and displays a notice banner in the status area when an update is available, using `oneshot` channels for non-blocking background checks.
- **Daemon DB helpers**: `bot_db_config()`, `connect_bot_db()`, and `open_bot_db()` extracted as public methods on `Daemon`, enabling the auto-updater and future subsystems to share the same bot database.
- **Extension client `AutoUpdateState` types**: TypeScript interfaces for `AutoUpdateStatus` and `AutoUpdateState` with all fields (status, current_tag, latest_tag, SHA256, checksum_verified, etc.).
- **i18n strings for update UI**: `updateReadyTitle`, `updateReadyBody`, `installRestartUpdate`, `updateRestartConfirm` added to all 6 supported locales.
- **Cli updater takes `&Daemon`**: `cli::updater::run()` now accepts `&Daemon` instead of `home_dir: &Path`, giving the updater access to daemon-level utilities for the shared update logic.


## [0.7.6] — 2026-05-21

### Changed

- **OpenAI strict mode compliance for all tool schemas**: every tool function now passes OpenAI strict validation — all properties listed in `required` (optional fields use `["type","null"]`), `additionalProperties: false` on all objects, no unsupported schema keywords. Added `json_schema.rs` test utility with `assert_openai_strict_parameters` and schema-validation tests covering brain client, cron tools, browser tools, conversation API, goal tool, multimodal agents, transcription, and TTS.
- **Dependency bumps**: `anda_brain` 0.6.0→0.6.1, `anda_core` 0.12.2→0.12.3, `anda_engine` 0.12.16→0.12.19.
- **Config context windows reduced**: memory-capable model context windows lowered from 1,000,000 to 400,000 to align with actual provider limits.
- **Model rename**: `gpt-5.4-mini` → `gpt-5.4`.
- **Codex token loading moved into `Config::from_file`**: previously handled in `main.rs`, now applied consistently at config load time so all callers benefit.
- **Compaction threshold simplified**: switched from half-window-with-clamps to 80% of context_window with 100K token minimum, providing more predictable compaction behavior.
- **Removed `max_output_tokens` overrides** from agent completion configs — now relying on provider/model defaults.
- **Removed `minLength: 1`** from TTS text parameter schema (not allowed under OpenAI strict mode).

## [0.7.5] — 2026-05-21

### Changed

- **Hippocampus renamed to Brain**: all internal module names, file paths, docsite pages, i18n translations, and documentation references renamed from `hippocampus` to `brain` — aligning with the standalone `anda-brain` rename.


## [0.7.4] — 2026-05-21

### Changed

- **Current datetime injected into engine context**: the `extra_user_context` now includes an RFC 3339-formatted `Current datetime` field at the top of the implicit context, giving agents accurate temporal awareness without relying on training-cutoff heuristics.
- **Cron job metadata refactored**: `cron_job` (full job content as a string) replaced with `cron_job_id` (u64) in request metadata, reducing serialized payload size. The job name now falls back to the numeric ID when `cron_job_name` is empty. Cron job content is no longer echoed in completion messages.
- **Stop/New command handling reordered**: `PromptCommand::Stop` now breaks the prompt loop immediately without processing any remaining commands. `PromptCommand::New` is logged as unexpected (it should be handled at the agent level in `run()`, not in the session runner) and no longer injects its prompt as follow-up content.
- **Session Working status auto-repair**: when a conversation status is not `Working` but the runner has pending tasks (`!is_idle()`), the status is now persisted as `Working` with `failed_reason` cleared — recovering from stale status states without manual intervention.
- **Sidebar channel toggle via Button**: the collapsed/expanded chevron icon in the channel sidebar is now wrapped in a shadcn-svelte `Button` with `aria-label` and `title`, replacing an unlabeled clickable div.


### Added

- **Codex OAuth token auto-loading**: when a model provider is configured with `api_base: "https://chatgpt.com/backend-api/codex"`, the daemon now automatically reads `~/.codex/auth.json` and injects the `access_token` as `api_key`. This enables seamless Codex backend usage without hardcoding tokens in config — just log in once and the daemon picks up the token on restart.
### Fixed

- **Duplicate `scrollIntoView` calls**: the App.svelte `$effect` now tracks `prevLastMessageId` to only trigger `scrollIntoView` when the last message ID actually changes, preventing redundant scroll animations on unrelated reactivity triggers.

## [0.7.3] — 2026-05-20

### Added

- **Multimodal media understanding**: the engine now supports image, video, and audio understanding via model-label routing. A new `multimodal.rs` module (894 lines) handles media resource extraction, model dispatch, and content injection into the system prompt. Models with `image_understanding`, `video_understanding`, or `audio_understanding` labels are automatically selected for the corresponding media types.
- **Screenshot materialization**: browser screenshots taken via the Chrome extension now have their `data_url` automatically decoded and saved to disk under `browser-screenshots/`. The saved file path is injected back into the action result, making screenshots immediately consumable by downstream tools like `image_understanding` without a separate download step.
- **CSP-resistant JavaScript execution**: introduced `chrome_script` with a Chrome Debugger API bridge (`debugger` world) that bypasses Content Security Policy restrictions, replacing the previous `chrome.scripting.executeScript` approach that failed on CSP-strict pages (e.g., GitHub, X).
- **Browser tool split**: the monolithic `chrome_browser` tool was split into four focused, single-responsibility tools — `chrome_tabs` (navigation/tab management), `chrome_page` (inspection/screenshot/extraction), `chrome_input` (click/type/scroll/interaction), and `chrome_script` (JavaScript execution). Each tool has a minimal schema, clearer error messages, and independent timeout handling. The legacy tool and `ChromeBrowserToolKind::Legacy` variant have been fully removed.
- **Alert dialog component**: a full `AlertDialog` shadcn-svelte component family (overlay, content, header, title, description, footer, cancel, action) added to the Chrome extension UI library, enabling confirmation dialogs for destructive actions.
- **Channel deletion with confirmation**: users can now delete channels via the UI with an alert dialog confirmation step, preventing accidental data loss.
- **Side tasks panel**: the Chrome extension sidebar was restructured into a `SidePanel` component with a tab-based layout separating chat channels from side tasks, improving navigation and extensibility.
- **I18n audit tool**: a new `scripts/check-i18n.mjs` (558 lines) validates i18n coverage across all 6 supported locales, detecting missing keys, untranslated messages, and stale entries.
- **Vitest infrastructure**: Chrome extension client logic now has a proper test framework with `vitest.config.ts`, initial test suites for `poll-conversation`, `side-panel`, and `voice` modules, and extracted pure functions for testability.
- **WeChat login status handling**: three new `LoginStatus` variants are now handled — `NeedVerifyCode` (pair-code verification required on phone), `VerifyCodeBlocked` (too many wrong verification codes), and `BindedRedirect` (bot already bound to this instance, no new credentials issued).

### Changed

- **weixin-agent promoted to crates.io**: `weixin-agent` 0.1.0 → 0.2.0, moved from a git dependency (`[patch.crates-io]`) to the public crates.io registry. The `[patch]` entry has been removed from the workspace `Cargo.toml`.
- **Agent system instructions**: `render_system_instructions()` now uses named format parameters (`{ins}`, `{knowledge}`, `{notes}`, etc.) instead of positional `{}` placeholders, improving readability and reducing argument ordering bugs.
- **Chrome extension decomposed**: `client.ts` (2,099 lines) split into focused modules — `channel.svelte.ts` (561 lines), `side-panel.svelte.ts` (653 lines), `conversations.ts`, `poll-conversation.ts`, `voice.ts`, `types.ts`, `commands.ts`, `chrome.ts`. `service_worker.ts` (1,884 lines) similarly decomposed into `browser-actions.ts` (1,046 lines), `page-speech.ts`, `page-voice.ts`, `page-audio.ts`, `tts.ts`, `settings.ts`, and `types.ts`.
- **Legacy `chrome_browser` tool removed**: the original monolithic browser tool has been fully removed from both `browser.rs` and `engine.rs`, leaving only the four split tools (`chrome_tabs`, `chrome_page`, `chrome_input`, `chrome_script`).


### Added

- **Codex OAuth token auto-loading**: when a model provider is configured with `api_base: "https://chatgpt.com/backend-api/codex"`, the daemon now automatically reads `~/.codex/auth.json` and injects the `access_token` as `api_key`. This enables seamless Codex backend usage without hardcoding tokens in config — just log in once and the daemon picks up the token on restart.
### Fixed

- **CSP bypass result extraction**: the Debugger bridge's `Runtime.evaluate` results were not properly unwrapped when the evaluated expression returned an object handle rather than a value, causing `chrome_script` output to be empty on certain pages. Now correctly inspects and extracts object properties via the RemoteObject protocol.
- **Debugger concurrency**: multiple overlapping `chrome_script` calls could collide on the single DevTools debugger session. Fixed by serializing bridge calls through a per-tab mutex.
- **Agent session/ancestor handling**: `spawn_session_runner` now correctly owns the media understanding step (previously duplicated at 3 call sites), and the session runner content filter excludes stale inline data from follow-up messages.
- **Windows PowerShell key**: corrected a `ctrl+shift+p` → `ctrl+shift+P` casing issue that prevented the command palette shortcut from working on Windows.

### Dependencies

- `crypto-common` 0.2.1 → 0.2.2
- `typetag` 0.2.21 → 0.2.22
- `weixin-agent` 0.1.0 → 0.2.0 (git → crates.io)

## [0.7.2] — 2026-05-17

### Added

- **Submit key mode setting**: users can choose between two input modes in Chrome extension settings: "Enter sends" (Shift+Enter for newlines, the new default) or "Ctrl/Cmd+Enter sends" (Enter for newlines). Configurable via a radio button group in the settings panel. Persisted to `chrome.storage.local` alongside `baseUrl` and `token`.
- **Local message reconciliation**: when a locally drafted message is confirmed by the server, `reconcileLocalMessages()` merges the server-side message with the local draft instead of appending a duplicate. This prevents ghost drafts from appearing after round-trips.
- **WebSocket transient error recovery**: `isTransientWebSocketError()` detects `WebSocket connection closed/timed out/not connected` errors and transitions the UI status to `reconnecting` instead of surfacing an error to the user.
- **Locale strings for enter key behavior**: new i18n messages (`enterKeyBehavior`, `enterSendsMessage`, `shiftEnterNewLine`, `modifierEnterSendsMessage`, `enterNewLineModifierSends`, `sendWithEnter`) added for all 6 supported locales (ar, en, es, fr, ru, zh_CN).
- **Token input upgraded**: Bearer token field in settings changed from `Textarea` (4 rows) to single-line `Input type="text"` for cleaner layout.

### Changed

- **Polling interval increased**: `pollingIntervalMs` raised from 2000ms to 3000ms to reduce unnecessary server load.
- **Submit tooltip adapts to mode**: send button tooltip now shows "Enter to send. Shift + Enter for a new line." in `enter` mode, and "Command/Control + Enter to send. Enter for a new line." in `modifier-enter` mode.
- **Manifest permissions simplified**: `"host_permissions": ["*://*/*"]` + `"optional_host_permissions": ["file:///*"]` replaced with `"host_permissions": ["<all_urls>"]`, equivalent behavior.
- **`updateConversationMessages` guard**: now passes through even when `incoming` is empty if `updatedAt` is set, ensuring timestamps update even without new messages.

## [0.7.1] — 2026-05-16

### Added

- **`/new` command**: `/new [prompt]` (alias `/clear`) starts a fresh conversation, completing the current one if unfinished. Works across CLI, TUI, channel runtime, and Chrome extension. Trusted channel users can use `/new` in IM; `$external_user` attempts are ignored.
- **Stale conversation detection in channel output**: when agent output arrives for a route whose current conversation has moved on (e.g., after `/new`), the channel message is prefixed with `[Previous conversation #N]` so recipients know the context has shifted.
- **TUI scrollback purge on `/new`**: `ClearType::Purge` clears the terminal scrollback buffer when starting a new conversation, giving a truly fresh visual experience. Added `clear_message_view()` and `pending_scrollback_purge` state.
- **Chrome extension `/new` integration**: command palette entry for `/new`, `parseNewPromptCommand()` in `client.ts`, and `clearConversationDisplay()` for full display reset on new conversation.
- **System extra content injection**: `system_extra_content()` serializes request context metadata (`ctx.extra`) as structured `ContentPart` prefixed with `[$system: ...]`, replacing ad-hoc string formatting. Applied at both initial prompt and follow-up input boundaries.
- **`session_creation_lock`**: mutex serializes session creation to prevent races between concurrent requests for the same source.
- **`get_session_by_source()`**: finds an active session by source key, enabling session reuse across conversation boundaries (e.g., after `/new` without a prompt).
- **`finish_when_idle` flag**: when `/new` detaches a running session, `finish_when_idle=true` tells the session runner to complete and exit gracefully once idle — no abrupt kill.

### Changed

- **ContentPart migration**: steering and follow-up messages now use `Vec<ContentPart>` internally instead of string concatenation (`format!("{}\n\n{}")`). Resource attachments are converted to `ContentPart` directly via `follow_up_content()`. This aligns with `anda_engine` 0.12.9's upgraded steering API and enables multimodal content passthrough.
- **Background task hooks carry context**: `on_background_progress` and `on_background_end` now pass `ctx.meta().extra` through `ConversationInput`, so background task outputs correctly include the originating request context.
- **Session detach returns value**: `detach_session()` returns `Option<Arc<Session>>` instead of just removing, enabling the caller to set `finish_when_idle` before the session exits.
- **`clear_route_conversation()`**: new channel runtime method removes current route→conversation binding while preserving the old conv→route mapping for stale output detection.
- **`force_standalone_conversation` flag**: new conversations started via `/new` skip history chaining and ancestor linking, keeping the fresh session truly independent.
- **Status bar updated**: TUI help text now includes `/new [message]` as the first slash command.

### Dependencies

- `anda_core` 0.12.0 → 0.12.1
- `anda_engine` 0.12.7 → 0.12.9
- `anda_hippocampus` 0.5.2 → 0.5.3
- `ic-agent` / `ic-transport-types` 0.47.2 → 0.47.3

## [0.7.0] — 2026-05-13

### Added

- **Streaming TTS playback with chunked synthesis**: long assistant responses are split into ~800-character chunks and synthesized/played back in a pipeline (play current chunk while synthesizing the next), replacing the previous wait-for-full-synthesis approach. This dramatically reduces response latency in voice conversations.
- **Markdown-to-speech text normalization**: `prepare_voice_tts_text()` strips markdown formatting prefixes (`>`, `-`, `#`, `*`, `|`), collapses whitespace, filters TTS-unsafe characters (emoji, control chars), and normalizes punctuation for natural-sounding speech synthesis.
- **Stepfun STT provider**: new `transcription/stepfun.rs` module with SSE-based streaming ASR, supporting hotwords, ITN, PCM codec/rate/bits/channel configuration, and optional prompts.
- **Stepfun TTS provider**: new `tts/stepfun.rs` module supporting `stepaudio-2.5-tts` with configurable voice, speed, volume, sample rate, optional pronunciation maps, and markdown filtering.
- **Per-provider audio format**: each TTS provider now declares its native audio format via `audio_format()` (e.g., `stepfun` returns `pcm`, others `mp3`). `audio_artifact_for_provider()` builds artifacts with the correct MIME type for each provider.
- **Voice status spinner**: `VoiceStatusSpinner` renders rotating progress indicators during long operations (sending, waiting, synthesizing, playing) for clear feedback in voice mode.
- **Graceful Ctrl-C in voice mode**: all async await points in the voice loop are wrapped through `wait_with_voice_status()`, enabling clean interruption during any phase (send, poll, synthesize, play).
- **Daemon startup diagnostics**: background daemons now capture stdout/stderr to a log file. On startup failure, the last 64KB of the daemon log is tailed and parsed for structured JSON log entries or plain error lines, surfaced in the error message.
- **`try_wait()` on `BackgroundDaemon`**: exposes the child process for early exit detection during startup wait loops.
- **Cron job metadata in channel output**: cron-triggered agent completions now display the job name, kind (shell/agent), and job content in the IM channel message, so recipients know which scheduled job produced the output.
- **`fmt::Display` for `JobKind`**: human-readable `"shell"` / `"agent"` labels.

### Changed

- **Voice response text includes artifacts**: `VoiceConversationCursor` now tracks `seen_artifacts`, and `assistant_text_from_messages()` includes artifact descriptions in the synthesized text when present (e.g., "I created a file for you").
- **Error handling**: `main()` wrapped in `run()` for proper `log::error` on unhandled failures instead of default Rust panic output.
- **TTS config**: `default_voice` field removed; per-provider voices are configured directly under each provider section.

### Refactored

- **TTS providers modularized**: monolithic `tts.rs` split into per-provider modules: `tts/edge.rs`, `tts/google.rs`, `tts/openai.rs`, `tts/stepfun.rs`. Provider-specific config types moved to `config/tts.rs`.
- **Transcription providers modularized**: monolithic `transcription.rs` split into per-provider modules: `transcription/google.rs`, `transcription/groq.rs`, `transcription/local_whisper.rs`, `transcription/openai.rs`, `transcription/stepfun.rs`. Provider-specific config types moved to `config/transcription.rs`.

## [0.6.5] — 2026-05-12

### Added

- **Startup self-check & conversation recovery**: `AndaBot::init()` now triggers a startup self-check (5s after launch) that scans all source-bound conversations and auto-resumes any in `Submitted`/`Working` state from the last saved history. Interrupted conversations survive process restarts: the agent reconstructs system instructions, chat history, request metadata, and tool selection, then sends a recovery prompt and continues the session. Recovery is scoped to currently active IM channels.
- **`log_level` config**: new `log_level` field in `config.yaml` (default `warn`) controls structured log verbosity. `logger::init_daily_json_logger` now accepts an explicit level parameter instead of reading from env.
- **Cron agent job prompt enrichment**: cron-triggered agent jobs now receive a system runtime prompt including `_id`, `name`, and `instructions` metadata, helping the agent distinguish scheduled work from user-initiated requests.
- **Conversation child chaining**: new conversations are now linked as the `child` of the previous conversation in the chain. `latest_conversation_in_chain()` traverses the child chain (up to 256 hops, with cycle detection) to find the latest conversation for a given source.
- **`conversation_chat_history()`**: extracts and marks messages from stored conversation JSON, stripping dangling `tool_calls` without corresponding `tool_result` blocks.
- **`GetConversation` with `_id: 0`**: resolves to the latest conversation document via `latest_document_id()`.
- **`external_user_name()` helper**: formats `$external_user:<name>` participant names consistently.
- **Tests**: `startup_status_policy_resumes_only_running_states`, `request_meta_from_conversation_recovers_route_from_source_key`, `conversation_chat_history_marks_startup_runtime_messages`, `format_local_date_returns_datetime_with_timezone`.

### Changed

- **System instructions use local datetime format**: `format_local_date()` produces `"YYYY-MM-DD HH(AM/PM) ±TZ"` via chrono `clock` feature, replacing the RFC 3339 `rfc3339_datetime()` format across all system instruction rendering.
- **Conversation continuation semantics**: existing conversations in `Submitted`/`Working`/`Idle` state can now be continued with an empty prompt (the session enters wait mode). Previously all prompts required non-empty content.
- **`user_info()` parameter type**: `Principal` → `String` for broader compatibility (aligned with anda_hippocampus v0.5.2 changes).
- **`mark_special_user_messages` unified**: `mark_system_runtime_messages` and `mark_external_user_messages` merged into a single function. External user messages with existing names now preserve them via `external_user_name()`.
- **Session field renamed**: `source` → `source_key` for clarity.
- **`source_state` updated on compaction**: compaction now updates the source→conversation mapping to the new conversation id.
- **Runtime prompt wording**: `"not from the external user"` → `"not from the user"`.
- **Code extraction**: `persist_conversation_state()`, `spawn_session_runner()`, `available_tool_names()` extracted from inline logic to reusable methods for startup recovery code paths.
- **`chrono`**: `clock` feature enabled for local timezone support.

### Dependencies

- `anda_hippocampus` → 0.5.2 (user init routing, local_date_hour, prompt improvements)

## [0.6.4] — 2026-05-11


### Added

- **Codex OAuth token auto-loading**: when a model provider is configured with `api_base: "https://chatgpt.com/backend-api/codex"`, the daemon now automatically reads `~/.codex/auth.json` and injects the `access_token` as `api_key`. This enables seamless Codex backend usage without hardcoding tokens in config — just log in once and the daemon picks up the token on restart.
### Fixed

- **IME composition jitter in TUI input** ([#1](https://github.com/ldclabs/anda-bot/issues/1)): the main render loop now only redraws the terminal when observable state has actually changed, using `ChatRenderSnapshot` / `StatusRenderSnapshot` comparison plus a `needs_render` flag. Previously the terminal was redrawn on every loop iteration (~6–7 fps), causing IME composition candidate windows to flicker and shift on systems like Fedora 42. Render-on-demand triggers include: chat message changes, new streaming tokens, terminal resize, key input, paste, thinking state transitions, and daemon status changes.

### Added

- **↑/↓ cursor navigation in multi-line input**: `Up` and `Down` arrow keys now move the cursor vertically through multi-line input, tracking a preferred visual column (`input_preferred_col`) so repeated ↑/↓ stays on the same column. Built on `move_cursor_vertically()`, `input_cursor_points()`, and `input_cursor_for_visual_position()`.
- **Input scrollbar**: when input text exceeds the available area, a vertical scrollbar (`┃` thumb on `│` track) appears at the right edge. The viewport auto-scrolls to keep the cursor visible, using `InputViewport` + `input_scroll_top()`.
- **Ctrl+J as alternative newline**: for terminals that do not distinguish `Shift+Enter`, `Ctrl+J` now also inserts a newline (`input_newline_key()`).
- **Input viewport abstraction**: new `InputViewport` struct and `build_input_viewport()` encapsulate line rendering, scroll position, cursor placement, and virtual continuation lines when the cursor wraps to a new row after the last actual character.
- **7 new tests**: `input_newline_key_accepts_shift_enter_and_ctrl_j`, `move_cursor_vertically_preserves_visual_column`, `move_cursor_vertically_handles_wrapped_lines`, `input_viewport_follows_cursor_to_bottom_of_long_paste`, `input_viewport_keeps_cursor_line_visible_when_moved_up`, `input_viewport_adds_virtual_line_when_cursor_wraps_past_full_row`, `input_scroll_top_tracks_cursor_without_exceeding_content`.

### Changed

- **Keyboard shortcut help updated**: status bar now reads `"Enter send  •  Shift+Enter/Ctrl+J newline  •  ↑/↓ move lines  •  Ctrl+U clear  •  Ctrl+C quit"`. README.md, README_cn.md, and anda_bot/README.md all reflect the new shortcuts.
- **`handle_key` now receives `input_content_width`**: needed for vertical cursor movement calculations, which depend on the actual visible content width.
- **`wrapped_cursor_position` split**: new `wrapped_cursor_position_usize` internal variant returns `(u16, usize)` for scroll-aware cursor row tracking.

## [0.6.3] — 2026-05-11

### Added

- **Markdown rendering in TUI chat messages**: assistant `ContentPart::Text` messages are now rendered through a GFM-compliant markdown pipeline (`tui/markdown.rs`, ~450 lines) using the `markdown` crate. Supported elements receive distinct ratatui styles: 4 heading levels (panda white / bamboo light / bamboo green / leaf mint, all bold), **bold** (bold modifier), *emphasis* (italic), ~~strikethrough~~ (dim + crossed-out), `inline code` and code blocks (accent teal on footer background), [links](url) (underlined teal), > blockquotes (dim italic), lists (bamboo light), and tables with left/center/right alignment support plus styled header/separator/body rows. Grayscale fallback: when markdown parse fails, text is rendered as plain text instead of erroring.
- **Grapheme-aware line wrapping for styled spans**: the new `wrap_styled_body_line` function wraps styled `Line<'static>` values grapheme-by-grapheme, preserving individual span styles across line boundaries. Control characters and zero-width graphemes are filtered during wrap. Consecutive same-style spans are merged into single spans where possible.
- **Table rendering in TUI**: GFM tables are parsed from the source text, column-widths computed via Unicode display width, and cells aligned according to the source alignment hints (`:---`, `:---:`, `---:`). Separator rows are rendered between header and body. Alignment defaults to `---` (3 dashes, right-padded).
- **New tests**: `chat_message_lines_render_markdown_source_styles` (verifies heading bold + color, inline bold + inline code styles), `chat_message_lines_render_markdown_tables` (verifies aligned markdown source output for tables).
- **Background shell intermediate output streaming**: the `Session` tool hook now implements `on_background_progress`, forwarding intermediate background task output to the agent as `$system` runtime prompts (not just final completion). This lets the agent see long-running task progress in real time.

### Changed

- **Shell runtime runs in insecure mode**: `NativeRuntime::new(workspace).insecure()` allows shell commands wider system access when needed.
- **anda_engine upgraded to 0.12.4**.
- **`push_wrapped_block` renamed → `push_markdown_block`**: now delegates to `markdown::render()` instead of doing simple `text.lines()` + `wrap_visual()`. The old plain-text wrapping logic is replaced by styled span wrapping.
- **Background shell prompt label unified**: `"background shell task"` → `"background shell"` across both `on_background_end` and the new `on_background_progress` hooks.

## [0.6.2] — 2026-05-10

### Changed

- **Cron jobs run as original caller**: `CronJobOrigin` now captures the `caller` Principal at creation time via `from_meta_with_caller()`. When a cron job executes (shell or agent), it impersonates the original creator rather than the system controller. `notify_shell_result` also uses the caller for agent notification. This ensures scheduled jobs remember who created them and operate with the correct identity.
- **SessionRequestMeta**: new `SessionRequestMeta` struct (stored as session state via `ctx.base.set_state`) persists the original request metadata across the session lifetime. When an agent reconnects to an existing session, `request_meta` is updated with the current conversation ID. `CreateCronTool` reads `SessionRequestMeta` from context state to resolve the caller identity for cron job origin capture.

## [0.6.1] — 2026-05-10

### Added

- **Goal as a first-class agent-callable tool**: the `goal` tool is now directly callable by the agent (not just via `/goal` slash command). When the agent encounters a complex multi-turn request, it can autonomously start or update goal mode by calling `goal` with a concrete objective and verification criteria, then continue working normally. The session stays alive as long as a goal is active. `GoalTool` shares the session's goal state and `active_at` timestamp via `Arc`, and `SelfInstructions.md` includes explicit guidance on when the agent should (and should not) use the tool.

## [0.6.0] — 2026-05-09

### Added

- **External user support with trust boundaries**: new `allow_external_users` config field for supported channel types (Discord, Telegram, Lark, WeChat). When enabled, messages from non-allowlisted senders are tagged as `external_user: true` and wrapped with `[$external_user: channel="...", sender="..."]` prefix, allowing the agent to distinguish untrusted guests from the owner/partner. A comprehensive Trust Boundaries section in `SelfInstructions.md` governs how the agent handles external user data.
- **Cron job origin context**: new `CronJobOrigin` struct captures the full request context (user, source, reply_target, thread, workspace, conversation_id, external_user) when a cron job is created. Origin is persisted in the job record (schema v2) and round-tripped back into `RequestMeta` on each execution, so scheduled jobs "remember" which channel and conversation they came from.
- **Shell cron result notification**: when a scheduled shell job completes, the result (stdout or error) is fed back to the agent via `system_runtime_prompt("cron shell job result")`, enabling the agent to incorporate the outcome and notify the originating user in-channel.
- **Channel route recovery from RequestMeta**: `on_completion` hook now falls back to `route_from_meta()` when `route_for_conversation()` misses, reconstructing the channel route from persisted `RequestMeta` extras. New bindings are persisted for future lookups.

### Changed

- **System prompt format upgrade**: `[$system runtime message: ...]` → `[$system: kind="..."]` (structured key-value format) across compaction, goal continuation, subagent progress/final output, and background shell task notifications. New `mark_special_user_messages()` unifies backfilling for both `$system` and `$external_user` names.
- **Formation attribution for external users**: memory formation now uses `$external_user:<sender>` as counterparty instead of the caller when `external_user` is set, keeping guest memories isolated from the trusted user's profile.
- **Channel message schema v2**: `ChannelMessage` gains `external_user: Option<bool>` field, with all channel implementations updated to populate it and tests added for the new behavior.
- **CronRuntime::connect** simplified: controller Principal now derived from `Principal::management_canister()` instead of requiring an explicit `engine_id` parameter.
- **Documentation**: `allow_external_users` documented in README.md, README_cn.md, and anda_bot/README.md; config.yaml updated with commented examples; config tests assert the new field.

## [0.5.4] — 2026-05-09

### Added

- **Multi-workspace support**: `workspace_dir: PathBuf` → `workspaces: Vec<PathBuf>` in `EngineConfig`, covering workspace, sandbox, channels, and skills directories. File tools (`ReadFile`, `SearchFile`, `EditFile`, `WriteFile`) resolve paths across all configured workspaces via `with_workspaces()`.
- **$system runtime message protocol**: new `engine/system` module introduces structured runtime messages with a `[$system runtime message: <kind>]` prefix and disclaimer, allowing the model to distinguish system/operational prompts from external user intent. Wraps compaction handoffs, goal continuation prompts, subagent progress/final output, and background shell task notifications. A `mark_system_runtime_messages()` function backfills the `$system` name on persisted user-role messages that contain these prompts, ensuring correct attribution across sessions.

### Changed

- **SelfInstructions rewrite**: system prompt restructured to be more concise and persona-driven. New sections: `Participants` (explicitly naming `$self`, `$system`, and the external user), `Personality And Relationship`, and `Communication`. Memory/Growth and Working Philosophy sections simplified. Tone shifted from tutorial-style to a confident partner persona.
- **Quick dependency bumps**: `anda_cognitive_nexus` 0.7.18, `anda_engine` 0.12.2, `anda_hippocampus` 0.5.1, `hashbrown` 0.17.1, `quick-xml` 0.39.4, `tokio` 1.52.3.

## [0.5.3] — 2026-05-08

### Added

- **Session introspection API & CLI**: new `ListSessions` / `GetSession` tool calls on the engine allow agents (and external callers) to inspect active session state. CLI: `anda session list` and `anda session get <id>` with `--json` output.
- **Documentation site (docsite)**: Docusaurus-based documentation at docs.anda.bot covering quick start, memory (Hippocampus), runtime (channels, configuration), and workflows (long-horizon goals). Full i18n across 6 languages (EN, ZH, ES, FR, RU, AR).
- **SubAgentManager & SkillManager** added to base tools, enabling agents to spawn subagents and load skills without custom tool config.

### Changed

- **Workspace prompt hardened**: system prompt now labels the workspace as AUTHORITATIVE with an explicit warning not to trust workspace paths from past `user_history_conversations` — they may belong to different sessions.
- **Context continuity**: the current conversation is now included in `history_conversations`, giving the model access to the full thread (not just ancestors).
- **Dependencies**: `anda_engine` 0.12.0 → 0.12.1.

## [0.5.2] — 2026-05-07

### Changed

- **SubAgent module alignment**: imports migrated from `anda_engine::context` to `anda_engine::subagent` (the new top-level module in anda_engine v0.12).
- **Core function promotions**: `prompt_with_resources` and `text_resource_documents` moved from local helpers to `anda_core`.
- **Goal completion behavior**: instead of silently keeping the session active after goal completion, a supervisor evaluation message is now injected into the chat history.
- **Terminology**: `SessionJob` → `SessionRunner`, `task_id` → `session_id`, "Background task" → "Subagent session" throughout.
- **CLI**: `--skills-only` flag renamed to `--skills`.
- **Website rebrand**: landing page repositioned as "open-source Rust terminal agent with graph memory, subagents, and external tool integration"; all 6 locales updated; hero section redesigned.

### Added

- **Background subagent progress reporting**: new `on_background_progress` hook surfaces intermediate subagent output to the user in real-time via chat messages.
- **Side agent shell access**: `ShellTool` added to side agent's allowed tools for read-only filesystem inspection.


### Added

- **Codex OAuth token auto-loading**: when a model provider is configured with `api_base: "https://chatgpt.com/backend-api/codex"`, the daemon now automatically reads `~/.codex/auth.json` and injects the `access_token` as `api_key`. This enables seamless Codex backend usage without hardcoding tokens in config — just log in once and the daemon picks up the token on restart.
### Fixed

- **recall_memory discipline**: tool description updated to explicitly discourage calls for facts already present in the active conversation.

### Dependencies

- `anda_core`, `anda_engine`, `anda_engine_server`, `anda_web3_client`: 0.11 → 0.12
- `anda_hippocampus`: 0.4 → 0.5

## [0.5.1] — 2026-05-07

### Changed

- **Conversation state consolidation**: `source_conversation` and `tools_usage` state moved from `AndaBot` into `ConversationsTool`, reducing surface area in agent.rs and centralizing serialization logic.
- **Async non-blocking send**: synchronous `send` replaced with oneshot-channel `start_send` / `finish_pending_send` to decouple the UI event loop from network I/O.
- Added `awaiting_response` field to track the gap between request dispatch and response arrival.

## [0.5.0] — 2026-05-07

Initial tracked release.
