# Agent OS - Requirements

## Core Purpose
An operating system for autonomous AI agents that receives tasks, completes them using tools, and persists state.

## Functional Requirements

### 1. Task Queue
- Add tasks via HTTP API
- Process tasks automatically (autonomous loop)
- Track status: pending, processing, completed, failed

### 2. Agent Reasoning
- Use LLM (Ollama) to reason about tasks
- Decide which tools to use
- Execute tools and incorporate results

### 3. Tool Execution
- Built-in tools: get_time, list_directory, read_file, http_get, search_web, execute_command, spawn_agent, send_message
- Tools return structured results to agent

### 4. Persistence
- Save agent state to disk
- Survive restarts
- Remember conversation history

### 5. HTTP API
- Health check
- Task management (add, list, get next)
- Tool execution

### 6. MCP Server (EXISTING)
- JSON-RPC 2.0 endpoint
- Expose tools as MCP
- External tools can connect

### 7. Tool Permissions (EXISTING)
- Per-tool permission categories: network, filesystem, execute, spawn
- Global permission settings

### 8. Private Inference (EXISTING)
- Route sensitive tasks to local models
- Separate public/private Ollama endpoints

## Non-Functional Requirements

### Performance
- Task processing < 30s for simple tasks
- Tool execution < 5s timeout

### Reliability
- Auto-restart on crash
- Log errors to file

## Out of Scope (v1)
- Multi-node deployment
- Bare metal kernel
