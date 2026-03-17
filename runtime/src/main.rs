//! Agent OS - Operating System for Autonomous AI Agents
//!
//! Built for agents to consume programmatically.
//! Configurable via YAML: cargo run -- --config agent-os.yml

use anyhow::Result;
use axum::{
    routing::{get, post},
    Router, Json,
    extract::State,
};
use clap::Parser;
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

#[derive(Debug, Clone, Deserialize)]
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
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
}

fn default_host() -> String { "0.0.0.0".to_string() }
fn default_port() -> u16 { 8080 }

#[derive(Debug, Clone, Deserialize)]
pub struct OllamaConfig {
    #[serde(default = "default_ollama_url")]
    pub url: String,
    #[serde(default = "default_model")]
    pub model: String,
}

fn default_ollama_url() -> String { "http://192.168.0.247:11434".to_string() }
fn default_model() -> String { "qwen3.5:35b-a3b".to_string() }

#[derive(Debug, Clone, Deserialize)]
pub struct StorageConfig {
    #[serde(default = "default_storage_path")]
    pub path: String,
}

fn default_storage_path() -> String { "/var/agent-os/storage".to_string() }

#[derive(Debug, Clone, Deserialize)]
pub struct SystemConfig {
    #[serde(default = "default_system_prompt")]
    pub system_prompt: String,
}

fn default_system_prompt() -> String { 
    "You are an autonomous AI agent. Complete tasks using tools.\nWhen done, output: TASK_COMPLETE: <result>".to_string() 
}

#[derive(Debug, Clone, Deserialize)]
pub struct ToolConfig {
    pub name: String,
    pub description: String,
    #[serde(default = "default_handler")]
    pub handler: String,
    pub parameters: serde_json::Value,
}

fn default_handler() -> String { "builtin".to_string() }

#[derive(Debug, Clone, Deserialize)]
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

#[derive(Parser, Debug)]
#[command(name = "agent-os")]
pub struct Args {
    #[arg(short, long, default_value = "agent-os.yml")]
    pub config: String,
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
    pub parameters: serde_json::Value,
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
    pub storage_path: PathBuf,
    pub running: Arc<std::sync::atomic::AtomicU64>,
}

impl AgentOsState {
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
            storage_path: PathBuf::from(&config.storage.path),
            running: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        }
    }

    pub async fn init_tools(&self, config: &Config) {
        let mut tools = self.tools.write().await;
        
        for tool_config in &config.tools {
            tools.insert(tool_config.name.clone(), Tool {
                name: tool_config.name.clone(),
                description: tool_config.description.clone(),
                parameters: tool_config.parameters.clone(),
            });
            tracing::info!("Loaded tool: {}", tool_config.name);
        }
    }

    pub async fn think_with_tools(&self, agent_id: Uuid, task: &str, max_turns: usize) -> Result<String> {
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
                "model": self.model,
                "messages": messages,
                "tools": tools_json,
                "stream": false
            });
            
            let response = client.post(format!("{}/api/chat", self.ollama_url))
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

    pub async fn execute_tool(&self, tool_name: &str, args: &str) -> Result<serde_json::Value> {
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
struct ThinkRequest { prompt: String, max_turns: Option<usize> }

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
        match state.think_with_tools(agent_id, &req.prompt, req.max_turns.unwrap_or(10)).await {
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
                match state.think_with_tools(agent_id, &desc, 10).await {
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
// Main
// ============================================================================

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let args = Args::parse();
    
    // Load config
    let config_content = std::fs::read_to_string(&args.config)?;
    let config: Config = serde_yaml::from_str(&config_content)?;
    
    tracing::info!("Loaded config from {}", args.config);
    tracing::info!("Ollama: {} ({})", config.ollama.url, config.ollama.model);
    tracing::info!("Tools: {}", config.tools.len());

    tokio::fs::create_dir_all(&config.storage.path).await?;

    let state = Arc::new(AgentOsState::new(&config));
    state.init_tools(&config).await;

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
        .with_state(state);

    axum::serve(listener, app).await?;
    Ok(())
}
