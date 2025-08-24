use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheKey {
    pub task_name: String,
    pub inputs_hash: String,
    pub workspace_hash: String,
    pub command_hash: String,
    pub environment_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheEntry {
    pub id: Uuid,
    pub key: CacheKey,
    pub outputs: HashMap<String, String>, // path -> blob_hash
    pub metadata: CacheMetadata,
    pub created_at: DateTime<Utc>,
    pub accessed_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheMetadata {
    pub duration_ms: i64,
    pub exit_code: i32,
    pub stdout_hash: Option<String>,
    pub stderr_hash: Option<String>,
    pub dependencies: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskDefinition {
    pub name: String,
    pub command: String,
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
    pub dependencies: Vec<String>,
    pub cache: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceConfig {
    pub name: String,
    pub tasks: HashMap<String, TaskDefinition>,
    pub cache_dir: Option<String>,
    pub cache_size_limit: Option<String>,
    pub cache_max_age: Option<String>,
}

// New configuration types for cue.workspace.toml and cue.toml
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceRootConfig {
    pub workspace: WorkspaceRoot,
    pub cache: Option<CacheConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceRoot {
    pub name: String,
    pub projects: Option<ProjectsConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectsConfig {
    /// Whether to auto-discover cue.toml files. Defaults to true.
    #[serde(default = "default_auto_discover")]
    pub auto_discover: bool,
    
    /// List of glob patterns to include (only used when auto_discover is false)
    pub include: Option<Vec<String>>,
    
    /// List of glob patterns to exclude from discovery and include
    pub exclude: Option<Vec<String>>,
}

fn default_auto_discover() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheConfig {
    pub remote: Option<String>,
    pub size_limit: Option<String>,
    pub eviction_policy: Option<EvictionPolicy>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EvictionPolicy {
    Lru,
    Fifo,
    Random,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectConfig {
    pub project: Project,
    pub tasks: HashMap<String, ProjectTask>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub name: String,
    pub description: Option<String>,
    pub version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectTask {
    pub command: String,
    pub inputs: Option<Vec<String>>,
    pub outputs: Option<Vec<String>>,
    pub dependencies: Option<Vec<String>>,
    pub cache: Option<bool>,
    pub description: Option<String>,
}

// CAS-related types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlobInfo {
    pub hash: String,
    pub size: u64,
    pub ref_count: u32,
    pub last_access: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionInfo {
    pub id: Uuid,
    pub cache_key: String,
    pub exit_code: i32,
    pub stdout_hash: Option<String>,
    pub stderr_hash: Option<String>,
    pub duration_ms: i64,
    pub created_at: DateTime<Utc>,
    pub last_access: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputMapping {
    pub action_id: Uuid,
    pub blob_hash: String,
    pub relative_path: String,
}
