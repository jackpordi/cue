// Configuration management for the CLI
// This module will handle loading and parsing workspace configurations

use cue_common::{
    Result, WorkspaceRootConfig, ProjectConfig, WorkspaceConfig, TaskDefinition,
    WorkspaceRoot, CacheConfig, Project, ProjectTask
};
use std::path::{Path, PathBuf};
use tracing::{info, debug, warn};
use glob::glob;

pub struct ConfigManager {
    workspace_root: PathBuf,
    workspace_config: Option<WorkspaceRootConfig>,
}

impl ConfigManager {
    /// Find the workspace root by looking for cue.workspace.toml or git root
    pub async fn find_workspace_root(start_dir: PathBuf) -> Result<PathBuf> {
        let mut current_dir = start_dir;
        
        loop {
            debug!("Checking directory for workspace: {}", current_dir.display());
            
            // Check if cue.workspace.toml exists in current directory
            let workspace_file = current_dir.join("cue.workspace.toml");
            if workspace_file.exists() {
                debug!("Found workspace config at: {}", current_dir.display());
                return Ok(current_dir);
            }
            
            // Check if we're at the git root (has .git directory)
            let git_dir = current_dir.join(".git");
            if git_dir.exists() && git_dir.is_dir() {
                debug!("Reached git root without finding workspace config: {}", current_dir.display());
                return Err(cue_common::CueError::Config(
                    "cue.workspace.toml not found. Please create a workspace configuration file in the git repository root.".to_string()
                ));
            }
            
            // Move to parent directory
            if let Some(parent) = current_dir.parent() {
                current_dir = parent.to_path_buf();
            } else {
                debug!("Reached filesystem root without finding workspace config");
                return Err(cue_common::CueError::Config(
                    "cue.workspace.toml not found. Please create a workspace configuration file.".to_string()
                ));
            }
        }
    }
    
    pub fn new(workspace_root: PathBuf) -> Self {
        Self {
            workspace_root,
            workspace_config: None,
        }
    }
    
    /// Load the workspace configuration from cue.workspace.toml
    pub async fn load_workspace_config(&mut self) -> Result<&WorkspaceRootConfig> {
        if let Some(ref config) = self.workspace_config {
            return Ok(config);
        }
        
        let workspace_file = self.workspace_root.join("cue.workspace.toml");
        debug!("Loading workspace config from: {}", workspace_file.display());
        
        if !workspace_file.exists() {
            return Err(cue_common::CueError::Config(
                "cue.workspace.toml not found. Please create a workspace configuration file.".to_string()
            ));
        }
        
        let content = tokio::fs::read_to_string(&workspace_file).await?;
        let config: WorkspaceRootConfig = toml::from_str(&content)?;
        
        debug!("Loaded workspace config: {:?}", config);
        self.workspace_config = Some(config);
        
        Ok(self.workspace_config.as_ref().unwrap())
    }
    
    /// Load all project configurations
    pub async fn load_all_projects(&self) -> Result<Vec<(PathBuf, ProjectConfig)>> {
        let workspace_config = self.workspace_config.as_ref()
            .ok_or_else(|| cue_common::CueError::Config("Workspace config not loaded".to_string()))?;
        
        let mut projects = Vec::new();
        
        // Get projects configuration, defaulting to auto-discover if not specified
        let projects_config = workspace_config.workspace.projects.as_ref()
            .unwrap_or(&cue_common::ProjectsConfig {
                auto_discover: true,
                include: None,
                exclude: None,
            });
        
        if projects_config.auto_discover {
            // Auto-discover cue.toml files
            projects = self.discover_projects(&projects_config.exclude).await?;
        } else {
            // Use include patterns
            if let Some(include_patterns) = &projects_config.include {
                projects = self.load_projects_from_patterns(include_patterns, &projects_config.exclude).await?;
            }
        }
        
        debug!("Loaded {} project configurations", projects.len());
        Ok(projects)
    }
    
