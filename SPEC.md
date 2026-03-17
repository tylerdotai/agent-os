# Agent OS - Specification

## Overview
A system that runs autonomous agents which receive tasks, use tools to complete them, and remember across restarts.

## User Stories

1. **As a user**, I can add a task to a queue so the agent can process it
2. **As a user**, I can see the status of my tasks (pending, completed, failed)
3. **As an agent**, I can use tools (read files, run commands, search web) to complete tasks
4. **As a system**, I persist agent state so it remembers past tasks
5. **As a system**, I expose tools via MCP protocol
6. **As a user**, I can restrict which tools an agent can use

## API Specification

### Endpoints

| Method | Path | Description |
|--------|------|-------------|
| GET | / | Health check |
| POST | /tasks | Add task |
| GET | /tasks | List all tasks |
| GET | /tasks/next | Get next pending task |
| POST | /think | Prompt agent (manual) |
| POST | /process | Process pending tasks (manual) |
| GET | /agents | List agents |
| POST | /agents | Spawn agent |
| GET | /tools | List tools |
| POST | /execute | Execute tool |

### MCP Endpoints

| Method | Path | Description |
|--------|------|-------------|
| GET | /mcp/tools | List tools (MCP format) |
| POST | /mcp/execute | Execute tool (JSON-RPC 2.0) |
| POST | /mcp/servers | Add external MCP server |

### Data Models

```
Task {
  id: UUID
  description: string
  status: "pending" | "processing" | "completed" | "failed"
  result: string?
  error: string?
  created_at: timestamp
  completed_at: timestamp?
}

Agent {
  id: UUID
  name: string
  system_prompt: string
  context: Message[]
}

Tool {
  name: string
  description: string
  permissions: string[]  // network, filesystem, execute, spawn
}
```

## Configuration

All configuration via TOML file:
- Server port
- Ollama URL and model
- Private inference settings
- Tool definitions
- Permission settings
- External MCP servers

## Acceptance Criteria

- [x] Can add task to queue
- [x] Can process task using LLM + tools
- [x] Task status updates correctly
- [ ] Agent persists across restarts
- [x] Health endpoint responds
- [x] Core tools work (get_time, list_directory)
- [x] MCP server endpoint responds
- [x] Tool permissions work
- [x] Private inference routing works
