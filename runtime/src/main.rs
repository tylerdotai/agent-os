//! Agent OS - Operating System for Autonomous AI Agents
//!
//! This is NOT a chat interface. This is an OS for agents to:
//! - Receive tasks from a queue
//! - Reason about tasks using LLM
//! - Execute tools to interact with the world
//! - Learn from results and persist state
//! - Communicate with other agents

use anyhow::Result;
use axum::{
    routing::{get, post, delete},
    Router, Json,
    extract::State,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock as TokioRwLock;
use uuid::Uuid;
use chrono::{DateTime, Utc};
use std::sync::atomic::{AtomicU64, Ordering};
use std::path::PathBuf;

// ============================================================================
// Core Types
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentIdentity {
    pub id: Uuid,
    pub name: String,
    pub parent_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Permissions {
    pub can_spawn_agents: bool,
    pub can_access_network: bool,
    pub can_access_filesystem: bool,
    pub can_execute_commands: bool,
    pub can_spend_money: bool,
}

impl Default for Permissions {
    fn default() -> Self {
        // Agents get permissions by default - this IS an agent OS
        Self {
            can_spawn_agents: true,
            can_access_network: true,
            can_access_filesystem: true,
            can_execute_commands: true,
            can_spend_money: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Quotas {
    pub max_context_tokens: usize,
    pub max_tools_per_minute: usize,
}

impl Default for Quotas {
    fn default() -> Self {
        Self {
            max_context_tokens: 128_000,
            max_tools_per_minute: 60,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextPage {
    pub id: Uuid,
    pub role: String,        // "system", "user", "assistant", "tool"
    pub content: String,
    pub token_count: usize,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tool {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub tool: String,
    pub parameters: serde_json::Value,
    pub result: Option<serde_json::Value>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: Uuid,
    pub description: String,
    pub status: TaskStatus,
    pub result: Option<String>,
    pub tool_calls: Vec<ToolCall>,
    pub created_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TaskStatus {
    Pending,
    Running,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Agent {
    pub identity: AgentIdentity,
    pub permissions: Permissions,
    pub quotas: Quotas,
    pub context: Vec<ContextPage>,
    pub system_prompt: String,
}

impl Agent {
    pub fn new(name: String, parent_id: Option<Uuid>, system_prompt: &str) -> Self {
        Self {
            identity: AgentIdentity {
                id: Uuid::new_v4(),
                name,
                parent_id,
                created_at: Utc::now(),
            },
            permissions: Permissions::default(),
            quotas: Quotas::default(),
            context: Vec::new(),
            system_prompt: system_prompt.to_string(),
        }
    }

    pub fn total_tokens(&self) -> usize {
        self.context.iter().map(|p| p.token_count).sum()
    }

    pub fn add_context(&mut self, role: &str, content: &str) {
        let token_count = content.len() / 4;
        
        // Evict old messages if over budget
        while self.total_tokens() + token_count > self.quotas.max_context_tokens
              && self.context.len() > 1 {  // Keep system prompt
            if self.context.len() > 1 && self.context[1].role != "system" {
                self.context.remove(1);
            } else {
                break;
            }
        }

        self.context.push(ContextPage {
            id: Uuid::new_v4(),
            role: role.to_string(),
            content: content.to_string(),
            token_count,
            timestamp: Utc::now(),
        });
    }
}

// ============================================================================
// Agent OS State
// ============================================================================

pub struct AgentOsState {
    pub agents: Arc<TokioRwLock<HashMap<Uuid, Agent>>>,
    pub tasks: Arc<TokioRwLock<HashMap<Uuid, Task>>>,
    pub task_queue: Arc<TokioRwLock<Vec<Uuid>>>,
    pub tools: Arc<TokioRwLock<HashMap<String, Tool>>>,
    pub init_agent_id: Uuid,
    pub ollama_url: String,
    pub storage_path: PathBuf,
    pub tool_call_count: AtomicU64,
}

impl AgentOsState {
    pub fn new(ollama_url: &str, storage_path: PathBuf) -> Self {
        let system_prompt = r#"You are an autonomous AI agent running on Agent OS.
Your job is to:
1. Receive tasks from the queue
2. Reason about what needs to be done
3. Use tools to accomplish tasks
4. Report results

You have access to tools. Use them proactively.
Always explain your reasoning."#;

        let init_agent = Agent::new("init".to_string(), None, system_prompt);
        let init_agent_id = init_agent.identity.id;
        
        let agents = Arc::new(TokioRwLock::new(HashMap::new()));
        let tasks = Arc::new(TokioRwLock::new(HashMap::new()));
        let task_queue = Arc::new(TokioRwLock::new(Vec::new()));
        let tools = Arc::new(TokioRwLock::new(HashMap::new()));

        // Spawn init agent
        let agents_clone = agents.clone();
        tokio::spawn(async move {
            let mut agents = agents_clone.write().await;
            agents.insert(init_agent_id, init_agent);
        });

        Self {
            agents,
            tasks,
            task_queue,
            tools,
            init_agent_id,
            ollama_url: ollama_url.to_string(),
            storage_path,
            tool_call_count: AtomicU64::new(0),
        }
    }

    pub async fn init_tools(&self) {
        let mut tools = self.tools.write().await;
        
        // Core tools for agent operation
        tools.insert("http_get".to_string(), Tool {
            name: "http_get".to_string(),
            description: "Make an HTTP GET request".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "url": {"type": "string", "description": "URL to fetch"}
                },
                "required": ["url"]
            }),
        });

        tools.insert("http_post".to_string(), Tool {
            name: "http_post".to_string(),
            description: "Make an HTTP POST request".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "url": {"type": "string"},
                    "body": {"type": "object"}
                },
                "required": ["url", "body"]
            }),
        });

        tools.insert("search_web".to_string(), Tool {
            name: "search_web".to_string(),
            description: "Search the web using SearXNG".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string", "description": "Search query"}
                },
                "required": ["query"]
            }),
        });

        tools.insert("read_file".to_string(), Tool {
            name: "read_file".to_string(),
            description: "Read a file from the filesystem".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string"}
                },
                "required": ["path"]
            }),
        });

        tools.insert("write_file".to_string(), Tool {
            name: "write_file".to_string(),
            description: "Write content to a file".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string"},
                    "content": {"type": "string"}
                },
                "required": ["path", "content"]
            }),
        });

        tools.insert("list_directory".to_string(), Tool {
            name: "list_directory".to_string(),
            description: "List files in a directory".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "default": "."}
                },
                "required": ["path"]
            }),
        });

        tools.insert("execute_command".to_string(), Tool {
            name: "execute_command".to_string(),
            description: "Execute a shell command".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "command": {"type": "string"}
                },
                "required": ["command"]
            }),
        });

        tools.insert("get_time".to_string(), Tool {
            name: "get_time".to_string(),
            description: "Get current date and time".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {}
            }),
        });
    }

    pub async fn spawn_agent(&self, name: String, parent_id: Option<Uuid>) -> Result<Uuid> {
        let system_prompt = format!(
            "You are {}, an autonomous agent. Your job is to complete tasks from the queue using available tools.",
            name
        );
        let agent = Agent::new(name, parent_id, &system_prompt);
        let id = agent.identity.id;
        
        let mut agents = self.agents.write().await;
        agents.insert(id, agent);
        
        self.persist_agent(&id).await?;
        
        Ok(id)
    }

    pub async fn add_task(&self, description: String) -> Result<Uuid> {
        let task = Task {
            id: Uuid::new_v4(),
            description,
            status: TaskStatus::Pending,
            result: None,
            tool_calls: Vec::new(),
            created_at: Utc::now(),
            completed_at: None,
        };
        
        let task_id = task.id;
        
        let mut tasks = self.tasks.write().await;
        tasks.insert(task_id, task);
        
        let mut queue = self.task_queue.write().await;
        queue.push(task_id);
        
        tracing::info!("Task added to queue: {}", task_id);
        
        return Ok(task_id);
    }

    pub async fn get_next_task(&self) -> Option<Uuid> {
        let mut queue = self.task_queue.write().await;
        queue.pop()
    }

    pub async fn execute_tool(&self, agent_id: Uuid, tool_name: &str, params: &serde_json::Value) -> Result<serde_json::Value> {
        self.tool_call_count.fetch_add(1, Ordering::Relaxed);
        
        let tools = self.tools.read().await;
        let tool = tools.get(tool_name)
            .ok_or_else(|| anyhow::anyhow!("Tool not found: {}", tool_name))?;
        
        let agents = self.agents.read().await;
        let agent = agents.get(&agent_id)
            .ok_or_else(|| anyhow::anyhow!("Agent not found"))?;
        
        // Check permissions
        match tool_name {
            "http_get" | "http_post" | "search_web" => {
                if !agent.permissions.can_access_network {
                    return Err(anyhow::anyhow!("Permission denied: network access required"));
                }
            }
            "read_file" | "write_file" | "list_directory" => {
                if !agent.permissions.can_access_filesystem {
                    return Err(anyhow::anyhow!("Permission denied: filesystem access required"));
                }
            }
            "execute_command" => {
                if !agent.permissions.can_execute_commands {
                    return Err(anyhow::anyhow!("Permission denied: command execution required"));
                }
            }
            _ => {}
        }
        
        drop(agents);  // Release lock
        
        // Execute tool
        let result = match tool_name {
            "http_get" => {
                let url = params["url"].as_str().unwrap_or("");
                let client = reqwest::Client::new();
                let resp = client.get(url).send().await?;
                serde_json::json!({"status": resp.status(), "body": resp.text().await?})
            }
            "http_post" => {
                let url = params["url"].as_str().unwrap_or("");
                let body = &params["body"];
                let client = reqwest::Client::new();
                let resp = client.post(url).json(body).send().await?;
                serde_json::json!({"status": resp.status(), "body": resp.text().await?})
            }
            "search_web" => {
                let query = params["query"].as_str().unwrap_or("");
                let client = reqwest::Client::new();
                let resp = client.get(&format!("http://192.168.0.247:18080/search?q={}", urlencoding::encode(query)))
                    .send().await?;
                let text = resp.text().await?;
                serde_json::json!({"results": text.chars().take(2000).collect::<String>()})
            }
            "read_file" => {
                let path = params["path"].as_str().unwrap_or("");
                let content = tokio::fs::read_to_string(path).await?;
                serde_json::json!({"content": content})
            }
            "write_file" => {
                let path = params["path"].as_str().unwrap_or("");
                let content = params["content"].as_str().unwrap_or("");
                tokio::fs::write(path, content).await?;
                serde_json::json!({"success": true})
            }
            "list_directory" => {
                let path = params["path"].as_str().unwrap_or(".");
                let mut entries = tokio::fs::read_dir(path).await?;
                let mut names = Vec::new();
                while let Some(entry) = entries.next_entry().await? {
                    names.push(entry.file_name().to_string_lossy().to_string());
                }
                serde_json::json!({"files": names})
            }
            "get_time" => {
                serde_json::json!({"time": Utc::now().to_rfc3339()})
            }
            _ => serde_json::json!({"error": "Tool not implemented"})
        };
        
        Ok(result)
    }

    pub async fn think(&self, agent_id: Uuid, prompt: &str) -> Result<String> {
        let mut agents = self.agents.write().await;
        let agent = agents.get_mut(&agent_id)
            .ok_or_else(|| anyhow::anyhow!("Agent not found"))?;
        
        // Add user message
        agent.add_context("user", prompt);
        
        // Build messages for Ollama
        let messages: Vec<serde_json::Value> = agent.context.iter().map(|p| {
            serde_json::json!({
                "role": p.role,
                "content": p.content
            })
        }).collect();
        
        // Add tools to system prompt
        let tools = self.tools.read().await;
        let tool_descriptions: Vec<String> = tools.values().map(|t| {
            format!("- {}: {}", t.name, t.description)
        }).collect();
        drop(tools);
        
        // Call Ollama
        let client = reqwest::Client::new();
        let mut request_body = serde_json::json!({
            "model": "qwen3:8b",
            "messages": messages,
            "stream": false
        });
        
        let response = client.post(format!("{}/api/chat", self.ollama_url))
            .json(&request_body)
            .send()
            .await?;
        
        let result: serde_json::Value = response.json().await;
        
        let assistant_message = result["message"]["content"]
            .as_str()
            .unwrap_or("")
            .to_string();
        
        // Add assistant response to context
        agent.add_context("assistant", &assistant_message);
        
        // Persist agent state
        drop(agents);
        self.persist_agent(&agent_id).await?;
        
        Ok(assistant_message)
    }

    async fn persist_agent(&self, agent_id: &Uuid) -> Result<()> {
        let agents = self.agents.read().await;
        if let Some(agent) = agents.get(agent_id) {
            let path = self.storage_path.join(format!("{}.json", agent_id));
            let json = serde_json::to_string_pretty(agent)?;
            tokio::fs::write(&path, json).await?;
        }
        Ok(())
    }
}

