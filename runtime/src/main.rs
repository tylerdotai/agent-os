//! Agent OS Runtime
//! 
//! Userspace runtime for the Agent OS. Provides:
//! - Context management (token budgeting, eviction)
//! - Tool registry
//! - Message bus for agent-to-agent communication
//! - Persistence layer

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use uuid::Uuid;
use chrono::{DateTime, Utc};

// ============================================================================
// Core Types
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentIdentity {
    pub id: Uuid,
    pub name: String,
    pub parent_id: Option<Uuid>,
    pub permissions: Permissions,
    pub quotas: Quotas,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Permissions {
    pub can_spend_money: bool,
    pub can_post_publicly: bool,
    pub can_spawn_agents: bool,
    pub can_access_network: bool,
    pub can_access_filesystem: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Quotas {
    pub max_context_tokens: usize,
    pub max_compute_per_hour: usize,
    pub max_api_calls_per_day: usize,
}

impl Default for Quotas {
    fn default() -> Self {
        Self {
            max_context_tokens: 128_000,
            max_compute_per_hour: 3600,
            max_api_calls_per_day: 1000,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextPage {
    pub id: Uuid,
    pub content: String,
    pub importance: f32,
    pub last_accessed: DateTime<Utc>,
    pub in_memory: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Agent {
    pub identity: AgentIdentity,
    pub context_pages: Vec<ContextPage>,
    pub mounted_tools: Vec<String>,
}

impl Agent {
    pub fn new(name: String, parent_id: Option<Uuid>) -> Self {
        Self {
            identity: AgentIdentity {
                id: Uuid::new_v4(),
                name,
                parent_id,
                permissions: Permissions::default(),
                quotas: Quotas::default(),
                created_at: Utc::now(),
            },
            context_pages: Vec::new(),
            mounted_tools: Vec::new(),
        }
    }
}

// ============================================================================
// Context Manager
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EvictionPolicy {
    LRU,
    Importance,
    Semantic,
}

pub struct ContextManager {
    agents: Arc<RwLock<HashMap<Uuid, Agent>>>,
    eviction_policy: EvictionPolicy,
}

impl ContextManager {
    pub fn new() -> Self {
        Self {
            agents: Arc::new(RwLock::new(HashMap::new())),
            eviction_policy: EvictionPolicy::LRU,
        }
    }

    pub fn spawn_agent(&self, name: String, parent_id: Option<Uuid>) -> Result<Uuid> {
        let agent = Agent::new(name, parent_id);
        let id = agent.identity.id;
        
        let mut agents = self.agents.write().unwrap();
        agents.insert(id, agent);
        
        Ok(id)
    }

    pub fn add_context(&self, agent_id: Uuid, content: String, importance: f32) -> Result<()> {
        let mut agents = self.agents.write().unwrap();
        let agent = agents.get_mut(&agent_id)
            .ok_or_else(|| anyhow::anyhow!("Agent not found"))?;

        let page = ContextPage {
            id: Uuid::new_v4(),
            content,
            importance,
            last_accessed: Utc::now(),
            in_memory: true,
        };

        // Check budget and evict if needed
        let total_tokens = agent.context_pages.iter()
            .map(|p| p.content.len())
            .sum::<usize>() + page.content.len();
        
        if total_tokens > agent.identity.quotas.max_context_tokens {
            // Simple eviction: remove lowest importance
            agent.context_pages.sort_by(|a, b| a.importance.partial_cmp(&b.importance).unwrap());
            if let Some(evicted) = agent.context_pages.first() {
                agent.context_pages.remove(0);
                tracing::info!("Evicted context page {}", evicted.id);
            }
        }

        agent.context_pages.push(page);
        Ok(())
    }

    pub fn get_context(&self, agent_id: Uuid) -> Result<String> {
        let agents = self.agents.read().unwrap();
        let agent = agents.get(&agent_id)
            .ok_or_else(|| anyhow::anyhow!("Agent not found"))?;

        Ok(agent.context_pages
            .iter()
            .map(|p| p.content.as_str())
            .collect::<Vec<_>>()
            .join("\n"))
    }
}

// ============================================================================
// Tool Registry
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tool {
    pub name: String,
    pub description: String,
    pub schema: serde_json::Value,
    pub permissions: Vec<String>,
}

pub struct ToolRegistry {
    tools: Arc<RwLock<HashMap<String, Tool>>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn register(&self, tool: Tool) -> Result<()> {
        let mut tools = self.tools.write().unwrap();
        tools.insert(tool.name.clone(), tool);
        Ok(())
    }

    pub fn list(&self) -> Vec<String> {
        let tools = self.tools.read().unwrap();
        tools.keys().cloned().collect()
    }
}

// ============================================================================
// Message Bus
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MessageType {
    Request,
    Response,
    Event,
    Stream,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentMessage {
    pub id: Uuid,
    pub sender: Uuid,
    pub recipient: Option<Uuid>,  // None = broadcast
    pub msg_type: MessageType,
    pub action: String,
    pub payload: serde_json::Value,
    pub correlation_id: Option<Uuid>,
}

pub struct MessageBus {
    inbox: Arc<RwLock<HashMap<Uuid, Vec<AgentMessage>>>>,
    subscribers: Arc<RwLock<HashMap<Uuid, Vec<Uuid>>>>,
}

impl MessageBus {
    pub fn new() -> Self {
        Self {
            inbox: Arc::new(RwLock::new(HashMap::new())),
            subscribers: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn send(&self, message: AgentMessage) -> Result<()> {
        let mut inbox = self.inbox.write().unwrap();
        
        if let Some(recipient) = message.recipient {
            inbox.entry(recipient).or_insert_with(Vec::new).push(message);
        } else {
            // Broadcast - deliver to all agents
            for (_id, messages) in inbox.iter_mut() {
                messages.push(message.clone());
            }
        }
        
        Ok(())
    }

    pub fn receive(&self, agent_id: Uuid) -> Vec<AgentMessage> {
        let mut inbox = self.inbox.write().unwrap();
        inbox.remove(&agent_id).unwrap_or_default()
    }

    pub fn subscribe(&self, agent_id: Uuid, channel: Uuid) -> Result<()> {
        let mut subs = self.subscribers.write().unwrap();
        subs.entry(channel).or_insert_with(Vec::new).push(agent_id);
        Ok(())
    }
}

// ============================================================================
// Persistence
// ============================================================================

pub struct Persistence {
    storage_path: std::path::PathBuf,
}

impl Persistence {
    pub fn new(storage_path: std::path::PathBuf) -> Self {
        Self { storage_path }
    }

    pub fn checkpoint(&self, agent: &Agent) -> Result<std::path::PathBuf> {
        let path = self.storage_path.join(format!("{}.json", agent.identity.id));
        let json = serde_json::to_string_pretty(agent)?;
        std::fs::write(&path, json)?;
        Ok(path)
    }

    pub fn restore(&self, agent_id: Uuid) -> Result<Agent> {
        let path = self.storage_path.join(format!("{}.json", agent_id));
        let json = std::fs::read_to_string(path)?;
        let agent: Agent = serde_json::from_str(&json)?;
        Ok(agent)
    }
}

// ============================================================================
// Main Runtime
// ============================================================================

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    tracing::info!("Agent OS starting...");

    let context_manager = ContextManager::new();
    let tool_registry = ToolRegistry::new();
    let message_bus = MessageBus::new();
    let persistence = Persistence::new(std::path::PathBuf::from("/var/agent-os/storage"));

    // Spawn init agent
    let init_id = context_manager.spawn_agent("init".to_string(), None)?;
    tracing::info!("Spawned init agent: {}", init_id);

    // Register some basic tools
    tool_registry.register(Tool {
        name: "filesystem_read".to_string(),
        description: "Read a file from the filesystem".to_string(),
        schema: serde_json::json!({
            "type": "object",
            "properties": {
                "path": {"type": "string"}
            },
            "required": ["path"]
        }),
        permissions: vec!["filesystem:read".to_string()],
    })?;

    tool_registry.register(Tool {
        name: "http_get".to_string(),
        description: "Make an HTTP GET request".to_string(),
        schema: serde_json::json!({
            "type": "object",
            "properties": {
                "url": {"type": "string"}
            },
            "required": ["url"]
        }),
        permissions: vec!["network".to_string()],
    })?;

    tracing::info!("Registered tools: {:?}", tool_registry.list());
    tracing::info!("Agent OS ready");

    // Keep runtime alive
    tokio::signal::ctrl_c().await?;
    tracing::info!("Agent OS shutting down");

    Ok(())
}
