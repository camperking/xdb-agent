# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

Rust-based LLM chat agent ("chat-agent") that integrates with Signal Messenger via the `presage` library. It connects to a local OpenAI-compatible LLM server and exposes it as a Signal chatbot with per-conversation agent instances and automatic context compaction.

## Build & Run

```bash
cargo build --release          # Build
cargo run                      # Interactive chat (default subcommand)
cargo run -- daemon            # Signal daemon mode (background service)
cargo run -- link-device       # Link as secondary Signal device
cargo run -- config show       # Show allowlist config
cargo run -- config add-uuid <uuid>
cargo run -- config add-group <hex-key>
```

Verbosity: `-v` (debug), `-vv` (trace).

## Testing & Linting

```bash
cargo test                     # No tests exist yet
cargo clippy                   # Lint
cargo fmt                      # Format
```

## Architecture

- **`main.rs`** — CLI (clap) with subcommands: `chat`, `daemon`, `link-device`, `config`
- **`agent.rs`** — LLM agent lifecycle using the `rig-core` framework. Each user/group gets its own Agent with independent history. Auto-compacts context at 120k tokens by summarizing in German.
- **`signal.rs`** — Signal Protocol integration via `presage`/`presage-store-sqlite`. Handles message polling, allowlist validation, and reply dispatch. Uses channels for signal messaging.
- **`config.rs`** — TOML config at `~/.config/xdb-agent/config.toml` (allowed UUIDs and groups)
- **`slash_commands.rs`** — In-chat commands: `/new`, `/usage`, `/compact`, `/shutup`, `/talk`
- **`tools/`** — Agent tools (calculator, counter, file reader, weather via Open-Meteo, web search via DuckDuckGo)
- **`templates/xDevBot.md`** — German-language system prompt defining bot personality

### Key design patterns

- **Per-conversation agents**: Separate `Agent` instance per Signal contact/group, each with its own chat history
- **Context compaction**: Summarizes conversation history in German when tokens exceed 120k, stores summaries in `~/.local/share/xdb-agent/memory/`
- **Heartbeat suppression**: Returns `HEARTBEAT_OK` for group messages the bot shouldn't respond to
- **Allowlist-based access**: Only whitelisted UUIDs/groups receive responses
- **Dynamic context**: Optional vector-based retrieval of past summaries using Ollama embeddings (`nomic-embed-text-v2-moe`)

### External services

- Local OpenAI-compatible LLM at `http://localhost:8080` (model: `Qwen3.5-35B-A3B`)
- Ollama embeddings at `http://localhost:11434`
- Signal data stored in `~/.local/share/xdb-agent/signal.db3`