    /// Auto-discover cue.toml files in the workspace
    async fn discover_projects(&self, exclude_patterns: &Option<Vec<String>>) -> Result<Vec<(PathBuf, ProjectConfig)>> {
        let mut projects = Vec::new();
        
        // Walk through the workspace directory to find cue.toml files
        let mut entries = walkdir::WalkDir::new(&self.workspace_root)
            .follow_links(true)
            .into_iter()
            .filter_map(|e| e.ok());
        
        while let Some(entry) = entries.next() {
            if entry.file_name() == "cue.toml" {
                let project_path = entry.path().parent().unwrap_or(&self.workspace_root);
                
                // Check if this path should be excluded
                if self.should_exclude_path(project_path, exclude_patterns)? {
                    debug!("Excluding project path: {}", project_path.display());
                    continue;
                }
                
                if let Some(project_config) = self.load_project_config(project_path).await? {
                    projects.push((project_path.to_path_buf(), project_config));
                }
            }
        }
        
        Ok(projects)
    }
    
    /// Load projects from specific glob patterns
    async fn load_projects_from_patterns(&self, patterns: &[String], exclude_patterns: &Option<Vec<String>>) -> Result<Vec<(PathBuf, ProjectConfig)>> {
        let mut projects = Vec::new();
        
        for pattern in patterns {
            let full_pattern = self.workspace_root.join(pattern);
            debug!("Searching for projects with pattern: {}", full_pattern.display());
            
            let matches = glob(full_pattern.to_str().unwrap())?;
            for entry in matches {
                match entry {
                    Ok(path) => {
                        // Check if this path should be excluded
                        if self.should_exclude_path(&path, exclude_patterns)? {
                            debug!("Excluding project path: {}", path.display());
                            continue;
                        }
                        
                        if let Some(project_config) = self.load_project_config(&path).await? {
                            projects.push((path, project_config));
                        }
                    }
                    Err(e) => {
                        warn!("Failed to match glob pattern {}: {}", pattern, e);
                    }
                }
            }
        }
        
        Ok(projects)
    }
    
    /// Check if a path should be excluded based on exclude patterns
    fn should_exclude_path(&self, path: &std::path::Path, exclude_patterns: &Option<Vec<String>>) -> Result<bool> {
        if let Some(exclude_patterns) = exclude_patterns {
            for pattern in exclude_patterns {
                let full_pattern = self.workspace_root.join(pattern);
                if let Ok(matches) = glob(full_pattern.to_str().unwrap()) {
                    for entry in matches {
                        if let Ok(excluded_path) = entry {
                            if path.starts_with(&excluded_path) {
                                return Ok(true);
                            }
                        }
                    }
                }
            }
        }
        Ok(false)
    }
    
    /// Load a single project configuration
    pub async fn load_project_config(&self, project_path: &Path) -> Result<Option<ProjectConfig>> {
        let cue_file = if project_path.is_file() {
            project_path.to_path_buf()
        } else {
            project_path.join("cue.toml")
        };
        
        if !cue_file.exists() {
            debug!("No cue.toml found at: {}", cue_file.display());
            return Ok(None);
        }
        
        debug!("Loading project config from: {}", cue_file.display());
        let content = tokio::fs::read_to_string(&cue_file).await?;
        let config: ProjectConfig = toml::from_str(&content)?;
        
        Ok(Some(config))
    }
    
