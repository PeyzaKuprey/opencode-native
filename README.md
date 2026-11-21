# OpenCode Native (`ocn`)

Lightweight native port/fork direction for OpenCode-style agent workflows, built in Rust for direct device use.

## Why

OpenCode's official CLI is powerful but heavy on constrained devices. This project aims for a small native binary that can run directly on Linux/Android (Termux) without proot and with minimal RAM/ROM overhead.

## Status

Early MVP. Not full OpenCode parity yet.

Implemented:
- Native Rust CLI
- Config file (`~/.config/opencode-native/config.toml` or `$XDG_CONFIG_HOME/opencode-native/config.toml`)
- OpenAI-compatible chat API backend
- Basic agent loop with local tools: `shell`, `read`, `write`, `edit`, `glob`, `grep`
- `run` one-shot prompt execution
- `chat` interactive REPL
- Termux `.deb` packaging script

Planned:
- OpenCode HTTP API compatibility layer
- Sessions, permissions, MCP, plugins, LSP, TUI
- Better streaming, tool schemas, model/provider management
- Official Termux package submission

## Build

```bash
cargo build --release
```

Binary: `target/release/opencode-native`

## Install on Termux (local .deb)

```bash
./scripts/build-termux-deb.sh
pkg install ./dist/opencode-native_0.1.0_aarch64.deb
```

For official `pkg install opencode-native` availability, the package must be submitted/accepted into Termux repositories.

## Configuration

Create `~/.config/opencode-native/config.toml`:

```toml
[model]
provider = "openai-compatible"
base_url = "https://api.openai.com/v1"
api_key_env = "OPENAI_API_KEY"
model = "gpt-4.1-mini"

[agent]
system_prompt = "You are a helpful coding agent running natively on device."
max_steps = 20
```

Then run:

```bash
export OPENAI_API_KEY=...
opencode-native run "List files in current directory and summarize project"
```

## Name

- **OpenCode** - the product/agent platform.
- **Big Pickle** - the model identity you are talking to in this session.
- **OpenCode Native / ocn** - this lightweight native implementation.

## License

MIT
