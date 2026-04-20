# Anda Bot

Just A Rather Very Intelligent Bot.

## Usage

Run the daemon in the foreground:

```bash
cargo run -p anda_bot -- daemon
```

Run the unified `anda` binary in interactive TUI mode:

```bash
cargo run -p anda_bot --
```

On first launch, `anda` creates a default `.env` file in the local home directory (`~/.anda/.env` by default) with detailed setup notes.

The TUI is chat-focused now. It no longer edits configuration inline. If required model settings are missing, the UI will pause in setup mode and ask you to edit `.env`, then reload with `Ctrl+R` or restart `anda`.

Required `.env` keys for chat:

```env
MODEL_FAMILY=
MODEL_NAME=
MODEL_API_KEY=
MODEL_API_BASE=
```

Optional keys:

```env
GATEWAY_ADDR=127.0.0.1:8042
SANDBOX=false
# HTTPS_PROXY=
```

## License

Copyright © LDC Labs

Licensed under the MIT or Apache-2.0 license.
