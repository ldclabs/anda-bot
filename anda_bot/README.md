# Anda Bot

Born of panda. Awakened as Anda. A copilot for your computer, powered by the ANDA Hippocampus.

## Usage

Run the daemon in the foreground:

```bash
cargo run -p anda_bot -- daemon
```

Run the unified `anda` binary in interactive TUI mode:

```bash
cargo run -p anda_bot --
```

On first launch, `anda` creates a default `config.yaml` file in the local home directory (`~/.anda/config.yaml` by default) with detailed setup notes.

The TUI is chat-focused now. It no longer edits configuration inline. If required settings are missing, the UI will pause in setup mode and ask you to edit `config.yaml`, then reload with `Ctrl+R` or restart `anda`.

Minimal top-level keys:

```yaml
addr: 127.0.0.1:8042
sandbox: false
# https_proxy: http://127.0.0.1:7890
```

Model and channel settings live under `model` and `channels`:

```yaml
model:
	active: DeepSeek
	providers:
		DeepSeek:
      family: deepseek
      model: "deepseek-reasoner"
      api_base: "https://api.deepseek.com/v1"
			api_key: ...

  MiniMax:
      family: anthropic
      model: "MiniMax-M2.7-highspeed"
      api_base: "https://api.minimaxi.com/anthropic/v1"
      api_key: ""
      disabled: false

channels:
	irc:
		- id: libera
			server: irc.libera.chat
			nickname: anda-bot
			channels:
				- "#anda"
```

## License

Copyright © LDC Labs

Licensed under the MIT or Apache-2.0 license.
