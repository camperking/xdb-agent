# xdb-agent

An LLM chat agent with Signal Messenger integration, powered by [rig](https://github.com/0xPlaygrounds/rig) and [presage](https://github.com/whisperfish/presage).

The agent connects to a local OpenAI-compatible LLM server and exposes it over Signal as a chatbot. It supports individual conversations and group chats, with an allowlist to control who can interact with it.

A memory system stores recent conversation history and relevant facts, which are included in the prompt context sent to the LLM. The agent also has access to a set of tools (e.g. math operations, file reading) that it can invoke based on user input.

## Requirements

### Signal

- A Signal account on a primary device (phone) to scan the pairing QR code

### Large language model

- A running OpenAI-compatible LLM server (e.g. [llama.cpp](https://github.com/ggerganov/llama.cpp), [LM Studio](https://lmstudio.ai/), [Ollama](https://ollama.com/))
- listening on `localhost:8080`
- eg. `./llama-server -m ./Qwen3.5-9B-Q8_0.gguf --port 8080 -ctk bf16 -ctv bf16`

### Embedding model

- A running [Ollama](https://ollama.com/) server with an embedding model (default: `nomic-embed-text-v2-moe`)
- get it with `ollama pull nomic-embed-text-v2-moe`

### Development

- Rust (edition 2024)
- `protoc` (Protocol Buffers compiler): `sudo apt install protobuf-compiler`
- `libclang`: `sudo apt install libclang-dev`


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

**Add a group** (use the group's master key as a hex string, visible in Signal Desktop under group settings → **Group Link** or via a Signal debug export):
```sh
cargo run -- config add-group "64 hex characters = 32 bytes"
```

**View the current allowlist:**
```sh
cargo run -- config show
```

**Remove entries:**
```sh
cargo run -- config remove-uuid "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee"
cargo run -- config remove-group "64 hex characters = 32 bytes"
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

## Interactive slash commands
- `/new` — reset the current conversation context and token counters
- `/usage` — print cumulative token usage for the current conversation
- `/compact` — compact chat history, save to file and re-initialize the agent

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

Log output is written to stderr.

You can increase default verbosity with `-v` / `-vv` (global flag):

```sh
# info (default)
cargo run -- daemon

# debug
cargo run -- -v daemon

# trace
cargo run -- -vv daemon
```

`RUST_LOG` can still be used for explicit filter control:

```sh
RUST_LOG=debug cargo run -- daemon
```

The agent also logs token usage for each request and cumulative totals per conversation (direct user or group). If the OpenAI-compatible backend does not return usage fields, the agent falls back to a token estimate based on text length.
