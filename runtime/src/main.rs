//! Agent OS - Autonomous Agent Runtime
//!
//! The core loop: receive tasks → reason with tools → execute → report

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
use std::sync::atomic::{AtomicU64, Ordering};
use std::path::PathBuf;
use uuid::Uuid;
use chrono::{DateTime, Utc};

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
    pub permissions: Permissions,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Permissions {
    pub can_spawn_agents: bool,
    pub can_access_network: bool,
    pub can_access_filesystem: bool,
    pub can_execute_commands: bool,
}

impl Default for Permissions {
    fn default() -> Self {
        Self {
            can_spawn_agents: true,
            can_access_network: true,
            can_access_filesystem: true,
            can_execute_commands: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,      // system, user, assistant, tool
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
    pub status: String,  // pending, processing, completed, failed
    pub result: Option<String>,
    pub error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
}

// ============================================================================
// Agent OS State
// ============================================================================

pub struct AgentOsState {
    pub agents: Arc<TokioRwLock<HashMap<Uuid, Agent>>>,
    pub tasks: Arc<TokioRwLock<HashMap<Uuid, Task>>>,
    pub task_queue: Arc<TokioRwLock<Vec<Uuid>>>,
    pub tools: Arc<TokioRwLock<HashMap<String, Tool>>>,
    pub ollama_url: String,
    pub model: String,
    pub storage_path: PathBuf,
    pub running: Arc<AtomicU64>,
}

impl AgentOsState {
    pub fn new(ollama_url: &str, model: &str, storage_path: PathBuf) -> Self {
        let system_prompt = r#"You are an autonomous AI agent running on Agent OS.

Your job is to:
1. Receive tasks from the queue
2. Reason about what needs to be done
3. Use tools to accomplish tasks
4. Report results

Available tools:
- Use tools whenever you need to get information or perform actions
- After using a tool, you will get the result
- Based on results, decide the next step
- When task is complete, say "TASK_COMPLETE: <result>"

Be precise. Be efficient. Use tools proactively."#;

        let init_agent = Agent {
            id: Uuid::new_v4(),
            name: "init".to_string(),
            parent_id: None,
            created_at: Utc::now(),
            system_prompt: system_prompt.to_string(),
            context: vec![Message {
                role: "system".to_string(),
                content: system_prompt.to_string(),
                tool_call_id: None,
                tool_name: None,
            }],
            permissions: Permissions::default(),
        };
        
        let agents = Arc::new(TokioRwLock::new(HashMap::new()));
        let tasks = Arc::new(TokioRwLock::new(HashMap::new()));
        let task_queue = Arc::new(TokioRwLock::new(Vec::new()));
        let tools = Arc::new(TokioRwLock::new(HashMap::new()));

        // Spawn init agent
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
            ollama_url: ollama_url.to_string(),
            model: model.to_string(),
            storage_path,
            running: Arc::new(AtomicU64::new(0)),
        }
    }

    pub async fn init_tools(&self) {
        let mut tools = self.tools.write().await;
        
        tools.insert("get_time".to_string(), Tool {
            name: "get_time".to_string(),
            description: "Get current date and time".to_string(),
            parameters: serde_json::json!({}),
        });

        tools.insert("list_directory".to_string(), Tool {
            name: "list_directory".to_string(),
            description: "List files in a directory".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "default": "."}
                }
            }),
        });

        tools.insert("read_file".to_string(), Tool {
            name: "read_file".to_string(),
            description: "Read a file".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string"}
                },
                "required": ["path"]
            }),
        });

        tools.insert("http_get".to_string(), Tool {
            name: "http_get".to_string(),
            description: "Make HTTP GET request".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "url": {"type": "string"}
                },
                "required": ["url"]
            }),
        });

        tools.insert("search_web".to_string(), Tool {
            name: "search_web".to_string(),
            description: "Search the web".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string"}
                },
                "required": ["query"]
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
    }

    pub async fn think_with_tools(&self, agent_id: Uuid, task: &str, max_turns: usize) -> Result<String> {
        let mut agents = self.agents.write().await;
        let agent = agents.get_mut(&agent_id)
            .ok_or_else(|| anyhow::anyhow!("Agent not found"))?;
        
        // Add task as user message
        agent.context.push(Message {
            role: "user".to_string(),
            content: task.to_string(),
            tool_call_id: None,
            tool_name: None,
        });

        for _turn in 0..max_turns {
            // Build messages for Ollama
            let messages: Vec<serde_json::Value> = agent.context.iter().map(|m| {
                let mut msg = serde_json::json!({
                    "role": m.role,
                    "content": m.content
                });
                if let Some(tool_name) = &m.tool_name {
                    msg["tool_name"] = serde_json::json!(tool_name);
                }
                msg
            }).collect();

            // Get available tools
            let tools = self.tools.read().await;
            let tools_json: Vec<serde_json::Value> = tools.values().map(|t| {
                serde_json::json!({
                    "type": "function",
                    "function": {
                        "name": t.name,
                        "description": t.description,
                        "parameters": t.parameters
                    }
                })
            }).collect();
            drop(tools);

            // Call Ollama
            let client = reqwest::Client::new();
            let request = serde_json::json!({
                "model": self.model,
                "messages": messages,
                "tools": tools_json,
                "stream": false
            });
            
            let response = client.post(format!("{}/api/chat", self.ollama_url))
                .json(&request)
                .send()
                .await?;
            
            let result: serde_json::Value = response.json().await?;
            
            let content = result["message"]["content"].as_str().unwrap_or("");
            let tool_calls_opt = result["message"]["tool_calls"].as_array();
            
            if let Some(calls) = tool_calls_opt {
                if !calls.is_empty() {
                    // Tool call(s) detected
                    for call in calls {
                        let tool_name = call["function"]["name"].as_str().unwrap_or("");
                        let args_str = call["function"]["arguments"].to_string();
                        
                        // Add assistant message with tool call
                        agent.context.push(Message {
                            role: "assistant".to_string(),
                            content: format!("Using tool: {}", tool_name),
                            tool_call_id: None,
                            tool_name: Some(tool_name.to_string()),
                        });
                        
                        // Execute tool
                        let tool_result = self.execute_tool(tool_name, &args_str).await;
                        
                        // Add tool result as message
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
                    
                    // Continue loop to let model see tool results
                    continue;
                }
            }
            
            // No tool calls - this is the final response
            let final_response = content.to_string();
            
            // Check for TASK_COMPLETE
            if final_response.contains("TASK_COMPLETE:") {
                agent.context.push(Message {
                    role: "assistant".to_string(),
                    content: final_response.clone(),
                    tool_call_id: None,
                    tool_name: None,
                });
                return Ok(final_response);
            }
            
            // Check if model is done (no more tool calls in response)
            if content.trim().is_empty() || !content.contains("tool") {
                agent.context.push(Message {
                    role: "assistant".to_string(),
                    content: final_response.clone(),
                    tool_call_id: None,
                    tool_name: None,
                });
                return Ok(final_response);
            }
            
            // Add the response and continue
            agent.context.push(Message {
                role: "assistant".to_string(),
                content: final_response.clone(),
                tool_call_id: None,
                tool_name: None,
            });
        }
        
        Ok("Max turns reached".to_string())
    }

    pub async fn execute_tool(&self, tool_name: &str, args: &str) -> Result<serde_json::Value> {
        let params: serde_json::Value = serde_json::from_str(args)
            .unwrap_or(serde_json::json!({}));
        
        match tool_name {
            "get_time" => {
                Ok(serde_json::json!({"time": Utc::now().to_rfc3339()}))
            }
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
                let text = resp.text().await?;
                Ok(serde_json::json!({"body": text.chars().take(1000).collect::<String>()}))
            }
            "search_web" => {
                let query = params["query"].as_str().unwrap_or("");
                let client = reqwest::Client::new();
                let url = format!("http://192.168.0.247:18080/search?q={}", 
                    urlencoding::encode(query));
                let resp = client.get(&url).send().await?;
                let text = resp.text().await?;
                Ok(serde_json::json!({"results": text.chars().take(1500).collect::<String>()}))
            }
            "execute_command" => {
                let cmd = params["command"].as_str().unwrap_or("");
                let output = tokio::process::Command::new("sh")
                    .arg("-c")
                    .arg(cmd)
                    .output()
                    .await?;
                Ok(serde_json::json!({
                    "stdout": String::from_utf8_lossy(&output.stdout),
                    "stderr": String::from_utf8_lossy(&output.stderr),
                    "code": output.status.code()
                }))
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

    pub async fn get_next_task(&self) -> Option<Uuid> {
        let mut queue = self.task_queue.write().await;
        queue.pop()
    }

    pub async fn update_task_status(&self, task_id: Uuid, status: &str, result: Option<String>, error: Option<String>) {
        let mut tasks = self.tasks.write().await;
        if let Some(task) = tasks.get_mut(&task_id) {
            task.status = status.to_string();
            task.result = result;
            task.error = error;
            if status == "completed" || status == "failed" {
                task.completed_at = Some(Utc::now());
            }
        }
    }

    pub async fn start_autonomous_loop(&self, agent_id: Uuid) {
        self.running.fetch_add(1, Ordering::Relaxed);
        
        let running = self.running.clone();
        let state = Arc::new(()) as Arc<()>; // Placeholder for state clone
        
        // Clone all needed state
        let tasks = self.tasks.clone();
        let task_queue = self.task_queue.clone();
        let ollama_url = self.ollama_url.clone();
        let model = self.model.clone();
        let tools = self.tools.clone();
        
        // Need to clone agents too
        let agents = self.agents.clone();
        
        tracing::info!("Autonomous loop started");
        
        // Main autonomous loop
        while running.load(Ordering::Relaxed) > 0 {
            // Get next task from queue
            let task_id = {
                let mut queue = task_queue.write().await;
                queue.pop()
            };
            
            if let Some(id) = task_id {
                tracing::info!("Processing task: {}", id);
                
                // Update status to processing
                {
                    let mut t = tasks.write().await;
                    if let Some(task) = t.get_mut(&id) {
                        task.status = "processing".to_string();
                    }
                }
                
                // Get the task description
                let description = {
                    let t = tasks.read().await;
                    t.get(&id).map(|task| task.description.clone())
                };
                
                if let Some(desc) = description {
                    // Get agent and process
                    let agent_exists = {
                        let agents_read = agents.read().await;
                        agents_read.contains_key(&agent_id)
                    };
                    
                    if agent_exists {
                        // Call think_with_tools - this needs self reference
                        // We'll do the thinking inline here
                        let result = self.think_with_tools(agent_id, &desc, 10).await;
                        
                        // Update task with result
                        let mut t = tasks.write().await;
                        if let Some(task) = t.get_mut(&id) {
                            match result {
                                Ok(r) => {
                                    task.status = "completed".to_string();
                                    task.result = Some(r);
                                }
                                Err(e) => {
                                    task.status = "failed".to_string();
                                    task.error = Some(e.to_string());
                                }
                            }
                            task.completed_at = Some(Utc::now());
                        }
                    }
                }
            } else {
                // No tasks - sleep briefly to avoid busy loop
                tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
            }
        }
        
        tracing::info!("Autonomous loop stopped");
    }

    pub async fn stop_autonomous_loop(&self) {
        self.running.fetch_sub(1, Ordering::Relaxed);
    }
}

// ============================================================================
// URL encoding
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
// HTTP Handlers
// ============================================================================

#[derive(Serialize)]
struct ApiResponse<T> {
    success: bool,
    data: Option<T>,
    error: Option<String>,
}

#[derive(Deserialize)]
struct TaskRequest {
    description: String,
}

#[derive(Deserialize)]
struct ThinkRequest {
    prompt: String,
    max_turns: Option<usize>,
}

async fn add_task(State(state): State<Arc<AgentOsState>>, Json(req): Json<TaskRequest>) -> Json<ApiResponse<Uuid>> {
    match state.add_task(req.description).await {
        Ok(id) => Json(ApiResponse { success: true, data: Some(id), error: None }),
        Err(e) => Json(ApiResponse { success: false, data: None, error: Some(e.to_string()) }),
    }
}

async fn get_next_task(State(state): State<Arc<AgentOsState>>) -> Json<ApiResponse<Option<Uuid>>> {
    let task_id = state.get_next_task().await;
    Json(ApiResponse { success: true, data: Some(task_id), error: None })
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
    let list: Vec<Tool> = tools.values().cloned().collect();
    Json(ApiResponse { success: true, data: Some(list), error: None })
}

async fn list_tasks(State(state): State<Arc<AgentOsState>>) -> Json<ApiResponse<Vec<Task>>> {
    let tasks = state.tasks.read().await;
    let list: Vec<Task> = tasks.values().cloned().collect();
    Json(ApiResponse { success: true, data: Some(list), error: None })
}

async fn get_task(State(state): State<Arc<AgentOsState>>, Json(id): Json<Uuid>) -> Json<ApiResponse<Task>> {
    let tasks = state.tasks.read().await;
    match tasks.get(&id) {
        Some(t) => Json(ApiResponse { success: true, data: Some(t.clone()), error: None }),
        None => Json(ApiResponse { success: false, data: None, error: Some("Not found".to_string()) })
    }
}

async fn start_loop(State(state): State<Arc<AgentOsState>>) -> Json<ApiResponse<String>> {
    let agents = state.agents.read().await;
    let init_id = agents.keys().next().cloned();
    drop(agents);
    
    if let Some(agent_id) = init_id {
        state.start_autonomous_loop(agent_id).await;
        Json(ApiResponse { success: true, data: Some("Loop started".to_string()), error: None })
    } else {
        Json(ApiResponse { success: false, data: None, error: Some("No agents".to_string()) })
    }
}

async fn stop_loop(State(state): State<Arc<AgentOsState>>) -> Json<ApiResponse<String>> {
    state.stop_autonomous_loop().await;
    Json(ApiResponse { success: true, data: Some("Loop stopped".to_string()), error: None })
}

// ============================================================================
// Main
// ============================================================================

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let ollama_url = std::env::var("OLLAMA_URL").unwrap_or_else(|_| "http://192.168.0.247:11434".to_string());
    let model = std::env::var("MODEL").unwrap_or_else(|_| "qwen3:8b".to_string());
    let storage_path = std::env::var("STORAGE_PATH").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("/var/agent-os/storage"));
    
    tokio::fs::create_dir_all(&storage_path).await?;

    let state = Arc::new(AgentOsState::new(&ollama_url, &model, storage_path));
    state.init_tools().await;

    let state_clone = state.clone();
    tokio::spawn(async move {
        state_clone.start_autonomous_loop(state_clone.agents.read().await.keys().next().unwrap().clone()).await;
    });

    let app = Router::new()
        .route("/tasks", post(add_task))
        .route("/tasks/next", get(get_next_task))
        .route("/tasks", get(list_tasks))
        .route("/task", post(get_task))
        .route("/think", post(think))
        .route("/execute", post(execute_tool))
        .route("/tools", get(list_tools))
        .route("/loop/start", post(start_loop))
        .route("/loop/stop", post(stop_loop))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await?;
    tracing::info!("Agent OS listening on http://0.0.0.0:8080");
    tracing::info!("Ollama: {} ({})", ollama_url, model);

    axum::serve(listener, app).await?;
    Ok(())
}
