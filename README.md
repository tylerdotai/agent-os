<!-- Improved compatibility of back to top link: See: https://github.com/othneildrew/Best-README-Template/pull/73 -->
<a id="readme-top"></a>

[![Contributors][contributors-shield]][contributors-url]
[![Forks][forks-shield]][forks-url]
[![Stargazers][stars-shield]][stars-url]
[![Issues][issues-shield]][issues-shield-url]
[![License][license-shield]][license-url]

<!-- PROJECT LOGO -->
<br />
<div align="center">
  <a href="https://codeberg.org/tylerdotai/agent-os">
    <img src="assets/IMG_7277.jpeg" alt="Agent OS Logo" width="120">
  </a>

  <h3 align="center">Agent OS</h3>

  <p align="center">
    An operating system designed specifically for autonomous AI agents.
    <br />
    <a href="https://codeberg.org/tylerdotai/agent-os"><strong>Explore the docs »</strong></a>
    <br />
    <br />
    <a href="https://codeberg.org/tylerdotai/agent-os/issues">Report Bug</a>
    ·
    <a href="https://codeberg.org/tylerdotai/agent-os/issues">Request Feature</a>
  </p>
</div>

<!-- TABLE OF CONTENTS -->
<details>
  <summary>Table of Contents</summary>
  <ol>
    <li><a href="#about">About</a></li>
    <li><a href="#features">Features</a></li>
    <li><a href="#architecture">Architecture</a></li>
    <li><a href="#getting-started">Getting Started</a></li>
    <li><a href="#configuration">Configuration</a></li>
    <li><a href="#api">API Reference</a></li>
    <li><a href="#roadmap">Roadmap</a></li>
    <li><a href="#license">License</a></li>
  </ol>
</details>

<!-- ABOUT -->
## About

Agent OS is a purpose-built operating system for autonomous AI agents. Unlike traditional OSes that manage hardware, Agent OS manages **context**, **tools**, **identity**, and **communication** for agents.

**Core insight:** Context is the new RAM.

<p align="right">(<a href="#readme-top">back to top</a>)</p>

<!-- FEATURES -->
## Features

- **TOML Configuration** — Declarative agent and tool definitions
- **MCP Server** — Expose Agent OS as an MCP server (JSON-RPC 2.0)
- **MCP Client** — Connect to external MCP tools (Qdrant, SearXNG)
- **Tool Permissions** — Deny-by-default access control
- **Private Inference** — Route sensitive tasks to local models
- **Task Queue** — Async task processing with persistence
- **Multi-Agent** — Spawn child agents, inter-agent messaging

<p align="right">(<a href="#readme-top">back to top</a>)</p>

<!-- ARCHITECTURE -->
## Architecture

```
┌─────────────────────────────────────────────────────────┐
│                   Agent OS Runtime                     │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐  │
│  │   Context   │  │    Tool     │  │  Message   │  │
│  │  Manager    │  │  Registry  │  │    Bus     │  │
│  └─────────────┘  └─────────────┘  └─────────────┘  │
│                         │                              │
│                    ┌────┴────┐                       │
│                    │  Ollama │  (qwen3.5:35b-a3b)    │
│                    └─────────┘                       │
└─────────────────────────────────────────────────────────┘
         │
    ┌────┴────┐
    │  Titan   │  (192.168.0.247)
    │ Proxmox │
    └─────────┘
```

<p align="right">(<a href="#readme-top">back to top</a>)</p>

<!-- GETTING STARTED -->
## Getting Started

### Prerequisites

- **Titan** — AMD Ryzen AI MAX+ 395 at 192.168.0.247
- Proxmox or Linux container

### Quick Start

```bash
# Clone
git clone git@codeberg.org:tylerdotai/agent-os.git
cd agent-os/runtime

# Build
cargo build --release

# Run
OLLAMA_URL=http://192.168.0.247:11434 MODEL=qwen3.5:35b-a3b \
  ./target/release/agent-os
```

### With Config

```bash
./target/release/agent-os --config agent-os.toml
```

