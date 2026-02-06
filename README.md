# nanoclaw

A lightweight personal AI assistant framework in Rust.

Ported from [nanobot](https://github.com/HKUDS/nanobot) by [HKUDS](https://github.com/HKUDS) (MIT License).

## What it does

- **Agent loop**: LLM -> tools -> response cycle with configurable providers
- **Multi-provider**: OpenRouter, Anthropic, OpenAI, Groq, DeepSeek, Gemini, vLLM (all via OpenAI-compatible API)
- **Built-in tools**: file read/write/edit, shell exec, web search/fetch, message, spawn subagents, cron scheduling
- **Chat channels**: Telegram, WhatsApp (bridge), Feishu
- **Memory**: Daily notes + long-term memory with file-based persistence
- **Skills**: Markdown-based skill system with YAML frontmatter
- **Sessions**: JSONL session persistence

## Build

```bash
cargo build --release
```

## Quick start

```bash
# Initialize config and workspace
nanoclaw onboard

# Add your API key to ~/.nanoclaw/config.json

# Chat directly
nanoclaw agent -m "Hello!"

# Interactive mode
nanoclaw agent

# Start gateway with channels
nanoclaw gateway
```

## Commands

| Command | Description |
|---------|-------------|
| `nanoclaw onboard` | Initialize config and workspace |
| `nanoclaw agent -m "..."` | Send a message to the agent |
| `nanoclaw agent` | Interactive chat mode |
| `nanoclaw gateway` | Start gateway with channels + agent loop |
| `nanoclaw status` | Show configuration status |
| `nanoclaw channels status` | Show channel status |
| `nanoclaw cron list` | List scheduled jobs |
| `nanoclaw cron add` | Add a scheduled job |

## Config

Configuration lives at `~/.nanoclaw/config.json`. Workspace defaults to `~/.nanoclaw/workspace/`.

## Attribution

This project is a Rust port of [nanobot](https://github.com/HKUDS/nanobot), an ultra-lightweight personal AI assistant by HKUDS. The original Python implementation is licensed under MIT.

## License

MIT
