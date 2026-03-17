//! Agent OS - Operating System for Autonomous AI Agents
//!
//! Built for agents to consume programmatically.
//! Configurable via YAML: cargo run -- --config agent-os.toml

use anyhow::Result;
use axum::{
    routing::{get, post},
    Router, Json,
    extract::State,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock as TokioRwLock;
use std::path::PathBuf;
use uuid::Uuid;
use chrono::{DateTime, Utc};

// ============================================================================
// Configuration
// ============================================================================

#[derive(Debug, Clone, Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub server: ServerConfig,
    #[serde(default)]
    pub ollama: OllamaConfig,
    #[serde(default)]
    pub storage: StorageConfig,
    #[serde(default)]
    pub system: SystemConfig,
    #[serde(default)]
    pub tools: Vec<ToolConfig>,
    #[serde(default)]
    pub permissions: PermissionsConfig,
    #[serde(default)]
    pub mcp_servers: Vec<McpServerConfig>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ServerConfig {
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
}

fn default_host() -> String { "0.0.0.0".to_string() }
fn default_port() -> u16 { 8080 }

#[derive(Debug, Clone, Deserialize, Default)]
pub struct OllamaConfig {
    #[serde(default = "default_ollama_url")]
    pub url: String,
    #[serde(default = "default_model")]
    pub model: String,
    #[serde(default)]
    pub private_url: Option<String>,
    #[serde(default)]
    pub private_model: Option<String>,
    #[serde(default)]
    pub default_private: bool,
}

fn default_ollama_url() -> String { "http://192.168.0.247:11434".to_string() }
fn default_model() -> String { "qwen3.5:35b-a3b".to_string() }

#[derive(Debug, Clone, Deserialize, Default)]
pub struct StorageConfig {
    #[serde(default = "default_storage_path")]
    pub path: String,
}

fn default_storage_path() -> String { "/var/agent-os/storage".to_string() }

#[derive(Debug, Clone, Deserialize, Default)]
pub struct SystemConfig {
    #[serde(default = "default_system_prompt")]
    pub system_prompt: String,
}

fn default_system_prompt() -> String { 
    "You are an autonomous AI agent. Complete tasks using tools.\nWhen done, output: TASK_COMPLETE: <result>".to_string() 
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ToolConfig {
    pub name: String,
    pub description: String,
    #[serde(default = "default_handler")]
    pub handler: String,
    #[serde(default)]
    pub parameters: Option<serde_json::Value>,
    #[serde(default)]
    pub permissions: Vec<String>,
}

fn default_handler() -> String { "builtin".to_string() }

#[derive(Debug, Clone, Deserialize, Default)]
pub struct PermissionsConfig {
    #[serde(default = "default_true")]
    pub allow_spawn: bool,
    #[serde(default = "default_true")]
    pub allow_network: bool,
    #[serde(default = "default_true")]
    pub allow_filesystem: bool,
    #[serde(default = "default_true")]
    pub allow_execute: bool,
}

fn default_true() -> bool { true }


// ============================================================================
// MCP Client - Connect to External MCP Servers
// ============================================================================

#[derive(Debug, Clone, Deserialize, Default)]
pub struct McpServerConfig {
    pub url: String,
    pub name: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct McpClientConfig {
    pub servers: Vec<McpServerConfig>,
}

pub struct McpClient {
    pub servers: Vec<McpServerConfig>,
}

impl McpClient {
    pub fn new(servers: Vec<McpServerConfig>) -> Self {
        Self { servers }
    }
    
    pub async fn list_tools(&self, server_url: &str) -> Result<Vec<Tool>> {
        let client = reqwest::Client::new();
        let resp = client.post(server_url)
            .json(&serde_json::json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "tools/list"
            }))
            .send()
            .await?;
        
        let result: serde_json::Value = resp.json().await?;
        
        let mut tools = Vec::new();
        if let Some(tool_list) = result["result"]["tools"].as_array() {
            for t in tool_list {
                tools.push(Tool {
                    name: format!("mcp:{}", t["name"].as_str().unwrap_or("")),
                    description: t["description"].as_str().unwrap_or("").to_string(),
                    parameters: Some(t["inputSchema"].clone()),
                    permissions: vec!["network".to_string()],
                });
            }
        }
        
        Ok(tools)
    }
    
    pub async fn call_tool(&self, server_url: &str, tool_name: &str, args: serde_json::Value) -> Result<serde_json::Value> {
        let client = reqwest::Client::new();
        let resp = client.post(server_url)
            .json(&serde_json::json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "tools/call",
                "params": {
                    "name": tool_name,
                    "arguments": args
                }
            }))
            .send()
            .await?;
        
        let result: serde_json::Value = resp.json().await?;
        Ok(result["result"].clone())
    }
}