<p align="right">(<a href="#readme-top">back to top</a>)</p>

<!-- CONFIGURATION -->
## Configuration

Create `agent-os.toml`:

```toml
[server]
port = 8080

[ollama]
url = "http://192.168.0.247:11434"
model = "qwen3.5:35b-a3b"

# Private inference for sensitive tasks
[ollama.private]
url = "http://192.168.0.247:11434"
model = "qwen3.5:35b-a3b"

[[tools]]
name = "get_time"
description = "Get current timestamp"
permissions = []

[[tools]]
name = "execute_command"
description = "Run shell command"
permissions = ["execute"]

[permissions]
allow_spawn = true
allow_network = true
allow_filesystem = true
allow_execute = true
```

<p align="right">(<a href="#readme-top">back to top</a>)</p>

<!-- API -->
## API Reference

| Endpoint | Method | Description |
|---------|--------|-------------|
| `/` | GET | Health check |
| `/agents` | GET/POST | List/spawn agents |
| `/tasks` | GET/POST | List/add tasks |
| `/tasks/next` | GET | Get next pending task |
| `/think` | POST | Prompt agent |
| `/execute` | POST | Execute tool |
| `/tools` | GET | List tools |
| `/messages` | GET | Get messages |
| `/process` | POST | Process all pending tasks |

### MCP Endpoints

| Endpoint | Method | Description |
|---------|--------|-------------|
| `/mcp/tools` | GET | List tools (MCP format) |
| `/mcp/execute` | POST | Execute tool (JSON-RPC 2.0) |
| `/mcp/agents` | POST | List agents |
| `/mcp/tasks` | GET/POST | List/add tasks |
| `/mcp/discover` | POST | Discover MCP tools |
| `/mcp/servers` | POST | Add MCP server |

### Example: Execute Tool

```bash
curl -X POST http://localhost:8080/mcp/execute \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc": "2.0",
    "id": 1,
    "method": "tools/call",
    "params": {
      "name": "get_time",
      "arguments": {}
    }
  }'
```

<p align="right">(<a href="#readme-top">back to top</a>)</p>

<!-- ROADMAP -->
## Roadmap

### GTC 2026 Priorities

- [x] **YAML Workflow Config** — Declarative agent/tool definitions
- [x] **MCP Server Export** — Expose as MCP server
- [x] **MCP Client** — Connect to external MCP tools
- [x] **Tool Permissions** — Deny-by-default access control
- [x] **Private Inference Routing** — Route sensitive to local
- [ ] **Observability** — OpenTelemetry tracing
- [ ] **Evaluation Framework** — Test agent task completion

### Future

- [ ] Port to seL4 microkernel
- [ ] Bare metal support
- [ ] Native GPU scheduling

<p align="right">(<a href="#readme-top">back to top</a>)</p>

<!-- LICENSE -->
## License

Distributed under the MIT License. See `LICENSE` for more information.

---

**Built by Tyler Delano** — [@tylerdotai](https://x.com/tylerdotai)

<p align="right">(<a href="#readme-top">back to top</a>)</p>

<!-- MARKDOWN LINKS -->
[contributors-shield]: https://img.shields.io/badge/contributors-1-orange?style=for-the-badge
[contributors-url]: https://codeberg.org/tylerdotai/agent-os/-/graphs/contributors
[forks-shield]: https://img.shields.io/badge/forks-1-black?style=for-the-badge
[forks-url]: https://codeberg.org/tylerdotai/agent-os/-/forks
[stars-shield]: https://img.shields.io/badge/stars-1-black?style=for-the-badge
[stars-url]: https://codeberg.org/tylerdotai/agent-os
[issues-shield]: https://img.shields.io/badge/issues-1-black?style=for-the-badge
[issues-shield-url]: https://codeberg.org/tylerdotai/agent-os/issues
[license-shield]: https://img.shields.io/badge/license-MIT-black?style=for-the-badge
[license-url]: https://codeberg.org/tylerdotai/agent-os/blob/main/LICENSE