// ============================================================================
// URL encoding helper
// ============================================================================
mod urlencoding {
    pub fn encode(s: &str) -> String {
        let mut result = String::new();
        for c in s.chars() {
            match c {
                'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' | '.' | '~' => result.push(c),
                ' ' => result.push_str("%20"),
                _ => {
                    for b in c.to_string().as_bytes() {
                        result.push_str(&format!("%{:02X}", b));
                    }
                }
            }
        }
        result
    }
}

// ============================================================================
// HTTP API (for OTHER agents/systems to interact with this OS)
// ============================================================================

#[derive(Serialize)]
struct ApiResponse<T> {
    success: bool,
    data: Option<T>,
    error: Option<String>,
}

#[derive(Deserialize)]
struct SpawnRequest {
    name: String,
    parent_id: Option<Uuid>,
}

#[derive(Deserialize)]
struct TaskRequest {
    description: String,
}

#[derive(Deserialize)]
struct ThinkRequest {
    agent_id: Option<Uuid>,
    prompt: String,
}

#[derive(Deserialize)]
struct ToolCallRequest {
    agent_id: Option<Uuid>,
    tool: String,
    parameters: serde_json::Value,
}

async fn spawn_agent(
    State(state): State<AgentOsState>,
    Json(req): Json<SpawnRequest>,
) -> Json<ApiResponse<Uuid>> {
    match state.spawn_agent(req.name, req.parent_id).await {
        Ok(id) => Json(ApiResponse {
            success: true,
            data: Some(id),
            error: None,
        }),
        Err(e) => Json(ApiResponse {
            success: false,
            data: None,
            error: Some(e.to_string()),
        }),
    }
}