    /// Get the merged workspace configuration with all projects
    pub async fn get_merged_config(&mut self) -> Result<WorkspaceConfig> {
        // Load workspace config first and extract needed data
        let workspace_config = self.load_workspace_config().await?;
        let workspace_name = workspace_config.workspace.name.clone();
        let cache_config = workspace_config.cache.clone();
        
        // Load all projects
        let projects = self.load_all_projects().await?;
        
        // Merge all project tasks into a single workspace config
        let mut all_tasks = std::collections::HashMap::new();
        
        for (_project_path, project_config) in projects {
            let project_name = &project_config.project.name;
            
            for (task_name, project_task) in &project_config.tasks {
                let full_task_name = format!("{}:{}", project_name, task_name);
                
                let task = TaskDefinition {
                    name: full_task_name.clone(),
                    command: project_task.command.clone(),
                    inputs: project_task.inputs.clone().unwrap_or_default(),
                    outputs: project_task.outputs.clone().unwrap_or_default(),
                    dependencies: project_task.dependencies.clone().unwrap_or_default(),
                    cache: project_task.cache.unwrap_or(true),
                };
                
                all_tasks.insert(full_task_name, task);
            }
        }
        
        // Get cache configuration
        let cache_dir = cache_config.as_ref()
            .and_then(|c| c.size_limit.as_ref())
            .map(|_| ".cue/cache".to_string());
        
        let cache_size_limit = cache_config.as_ref()
            .and_then(|c| c.size_limit.clone());
        
        let cache_max_age = Some("30d".to_string()); // Default 30 days
        
        Ok(WorkspaceConfig {
            name: workspace_name,
            tasks: all_tasks,
            cache_dir,
            cache_size_limit,
            cache_max_age,
        })
    }
    
    /// Find a specific task by name (supports project:task format)
    pub async fn find_task(&mut self, task_name: &str) -> Result<Option<TaskDefinition>> {
        let merged_config = self.get_merged_config().await?;
        Ok(merged_config.tasks.get(task_name).cloned())
    }
}

/// Create a default workspace configuration file
pub async fn create_default_workspace_config(workspace_root: &Path) -> Result<()> {
    let workspace_file = workspace_root.join("cue.workspace.toml");
    
    if workspace_file.exists() {
        return Err(cue_common::CueError::Config(
            "cue.workspace.toml already exists".to_string()
        ));
    }
    
    let default_config = WorkspaceRootConfig {
        workspace: WorkspaceRoot {
            name: "my-monorepo".to_string(),
            projects: Some(cue_common::ProjectsConfig {
                auto_discover: true,
                include: None,
                exclude: Some(vec!["node_modules/".to_string(), ".git/".to_string(), "target/".to_string()]),
            }),
        },
        cache: Some(CacheConfig {
            remote: None,
            size_limit: Some("50GB".to_string()),
            eviction_policy: Some(cue_common::EvictionPolicy::Lru),
        }),
    };
    
    let content = toml::to_string_pretty(&default_config)?;
    tokio::fs::write(&workspace_file, content).await?;
    
    info!("Created default workspace config at: {}", workspace_file.display());
    Ok(())
}

/// Create a default project configuration file
pub async fn create_default_project_config(project_path: &Path, project_name: &str) -> Result<()> {
    let cue_file = project_path.join("cue.toml");
    
    if cue_file.exists() {
        return Err(cue_common::CueError::Config(
            "cue.toml already exists".to_string()
        ));
    }
    
    let default_config = ProjectConfig {
        project: Project {
            name: project_name.to_string(),
            description: Some("A cue project".to_string()),
            version: Some("0.1.0".to_string()),
        },
        tasks: {
            let mut tasks = std::collections::HashMap::new();
            tasks.insert("build".to_string(), ProjectTask {
                command: "cargo build".to_string(),
                inputs: Some(vec!["src/**/*.rs".to_string(), "Cargo.toml".to_string()]),
                outputs: Some(vec!["target/".to_string()]),
                dependencies: Some(vec![]),
                cache: Some(true),
                description: Some("Build the project".to_string()),
            });
            tasks.insert("test".to_string(), ProjectTask {
                command: "cargo test".to_string(),
                inputs: Some(vec!["src/**/*.rs".to_string(), "tests/**/*.rs".to_string()]),
                outputs: Some(vec![]),
                dependencies: Some(vec!["build".to_string()]),
                cache: Some(true),
                description: Some("Run tests".to_string()),
            });
            tasks
        },
    };
    
    let content = toml::to_string_pretty(&default_config)?;
    tokio::fs::write(&cue_file, content).await?;
    
    info!("Created default project config at: {}", cue_file.display());
    Ok(())
}
