# xdb-agent

An LLM chat agent with Signal Messenger integration, powered by [rig](https://github.com/0xPlaygrounds/rig) and [presage](https://github.com/whisperfish/presage).

The agent connects to a local OpenAI-compatible LLM server (default: `http://localhost:8080`) and exposes it over Signal as a chatbot. It supports individual conversations and group chats, with an allowlist to control who can interact with it.

## Requirements

- Rust (edition 2024)
- `protoc` (Protocol Buffers compiler): `sudo apt install protobuf-compiler`
- `libclang`: `sudo apt install libclang-dev`
- A running OpenAI-compatible LLM server (e.g. [llama.cpp](https://github.com/ggerganov/llama.cpp), [LM Studio](https://lmstudio.ai/), [Ollama](https://ollama.com/))
- A Signal account on a primary device (phone) to scan the pairing QR code

## Build

```sh
cargo build --release
```

The binary will be at `target/release/chat-agent`.

## Usage

### 1. Pair as a secondary Signal device

Run this once to link the agent to your Signal account. A QR code will be printed in the terminal — scan it with the Signal app on your phone (**Linked Devices → Link New Device**).

```sh
cargo run -- link-device
# or with a custom device name:
cargo run -- link-device --device-name "my-agent"
```

### 2. Configure the allowlist

Only contacts and groups explicitly added to the allowlist will receive responses from the agent.

**Add a contact** (use their Signal UUID, visible in the Signal desktop app under contact details):
```sh
cargo run -- config add-uuid "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee"
```

**Add a group** (use the group's master key as a base64 string, visible in Signal Desktop under group settings → **Group Link** or via a Signal debug export):
```sh
cargo run -- config add-group "32-bytes-HEX-key"
```

**View the current allowlist:**
```sh
cargo run -- config show
```

**Remove entries:**
```sh
cargo run -- config remove-uuid "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee"
cargo run -- config remove-group "32-bytes-HEX-key"
```

The config is stored at `~/.config/xdb-agent/config.toml` by default:

```toml
allowed_uuids = [
    "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee",
]
allowed_groups = [
    "32-bytes-HEX-key",
]
```

### 3. Run the Signal daemon

```sh
cargo run -- daemon
```

The daemon will:
1. Load `config.toml` from the default path
2. Connect to Signal and drain the message backlog
3. Forward incoming text messages from allowlisted contacts/groups to the LLM
4. Send the LLM's response back via Signal

### 4. Interactive terminal chat

Run the agent in a plain stdin/stdout loop without Signal:

```sh
cargo run -- chat
# or just:
cargo run
```

## Custom paths

Both the Signal database and the config file paths can be overridden globally:

```sh
cargo run -- --db-path /path/to/signal.db3 --config /path/to/config.toml daemon
```

| Path | Default |
|------|---------|
| Signal database | `~/.local/share/xdb-agent/signal.db3` |
| Configuration | `~/.config/xdb-agent/config.toml` |

## Available tools

The LLM agent has access to the following tools:

| Tool | Description |
|------|-------------|
| `add` | Add two integers |
| `multiply` | Multiply two integers |
| `get_time` | Return the current UTC time |
| `increment_counter` | Increment a shared in-memory counter |
| `get_counter` | Read the current counter value |
| `read_file` | Read a file from the filesystem by path |

## Logging

Log output is written to stderr. The level can be controlled with the `RUST_LOG` environment variable:

```sh
RUST_LOG=debug cargo run -- daemon
```
