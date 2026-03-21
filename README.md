# Agent OS

An experimental operating environment for autonomous AI agents, focused on task execution, tool access, MCP interoperability, and persistent runtime state.

## Status

Active prototype. The repository currently ships a Rust runtime, a sample TOML config, a Linux kernel module experiment, and design docs for the larger vision.

## About

Agent OS treats agent context, tools, permissions, and task processing as first-class system concerns. Instead of building a single assistant, the project aims to provide a runtime where agents can receive work, reason with models, call tools, and expose capabilities over HTTP and MCP.

The current implementation is centered on the Rust runtime in `runtime/`.

## Current Scope

- HTTP API for task queueing, tool execution, agent spawning, and message inspection
- TOML-based configuration in `runtime/agent-os.toml`
- Built-in tools for time, filesystem access, HTTP fetches, shell execution, and agent-to-agent actions
- MCP-compatible endpoints for listing and calling tools
- Automatic background processing loop for queued tasks
- Prototype kernel module and VM bootstrap scripts for lower-level experiments

## Tech Stack

| Layer | Technology |
| --- | --- |
| Runtime | Rust 2021 |
| HTTP framework | Axum |
| Async runtime | Tokio |
| Serialization | Serde / JSON / TOML |
| Model backends | Ollama by default, optional OpenAI and Anthropic configs |
| Low-level experiments | C kernel module + Makefile |

## Project Structure

```text
agent-os/
|- runtime/
|  |- Cargo.toml
|  |- agent-os.toml
|  |- src/main.rs
|- kernel-module/
|  |- agent_os.c
|  |- Makefile
|- scripts/
|  |- create-dev-vm.sh
|- SPEC.md
|- REQUIREMENTS.md
|- TODO.md
```

## Getting Started

### Prerequisites

- Rust toolchain with Cargo
- An Ollama-compatible endpoint if you want live model execution
- A writable storage path for runtime state

### Build

```bash
git clone https://github.com/tylerdotai/agent-os.git
cd agent-os/runtime
cargo build
```

### Run

```bash
cargo run -- --config agent-os.toml
```

By default the sample config listens on `0.0.0.0:8080` and points at an Ollama server on `http://192.168.0.247:11434`.

## Configuration

The runtime loads TOML from `runtime/agent-os.toml` and supports environment overrides for `OLLAMA_URL` and `MODEL`.

Key sections:

- `server` for host and port
- `ollama` and `providers.*` for model routing
- `storage` for persisted task and queue files
- `tools` for tool registry entries
- `permissions` for global allow/deny flags
- `mcp_servers` for external MCP server discovery

## HTTP API

Core endpoints exposed by the runtime:

| Method | Path | Purpose |
| --- | --- | --- |
| `GET` | `/` | Health check |
| `GET` | `/agents` | List agents |
| `POST` | `/agents` | Spawn an agent |
| `GET` | `/tasks` | List tasks |
| `POST` | `/tasks` | Add a task |
| `GET` | `/tasks/next` | Pop the next queued task |
| `POST` | `/think` | Prompt the active agent |
| `POST` | `/execute` | Execute a tool |
| `GET` | `/tools` | List tools |
| `GET` | `/messages` | Inspect agent messages |
| `POST` | `/process` | Process pending tasks immediately |

## MCP Support

The runtime also exposes MCP-style routes:

- `GET /mcp/tools`
- `POST /mcp/execute`
- `GET /mcp/agents`
- `GET /mcp/tasks`
- `POST /mcp/tasks`
- `POST /mcp/discover`
- `POST /mcp/servers`

## Example

```bash
curl -X POST http://localhost:8080/tasks \
  -H "Content-Type: application/json" \
  -d '{"description":"List files in the current directory"}'
```

## Current Limitations

- This is still a prototype and several defaults are tailored to Tyler's local infrastructure
- Task persistence exists, but broader agent memory and production hardening are still incomplete
- There is no authentication layer on the HTTP API yet
- The README describes the implemented runtime, not the full long-term "OS" vision in `SPEC.md`

## Related Docs

- `SPEC.md` for the higher-level product direction
- `REQUIREMENTS.md` for current functional expectations
- `TODO.md` for upcoming implementation work

## License

MIT. See `LICENSE`.