async fn add_task(
    State(state): State<AgentOsState>,
    Json(req): Json<TaskRequest>,
) -> Json<ApiResponse<Uuid>> {
    match state.add_task(req.description).await {
        Ok(id) => Json(ApiResponse {
            success: true,
            data: Some(id),
            error: None,
        }),
        Err(e) => Json(ApiResponse {
            success: false,
            data: None,
            error: Some(e.to_string()),
        }),
    }
}

async fn get_task(
    State(state): State<AgentOsState>,
) -> Json<ApiResponse<Option<Uuid>>> {
    let task_id = state.get_next_task().await;
    Json(ApiResponse {
        success: true,
        data: Some(task_id),
        error: None,
    })
}

async fn think(
    State(state): State<AgentOsState>,
    Json(req): Json<ThinkRequest>,
) -> Json<ApiResponse<String>> {
    let agent_id = req.agent_id.unwrap_or(state.init_agent_id);
    
    match state.think(agent_id, &req.prompt).await {
        Ok(response) => Json(ApiResponse {
            success: true,
            data: Some(response),
            error: None,
        }),
        Err(e) => Json(ApiResponse {
            success: false,
            data: None,
            error: Some(e.to_string()),
        }),
    }
}

async fn execute_tool(
    State(state): State<AgentOsState>,
    Json(req): Json<ToolCallRequest>,
) -> Json<ApiResponse<serde_json::Value>> {
    let agent_id = req.agent_id.unwrap_or(state.init_agent_id);
    
    match state.execute_tool(agent_id, &req.tool, &req.parameters).await {
        Ok(result) => Json(ApiResponse {
            success: true,
            data: Some(result),
            error: None,
        }),
        Err(e) => Json(ApiResponse {
            success: false,
            data: None,
            error: Some(e.to_string()),
        }),
    }
}

