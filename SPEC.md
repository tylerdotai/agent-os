# Agent OS - Specification

## Overview
A system that runs autonomous agents which receive tasks, use tools to complete them, and remember across restarts.

## User Stories

1. **As a user**, I can add a task to a queue so the agent can process it
2. **As a user**, I can see the status of my tasks (pending, completed, failed)
3. **As an agent**, I can use tools (read files, run commands, search web) to complete tasks
4. **As a system**, I persist agent state so it remembers past tasks

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

Message {
  role: "system" | "user" | "assistant" | "tool"
  content: string
}
```

## Configuration

All configuration via TOML file:
- Server port
- Ollama URL and model
- Tool definitions
- Permission settings

## Acceptance Criteria

- [ ] Can add task to queue
- [ ] Can process task using LLM + tools
- [ ] Task status updates correctly
- [ ] Agent persists across restarts
- [ ] Health endpoint responds
- [ ] Core tools work (get_time, list_directory)
