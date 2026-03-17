# Agent OS Development Prompt

You're an autonomous AI agent helping build Agent OS (codeberg.org/tylerdotai/agent-os).

## Context

Agent OS is an operating system for AI agents — managing context, capabilities, and identity. After studying NVIDIA GTC 2026 (NeMoClaw, OpenShell, Dynamo), we've updated our roadmap.

## Current Architecture

- **Runtime**: Rust + Axum HTTP API (`runtime/src/main.rs`)
- **Kernel Module**: Linux kernel context tracking (`kernel-module/agent_os.c`)
- **Storage**: `/var/agent-os/storage`
- **LLM**: Connects to Ollama at `http://192.168.0.247:11434` (Titan)

## Priority Tasks (in order)

### 1. YAML Workflow Config
Replace hardcoded `init_tools()` with YAML configuration.

**Current:** Tools are hardcoded in `runtime/src/main.rs`
```rust
tools.insert("get_time".to_string(), Tool { ... });
```

**Goal:** Load from `agent-os.yml`:
```yaml
tools:
  - name: get_time
    description: "Get current timestamp"
  - name: search_web
    url: http://192.168.0.247:18080
    type: http

agents:
  - name: researcher
    model: qwen3.5:35b-a3b
    tools: [get_time, search_web]
```

**Files to modify:**
- `runtime/Cargo.toml` — add `serde`, `serde_yaml`
- `runtime/src/main.rs` — add config loader

### 2. MCP Server Export
Expose Agent OS as an MCP server so OpenClaw/Cursor can use it.

**Reference:** `../NeMo-Agent-Toolkit/packages/nvidia_nat_mcp/`

**What to add:**
- `/mcp` endpoint that lists tools
- Tool call handler via `/execute`

### 3. MCP Client
Connect to external MCP tools on Titan:
- **Qdrant**: `http://192.168.0.247:6333` (vector DB)
- **SearXNG**: `http://192.168.0.247:18080` (search)

**Reference:** NeMo's `mcp_client` function group

### 4. Tool Permissions (Deny-by-Default)
Following OpenShell pattern:

```rust
struct ToolPermission {
    tool_name: String,
    allowed: bool,
    requires_approval: bool,
}
```

- Agent starts with ZERO permissions
- User explicitly grants access
- Add `/permissions` API endpoint

### 5. Observability
Add OpenTelemetry tracing:

```rust
// Trace tool calls
tracing::info!(tool = %tool_name, "Executing tool");
```

Add to `Cargo.toml`:
```toml
tracing-subscriber = "0.3"
```

## Instructions

1. Start with **Task 1 (YAML Config)** — it's foundational
2. Work in `runtime/src/main.rs`
3. Test locally: `cargo run -- --config agent-os.yml`
4. Commit often: `git add . && git commit -m "feat: add YAML config loading"`

## Tools Available

You have access to:
- `read`, `write`, `edit` — file operations
- `exec` — run commands (cargo, git, etc.)
- `grep` — search code

## Success Criteria

- [ ] Agent loads config from YAML file
- [ ] Tools are dynamically registered
- [ ] Can add/remove tools via config
- [ ] MCP server endpoint responds

Start working on YAML config loading. Read `runtime/src/main.rs` first to understand current structure.