async fn list_agents(
    State(state): State<AgentOsState>,
) -> Json<ApiResponse<Vec<AgentIdentity>>> {
    let agents = state.agents.read().await;
    let identities: Vec<AgentIdentity> = agents.values()
        .map(|a| a.identity.clone())
        .collect();
    
    Json(ApiResponse {
        success: true,
        data: Some(identities),
        error: None,
    })
}

async fn list_tools(
    State(state): State<AgentOsState>,
) -> Json<ApiResponse<Vec<Tool>>> {
    let tools = state.tools.read().await;
    let tool_list: Vec<Tool> = tools.values().cloned().collect();
    
    Json(ApiResponse {
        success: true,
        data: Some(tool_list),
        error: None,
    })
}

async fn get_stats(
    State(state): State<AgentOsState>,
) -> Json<ApiResponse<serde_json::Value>> {
    let agents = state.agents.read().await;
    let tasks = state.tasks.read().await;
    let tool_calls = state.tool_call_count.load(Ordering::Relaxed);
    
    Json(ApiResponse {
        success: true,
        data: Some(serde_json::json!({
            "agents": agents.len(),
            "tasks": tasks.len(),
            "tool_calls": tool_calls,
        })),
        error: None,
    })
}

// ============================================================================
// Main
// ============================================================================

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let ollama_url = std::env::var("OLLAMA_URL")
        .unwrap_or_else(|_| "http://192.168.0.247:11434".to_string());
    
    let storage_path = std::env::var("STORAGE_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/var/agent-os/storage"));
    
    // Create storage directory
    tokio::fs::create_dir_all(&storage_path).await?;

    tracing::info!("Agent OS starting...");
    tracing::info!("Ollama: {}", ollama_url);
    tracing::info!("Storage: {:?}", storage_path);

    let state = AgentOsState::new(&ollama_url, storage_path);
    state.init_tools().await;

    tracing::info!("Init agent: {}", state.init_agent_id);

    let app = Router::new()
        // Agent management
        .route("/agents", post(spawn_agent))
        .route("/agents", get(list_agents))
        
        // Task queue (for receiving work)
        .route("/tasks", post(add_task))
        .route("/tasks/next", get(get_task))
        
        // Thinking (LLM)
        .route("/think", post(think))
        
        // Tool execution
        .route("/tools", get(list_tools))
        .route("/execute", post(execute_tool))
        
        // Stats
        .route("/stats", get(get_stats))
        
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await?;
    tracing::info!("Agent OS API listening on http://0.0.0.0:3000");
    tracing::info!("Endpoints:");
    tracing::info!("  POST /agents - Spawn new agent");
    tracing::info!("  GET  /agents - List agents");
    tracing::info!("  POST /tasks - Add task to queue");
    tracing::info!("  GET  /tasks/next - Get next task");
    tracing::info!("  POST /think - Have agent think");
    tracing::info!("  GET  /tools - List available tools");
    tracing::info!("  POST /execute - Execute a tool");

    axum::serve(listener, app).await?;

    Ok(())
}