fn load_config() -> Result<Config> {
    // Check for --config arg
    let args: Vec<String> = std::env::args().collect();
    let config_path = args.iter()
        .position(|a| a == "--config")
        .and_then(|i| args.get(i + 1))
        .unwrap_or(&"agent-os.toml".to_string())
        .clone();
    
    let content = std::fs::read_to_string(&config_path)?;
    let mut config: Config = toml::from_str(&content)?;
    
    // Allow env var overrides
    if let Ok(url) = std::env::var("OLLAMA_URL") {
        config.ollama.url = url;
    }
    if let Ok(model) = std::env::var("MODEL") {
        config.ollama.model = model;
    }
    
    Ok(config)
}

// ============================================================================
// Core Types
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Agent {
    pub id: Uuid,
    pub name: String,
    pub parent_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub system_prompt: String,
    pub context: Vec<Message>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: String,
    pub tool_call_id: Option<String>,
    pub tool_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tool {
    pub name: String,
    pub description: String,
    pub parameters: Option<serde_json::Value>,
    pub permissions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: Uuid,
    pub description: String,
    pub status: String,
    pub result: Option<String>,
    pub error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentMessage {
    pub id: Uuid,
    pub from_agent: Uuid,
    pub to_agent: Uuid,
    pub content: String,
    pub timestamp: DateTime<Utc>,
}

// ============================================================================
// Agent OS State
// ============================================================================

pub struct AgentOsState {
    pub agents: Arc<TokioRwLock<HashMap<Uuid, Agent>>>,
    pub tasks: Arc<TokioRwLock<HashMap<Uuid, Task>>>,
    pub task_queue: Arc<TokioRwLock<Vec<Uuid>>>,
    pub tools: Arc<TokioRwLock<HashMap<String, Tool>>>,
    pub messages: Arc<TokioRwLock<Vec<AgentMessage>>>,
    pub ollama_url: String,
    pub model: String,
    pub ollama_url_private: Option<String>,
    pub model_private: Option<String>,
    pub storage_path: PathBuf,
    pub running: Arc<std::sync::atomic::AtomicU64>,
    pub permissions: PermissionsConfig,
    pub mcp_client: McpClient,
}

impl AgentOsState {

    // Persistence stub - save/load tasks
    pub async fn save_state(&self) -> Result<()> {
        let path = self.storage_path.clone();
        
        // Save as simple JSON array to avoid serialization issues
        let tasks_list = {
            let tasks = self.tasks.read().await;
            tasks.values().map(|t| {
                serde_json::json!({
                    "id": t.id.to_string(),
                    "description": t.description,
                    "status": t.status,
                    "result": t.result,
                    "error": t.error,
                    "created_at": t.created_at.to_rfc3339(),
                    "completed_at": t.completed_at.map(|c| c.to_rfc3339())
                })
            }).collect::<Vec<_>>()
        };
        
        let queue: Vec<uuid::Uuid> = self.task_queue.read().await.clone();
        
        let tasks_json = serde_json::to_string_pretty(&tasks_list).unwrap_or_default();
        let queue_json = serde_json::to_string_pretty(&queue).unwrap_or_default();
        
        if let Err(e) = tokio::fs::create_dir_all(&path).await {
            tracing::warn!("Failed to create dir: {}", e);
        }
        if let Err(e) = tokio::fs::write(path.join("tasks.json"), &tasks_json).await {
            tracing::warn!("Failed to write tasks: {}", e);
        }
        if let Err(e) = tokio::fs::write(path.join("queue.json"), &queue_json).await {
            tracing::warn!("Failed to write queue: {}", e);
        }
        
        tracing::info!("State saved ({} tasks)", tasks_list.len());
        Ok(())
    }
    
    pub async fn load_state(&self) -> Result<()> {
        let path = &self.storage_path;
        tracing::info!("Loading state from {:?}", path);
        
        // Load tasks from array
        if let Ok(data) = tokio::fs::read_to_string(path.join("tasks.json")).await {
            tracing::info!("Read tasks.json: {} bytes", data.len());
            if let Ok(list) = serde_json::from_str::<Vec<serde_json::Value>>(&data) {
                let mut tasks = self.tasks.write().await;
                for value in list {
                    if let (Some(id_str), Some(desc), Some(status)) = (
                        value.get("id").and_then(|v| v.as_str()),
                        value.get("description").and_then(|v| v.as_str()),
                        value.get("status").and_then(|v| v.as_str()),
                    ) {
                        if let Ok(id) = uuid::Uuid::parse_str(id_str) {
                            let task = Task {
                                id,
                                description: desc.to_string(),
                                status: status.to_string(),
                                result: value.get("result").and_then(|v| v.as_str()).map(|s| s.to_string()),
                                error: value.get("error").and_then(|v| v.as_str()).map(|s| s.to_string()),
                                created_at: value.get("created_at").and_then(|v| v.as_str())
                                    .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                                    .map(|dt| dt.with_timezone(&chrono::Utc))
                                    .unwrap_or_else(chrono::Utc::now),
                                completed_at: value.get("completed_at").and_then(|v| v.as_str())
                                    .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                                    .map(|dt| dt.with_timezone(&chrono::Utc)),
                            };
                            tasks.insert(id, task);
                        }
                    }
                }
                tracing::info!("Loaded {} tasks", tasks.len());
            }
        }
        
        // Load task queue
        if let Ok(data) = tokio::fs::read_to_string(path.join("queue.json")).await {
            if let Ok(queue_ids) = serde_json::from_str::<Vec<uuid::Uuid>>(&data) {
                let mut queue = self.task_queue.write().await;
                *queue = queue_ids;
                tracing::info!("Loaded {} queued tasks", queue.len());
            }
        }
        
        Ok(())
    }
    pub fn new(config: &Config) -> Self {
        let system_prompt = config.system.system_prompt.clone();

        let init_agent = Agent {
            id: Uuid::new_v4(),
            name: "init".to_string(),
            parent_id: None,
            created_at: Utc::now(),
            system_prompt: system_prompt.clone(),
            context: vec![Message {
                role: "system".to_string(),
                content: system_prompt,
                tool_call_id: None,
                tool_name: None,
            }],
        };
        
        let agents = Arc::new(TokioRwLock::new(HashMap::new()));
        let tasks = Arc::new(TokioRwLock::new(HashMap::new()));
        let task_queue = Arc::new(TokioRwLock::new(Vec::new()));
        let tools = Arc::new(TokioRwLock::new(HashMap::new()));
        let messages = Arc::new(TokioRwLock::new(Vec::new()));

        let agents_clone = agents.clone();
        tokio::spawn(async move {
            let mut agents = agents_clone.write().await;
            agents.insert(init_agent.id, init_agent);
        });

        Self {
            agents,
            tasks,
            task_queue,
            tools,
            messages,
            ollama_url: config.ollama.url.clone(),
            model: config.ollama.model.clone(),
            ollama_url_private: config.ollama.private_url.clone(),
            model_private: config.ollama.private_model.clone(),
            storage_path: PathBuf::from(&config.storage.path),
            running: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            permissions: config.permissions.clone(),
            mcp_client: McpClient::new(config.mcp_servers.clone()),
        }
    }

    // Privacy routing
    fn get_ollama_url(&self, private: bool) -> String {
        if private {
            if let Some(ref url) = self.ollama_url_private {
                return url.clone();
            }
        }
        self.ollama_url.clone()
    }
    
    fn get_ollama_model(&self, private: bool) -> String {
        if private {
            if let Some(ref model) = self.model_private {
                return model.clone();
            }
        }
        self.model.clone()
    }

    pub async fn init_tools(&self, config: &Config) {
        let mut tools = self.tools.write().await;
        
        for tool_config in &config.tools {
            tools.insert(tool_config.name.clone(), Tool {
                name: tool_config.name.clone(),
                description: tool_config.description.clone(),
                parameters: tool_config.parameters.clone(),
                permissions: tool_config.permissions.clone(),
            });
            tracing::info!("Loaded tool: {}", tool_config.name);
        }
    }

    pub async fn think_with_tools(&self, agent_id: Uuid, task: &str, max_turns: usize, private: bool) -> Result<String> {
        let mut agents = self.agents.write().await;
        let agent = agents.get_mut(&agent_id)
            .ok_or_else(|| anyhow::anyhow!("Agent not found"))?;
        
        agent.context.push(Message {
            role: "user".to_string(),
            content: task.to_string(),
            tool_call_id: None,
            tool_name: None,
        });

        for _turn in 0..max_turns {
            let messages: Vec<serde_json::Value> = agent.context.iter().map(|m| {
                serde_json::json!({"role": m.role, "content": m.content})
            }).collect();

            let tools = self.tools.read().await;
            let tools_json: Vec<serde_json::Value> = tools.values().map(|t| {
                serde_json::json!({
                    "type": "function",
                    "function": {"name": t.name, "description": t.description, "parameters": t.parameters}
                })
            }).collect();
            drop(tools);

            let client = reqwest::Client::new();
            let request = serde_json::json!({
                "model": self.get_ollama_model(private),
                "messages": messages,
                "tools": tools_json,
                "stream": false
            });
            
            let response = client.post(format!("{}/api/chat", self.get_ollama_url(private)))
                .json(&request).send().await?;
            
            let result = response.json::<serde_json::Value>().await?;
            let content = result["message"]["content"].as_str().unwrap_or("");
            let tool_calls_opt = result["message"]["tool_calls"].as_array();

            if let Some(calls) = tool_calls_opt {
                if calls.len() > 0 {
                    for call in calls {
                        let tool_name = call["function"]["name"].as_str().unwrap_or("");
                        let args_str = call["function"]["arguments"].to_string();
                        
                        agent.context.push(Message {
                            role: "assistant".to_string(),
                            content: format!("Using tool: {}", tool_name),
                            tool_call_id: None,
                            tool_name: Some(tool_name.to_string()),
                        });
                        
                        let tool_result = self.execute_tool(tool_name, &args_str).await;
                        let result_str = match tool_result {
                            Ok(r) => r.to_string(),
                            Err(e) => format!("Error: {}", e),
                        };
                        
                        agent.context.push(Message {
                            role: "tool".to_string(),
                            content: result_str,
                            tool_call_id: None,
                            tool_name: Some(tool_name.to_string()),
                        });
                    }
                    continue;
                }
            }
            
            agent.context.push(Message {
                role: "assistant".to_string(),
                content: content.to_string(),
                tool_call_id: None,
                tool_name: None,
            });
            
            return Ok(content.to_string());
        }
        
        Ok("Max turns reached".to_string())
    }

    pub fn check_permission(&self, tool_permissions: &[String]) -> Result<()> {
        for perm in tool_permissions {
            match perm.as_str() {
                "network" if !self.permissions.allow_network => {
                    return Err(anyhow::anyhow!("Permission denied: network access not allowed"));
                }
                "filesystem" if !self.permissions.allow_filesystem => {
                    return Err(anyhow::anyhow!("Permission denied: filesystem access not allowed"));
                }
                "execute" if !self.permissions.allow_execute => {
                    return Err(anyhow::anyhow!("Permission denied: execute not allowed"));
                }
                "spawn" if !self.permissions.allow_spawn => {
                    return Err(anyhow::anyhow!("Permission denied: spawn not allowed"));
                }
                _ => {}
            }
        }
        Ok(())
    }

    pub async fn execute_tool(&self, tool_name: &str, args: &str) -> Result<serde_json::Value> {
        // Check tool permissions
        let tool_perms = {
            let tools = self.tools.read().await;
            tools.get(tool_name).map(|t| t.permissions.clone()).unwrap_or_default()
        };
        self.check_permission(&tool_perms)?;
        
        let params: serde_json::Value = serde_json::from_str(args).unwrap_or(serde_json::json!({}));
        
        match tool_name {
            "get_time" => Ok(serde_json::json!({"time": Utc::now().to_rfc3339()})),
            "list_directory" => {
                let path = params["path"].as_str().unwrap_or(".");
                let mut entries = tokio::fs::read_dir(path).await?;
                let mut files = Vec::new();
                while let Some(entry) = entries.next_entry().await? {
                    files.push(entry.file_name().to_string_lossy().to_string());
                }
                Ok(serde_json::json!({"files": files}))
            }
            "read_file" => {
                let path = params["path"].as_str().unwrap_or("");
                let content = tokio::fs::read_to_string(path).await?;
                Ok(serde_json::json!({"content": content}))
            }
            "http_get" => {
                let url = params["url"].as_str().unwrap_or("");
                let client = reqwest::Client::new();
                let resp = client.get(url).send().await?;
                Ok(serde_json::json!({"body": resp.text().await?}))
            }
            "search_web" => {
                let query = params["query"].as_str().unwrap_or("");
                let client = reqwest::Client::new();
                let url = format!("http://192.168.0.247:18080/search?q={}", urlencoding::encode(query));
                let resp = client.get(&url).send().await?;
                Ok(serde_json::json!({"results": resp.text().await?}))
            }
            "execute_command" => {
                let cmd = params["command"].as_str().unwrap_or("");
                let output = tokio::process::Command::new("sh").arg("-c").arg(cmd).output().await?;
                Ok(serde_json::json!({"stdout": String::from_utf8_lossy(&output.stdout), "stderr": String::from_utf8_lossy(&output.stderr)}))
            }
            "spawn_agent" => {
                let name = params["name"].as_str().unwrap_or("child");
                let prompt = params["system_prompt"].as_str().unwrap_or("You are an agent.");
                let agent = Agent {
                    id: Uuid::new_v4(),
                    name: name.to_string(),
                    parent_id: None,
                    created_at: Utc::now(),
                    system_prompt: prompt.to_string(),
                    context: vec![Message { role: "system".to_string(), content: prompt.to_string(), tool_call_id: None, tool_name: None }],
                };
                let id = agent.id;
                let mut agents = self.agents.write().await;
                agents.insert(id, agent);
                Ok(serde_json::json!({"agent_id": id, "name": name}))
            }
            "send_message" => {
                let to = params["to_agent"].as_str().unwrap_or("");
                let content = params["content"].as_str().unwrap_or("");
                let msg = AgentMessage {
                    id: Uuid::new_v4(),
                    from_agent: Uuid::new_v4(),
                    to_agent: Uuid::parse_str(to).unwrap_or(Uuid::new_v4()),
                    content: content.to_string(),
                    timestamp: Utc::now(),
                };
                let mut messages = self.messages.write().await;
                messages.push(msg);
                Ok(serde_json::json!({"sent": true}))
            }
            _ => Ok(serde_json::json!({"error": "Unknown tool"}))
        }
    }

    pub async fn add_task(&self, description: String) -> Result<Uuid> {
        let task = Task {
            id: Uuid::new_v4(),
            description,
            status: "pending".to_string(),
            result: None,
            error: None,
            created_at: Utc::now(),
            completed_at: None,
        };
        
        let task_id = task.id;
        let mut tasks = self.tasks.write().await;
        tasks.insert(task_id, task);
        
        let mut queue = self.task_queue.write().await;
        queue.push(task_id);
        
        Ok(task_id)
    }
}

// ============================================================================
// URL encoding
// ============================================================================
mod urlencoding {
    pub fn encode(s: &str) -> String {
        s.chars().map(|c| match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' | '.' | '~' => c.to_string(),
            ' ' => "%20".to_string(),
            _ => format!("%{:02X}", c as u8),
        }).collect()
    }
}

// ============================================================================
// HTTP API (Agent-focused)
// ============================================================================

#[derive(Serialize)]
struct ApiResponse<T> { success: bool, data: Option<T>, error: Option<String> }

#[derive(Deserialize)]
struct TaskRequest { description: String }

#[derive(Deserialize)]
struct ThinkRequest { 
    prompt: String, 
    max_turns: Option<usize>,
    #[serde(default)]
    private: Option<bool>,
}

#[derive(Deserialize)]
struct SpawnRequest { name: String, system_prompt: Option<String> }

async fn list_agents(State(state): State<Arc<AgentOsState>>) -> Json<ApiResponse<Vec<Agent>>> {
    let agents = state.agents.read().await;
    Json(ApiResponse { success: true, data: Some(agents.values().cloned().collect()), error: None })
}

async fn spawn_agent(State(state): State<Arc<AgentOsState>>, Json(req): Json<SpawnRequest>) -> Json<ApiResponse<Uuid>> {
    let prompt = req.system_prompt.unwrap_or_else(|| "You are an agent.".to_string());
    let agent = Agent {
        id: Uuid::new_v4(),
        name: req.name,
        parent_id: None,
        created_at: Utc::now(),
        system_prompt: prompt.clone(),
        context: vec![Message { role: "system".to_string(), content: prompt, tool_call_id: None, tool_name: None }],
    };
    let id = agent.id;
    state.agents.write().await.insert(id, agent);
    Json(ApiResponse { success: true, data: Some(id), error: None })
}

async fn add_task(State(state): State<Arc<AgentOsState>>, Json(req): Json<TaskRequest>) -> Json<ApiResponse<Uuid>> {
    match state.add_task(req.description).await {
        Ok(id) => Json(ApiResponse { success: true, data: Some(id), error: None }),
        Err(e) => Json(ApiResponse { success: false, data: None, error: Some(e.to_string()) }),
    }
}

async fn get_task(State(state): State<Arc<AgentOsState>>) -> Json<ApiResponse<Uuid>> {
    let mut queue = state.task_queue.write().await;
    match queue.pop() {
        Some(id) => Json(ApiResponse { success: true, data: Some(id), error: None }),
        None => Json(ApiResponse { success: false, data: None, error: Some("No tasks".to_string()) })
    }
}

async fn list_tasks(State(state): State<Arc<AgentOsState>>) -> Json<ApiResponse<Vec<Task>>> {
    let tasks = state.tasks.read().await;
    Json(ApiResponse { success: true, data: Some(tasks.values().cloned().collect()), error: None })
}

async fn think(State(state): State<Arc<AgentOsState>>, Json(req): Json<ThinkRequest>) -> Json<ApiResponse<String>> {
    let agents = state.agents.read().await;
    let init_id = agents.keys().next().cloned();
    drop(agents);
    
    if let Some(agent_id) = init_id {
        match state.think_with_tools(agent_id, &req.prompt, req.max_turns.unwrap_or(10), req.private.unwrap_or(false)).await {
            Ok(r) => Json(ApiResponse { success: true, data: Some(r), error: None }),
            Err(e) => Json(ApiResponse { success: false, data: None, error: Some(e.to_string()) }),
        }
    } else {
        Json(ApiResponse { success: false, data: None, error: Some("No agents".to_string()) })
    }
}

async fn execute_tool(State(state): State<Arc<AgentOsState>>, Json(req): Json<serde_json::Value>) -> Json<ApiResponse<serde_json::Value>> {
    let tool_name = req["tool"].as_str().unwrap_or("");
    let args = req["parameters"].to_string();
    match state.execute_tool(tool_name, &args).await {
        Ok(r) => Json(ApiResponse { success: true, data: Some(r), error: None }),
        Err(e) => Json(ApiResponse { success: false, data: None, error: Some(e.to_string()) }),
    }
}

async fn list_tools(State(state): State<Arc<AgentOsState>>) -> Json<ApiResponse<Vec<Tool>>> {
    let tools = state.tools.read().await;
    Json(ApiResponse { success: true, data: Some(tools.values().cloned().collect()), error: None })
}

async fn get_messages(State(state): State<Arc<AgentOsState>>) -> Json<ApiResponse<Vec<AgentMessage>>> {
    let messages = state.messages.read().await;
    Json(ApiResponse { success: true, data: Some(messages.clone()), error: None })
}

async fn process_all(State(state): State<Arc<AgentOsState>>) -> Json<ApiResponse<String>> {
    let agents = state.agents.read().await;
    let init_id = agents.keys().next().cloned();
    drop(agents);
    
    if let Some(agent_id) = init_id {
        let task_ids: Vec<Uuid> = {
            let tasks = state.tasks.read().await;
            tasks.values().filter(|t| t.status == "pending").map(|t| t.id).collect()
        };
        
        for task_id in task_ids {
            state.tasks.write().await.get_mut(&task_id).map(|t| t.status = "processing".to_string());
            
            let description = {
                let tasks = state.tasks.read().await;
                tasks.get(&task_id).map(|t| t.description.clone())
            };
            
            if let Some(desc) = description {
                match state.think_with_tools(agent_id, &desc, 10, false).await {
                    Ok(result) => {
                        state.tasks.write().await.get_mut(&task_id).map(|t| {
                            t.status = "completed".to_string();
                            t.result = Some(result);
                            t.completed_at = Some(Utc::now());
                        });
                    }
                    Err(e) => {
                        state.tasks.write().await.get_mut(&task_id).map(|t| {
                            t.status = "failed".to_string();
                            t.error = Some(e.to_string());
                        });
                    }
                }
            }
        }
        Json(ApiResponse { success: true, data: Some("Processed".to_string()), error: None })
    } else {
        Json(ApiResponse { success: false, data: None, error: Some("No agents".to_string()) })
    }
}

async fn root() -> Json<ApiResponse<String>> {
    Json(ApiResponse { success: true, data: Some("Agent OS running".to_string()), error: None })
}


// ============================================================================
// MCP (Model Context Protocol) Handlers
// ============================================================================

#[derive(Deserialize)]
struct McpRequest {
    jsonrpc: String,
    id: Option<serde_json::Value>,
    method: String,
    params: Option<serde_json::Value>,
}

#[derive(Serialize)]
struct McpResponse {
    jsonrpc: String,
    id: Option<serde_json::Value>,
    result: Option<serde_json::Value>,
    error: Option<McpError>,
}

#[derive(Serialize)]
struct McpError {
    code: i32,
    message: String,
}

async fn mcp_list_tools(State(state): State<Arc<AgentOsState>>) -> Json<McpResponse> {
    let tools = state.tools.read().await;
    let tool_list: Vec<serde_json::Value> = tools.values().map(|t| {
        serde_json::json!({
            "name": t.name,
            "description": t.description,
            "inputSchema": t.parameters.as_ref().unwrap_or(&serde_json::json!({}))
        })
    }).collect();
    
    Json(McpResponse {
        jsonrpc: "2.0".to_string(),
        id: None,
        result: Some(serde_json::json!({"tools": tool_list})),
        error: None,
    })
}

async fn mcp_execute(State(state): State<Arc<AgentOsState>>, Json(req): Json<McpRequest>) -> Json<McpResponse> {
    let params = req.params.as_ref();
    
    if let Some(params_obj) = params {
        let tool_name = params_obj.get("name").and_then(|n| n.as_str()).unwrap_or("");
        let arguments = params_obj.get("arguments").map(|a| a.to_string()).unwrap_or_default();
        
        match state.execute_tool(tool_name, &arguments).await {
            Ok(result) => {
                return Json(McpResponse {
                    jsonrpc: "2.0".to_string(),
                    id: req.id,
                    result: Some(result),
                    error: None,
                });
            }
            Err(e) => {
                return Json(McpResponse {
                    jsonrpc: "2.0".to_string(),
                    id: req.id,
                    result: None,
                    error: Some(McpError { code: -32603, message: e.to_string() }),
                });
            }
        }
    }
    
    Json(McpResponse {
        jsonrpc: "2.0".to_string(),
        id: req.id,
        result: None,
        error: Some(McpError { code: -32602, message: "Invalid params".to_string() }),
    })
}

async fn mcp_list_agents(State(state): State<Arc<AgentOsState>>, Json(req): Json<McpRequest>) -> Json<McpResponse> {
    let agents = state.agents.read().await;
    let agent_list: Vec<serde_json::Value> = agents.values().map(|a| {
        serde_json::json!({
            "id": a.id,
            "name": a.name,
            "created_at": a.created_at
        })
    }).collect();
    
    Json(McpResponse {
        jsonrpc: "2.0".to_string(),
        id: req.id,
        result: Some(serde_json::json!({"agents": agent_list})),
        error: None,
    })
}

async fn mcp_list_tasks(State(state): State<Arc<AgentOsState>>, Json(req): Json<McpRequest>) -> Json<McpResponse> {
    let tasks = state.tasks.read().await;
    let task_list: Vec<serde_json::Value> = tasks.values().map(|t| {
        serde_json::json!({
            "id": t.id,
            "description": t.description,
            "status": t.status
        })
    }).collect();
    
    Json(McpResponse {
        jsonrpc: "2.0".to_string(),
        id: req.id,
        result: Some(serde_json::json!({"tasks": task_list})),
        error: None,
    })
}

async fn mcp_add_task(State(state): State<Arc<AgentOsState>>, Json(req): Json<McpRequest>) -> Json<McpResponse> {
    let params = req.params.as_ref();
    
    if let Some(params_obj) = params {
        let description = params_obj.get("description").and_then(|d| d.as_str()).unwrap_or("");
        
        match state.add_task(description.to_string()).await {
            Ok(id) => {
                return Json(McpResponse {
                    jsonrpc: "2.0".to_string(),
                    id: req.id,
                    result: Some(serde_json::json!({"id": id})),
                    error: None,
                });
            }
            Err(e) => {
                return Json(McpResponse {
                    jsonrpc: "2.0".to_string(),
                    id: req.id,
                    result: None,
                    error: Some(McpError { code: -32603, message: e.to_string() }),
                });
            }
        }
    }
    
    Json(McpResponse {
        jsonrpc: "2.0".to_string(),
        id: req.id,
        result: None,
        error: Some(McpError { code: -32602, message: "Invalid params".to_string() }),
    })
}



// ============================================================================
// MCP Client - Discover and Add External Tools
// ============================================================================

async fn mcp_discover_tools(State(state): State<Arc<AgentOsState>>) -> Json<ApiResponse<usize>> {
    let mut count = 0;
    
    for server in &state.mcp_client.servers {
        match state.mcp_client.list_tools(&server.url).await {
            Ok(tools) => {
                let mut tool_store = state.tools.write().await;
                for tool in tools {
                    tool_store.insert(tool.name.clone(), tool);
                    count += 1;
                }
                tracing::info!("Discovered {} tools from MCP server {}", count, server.name);
            }
            Err(e) => {
                tracing::warn!("Failed to connect to MCP server {}: {}", server.name, e);
            }
        }
    }
    
    Json(ApiResponse {
        success: true,
        data: Some(count),
        error: None,
    })
}

// Endpoint to add MCP server at runtime
#[derive(Deserialize)]
struct AddMcpServerRequest {
    name: String,
    url: String,
}

async fn mcp_add_server(State(state): State<Arc<AgentOsState>>, Json(req): Json<AddMcpServerRequest>) -> Json<ApiResponse<String>> {
    // Try to connect and get tools
    match state.mcp_client.list_tools(&req.url).await {
        Ok(tools) => {
            let tool_count = tools.len();
            let mut tool_store = state.tools.write().await;
            for tool in tools.into_iter() {
                tool_store.insert(tool.name.clone(), tool);
            }
            tracing::info!("Added MCP server {} with {} tools", req.name, tool_count);
            Json(ApiResponse {
                success: true,
                data: Some(format!("Added {} with {} tools", req.name, tool_count)),
                error: None,
            })
        }
        Err(e) => {
            Json(ApiResponse {
                success: false,
                data: None,
                error: Some(e.to_string()),
            })
        }
    }
}


// ============================================================================
// Main
// ============================================================================

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    // Load config from YAML (or use defaults)
    let config = load_config().unwrap_or_else(|e| {
        tracing::warn!("Failed to load config: {}, using defaults", e);
        Config {
            server: ServerConfig { host: "0.0.0.0".to_string(), port: 8080 },
            ollama: OllamaConfig { url: "http://192.168.0.247:11434".to_string(), model: "qwen3.5:35b-a3b".to_string(), private_url: None, private_model: None, default_private: false },
            storage: StorageConfig { path: "/var/agent-os/storage".to_string() },
            system: SystemConfig { system_prompt: "You are an autonomous AI agent.".to_string() },
            tools: vec![],
            permissions: PermissionsConfig { allow_spawn: true, allow_network: true, allow_filesystem: true, allow_execute: true },
            mcp_servers: vec![],
        }
    });
    
    tracing::info!("Ollama: {} ({})", config.ollama.url, config.ollama.model);
    tracing::info!("Tools: {}", config.tools.len());

    tokio::fs::create_dir_all(&config.storage.path).await?;

    let state = Arc::new(AgentOsState::new(&config));
    state.init_tools(&config).await;
    state.load_state().await.unwrap_or_else(|e| tracing::warn!("Load state error: {}", e));
    // Start autonomous task processing loop
    let state_clone = state.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
            
            // Get next task
            let task_id = {
                let mut queue = state_clone.task_queue.write().await;
                queue.pop()
            };
            
            if let Some(tid) = task_id {
                tracing::info!("Auto-processing task {}", tid);
                
                // Update status
                {
                    let mut tasks = state_clone.tasks.write().await;
                    if let Some(task) = tasks.get_mut(&tid) {
                        task.status = "processing".to_string();
                    }
                }
                
                // Get description
                let description = {
                    let tasks = state_clone.tasks.read().await;
                    tasks.get(&tid).map(|t| t.description.clone())
                };
                
                if let Some(desc) = description {
                    // Get init agent
                    let agent_id = {
                        let agents = state_clone.agents.read().await;
                        agents.keys().next().cloned()
                    };
                    
                    if let Some(aid) = agent_id {
                        let result = state_clone.think_with_tools(aid, &desc, 10, false).await;
                        {
                            let mut tasks = state_clone.tasks.write().await;
                            if let Some(task) = tasks.get_mut(&tid) {
                                match result {
                                    Ok(r) => {
                                        task.status = "completed".to_string();
                                        task.result = Some(r);
                                        task.completed_at = Some(chrono::Utc::now());
                                    }
                                    Err(_e) => {
                                        task.status = "failed".to_string();
                                    }
                                }
                            }
                        }
                        // Save state after task completion
                        let _ = state_clone.save_state().await;
                    }
                }
            }
        }
    });

    let addr = format!("{}:{}", config.server.host, config.server.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("Agent OS on http://{}", addr);

    let app = Router::new()
        .route("/", get(root))
        .route("/agents", get(list_agents))
        .route("/agents", post(spawn_agent))
        .route("/tasks", post(add_task))
        .route("/tasks", get(list_tasks))
        .route("/tasks/next", get(get_task))
        .route("/think", post(think))
        .route("/execute", post(execute_tool))
        .route("/tools", get(list_tools))
        .route("/messages", get(get_messages))
        .route("/process", post(process_all))
        
        // MCP (Model Context Protocol) endpoints
        .route("/mcp/tools", get(mcp_list_tools))
        .route("/mcp/execute", post(mcp_execute))
        .route("/mcp/agents", get(mcp_list_agents))
        .route("/mcp/tasks", get(mcp_list_tasks))
        .route("/mcp/tasks", post(mcp_add_task))
        .route("/mcp/discover", post(mcp_discover_tools))
        .route("/mcp/servers", post(mcp_add_server))
        
        .with_state(state);

    axum::serve(listener, app).await?;
    Ok(())
}
