use cue_common::{Result, TaskDefinition, WorkspaceConfig, CacheKey};
use tracing::{info, warn, debug};
use owo_colors::OwoColorize;
use std::path::PathBuf;
use crate::cache::CacheManager;
use crate::config::ConfigManager;
use crate::executor::TaskExecutor;

pub async fn execute(
    task_name: &str, 
    args: &[String], 
    remote_cache: Option<String>,
    ignore_existing_cache: bool,
    no_cache: bool,
) -> Result<()> {
    // Parse task name to extract project and task
    let (project_name, task_name_only) = if let Some(colon_pos) = task_name.find(':') {
        let project = &task_name[..colon_pos];
        let task = &task_name[colon_pos + 1..];
        (project, task)
    } else {
        ("", task_name)
    };
    
    info!("Running \"{}\" for project \"{}\"", task_name_only, project_name);
    
    let start_time = std::time::Instant::now();
    
    // Find workspace root and initialize config manager
    let start_dir = std::env::current_dir()?;
    let workspace_root = ConfigManager::find_workspace_root(start_dir).await?;
    let mut config_manager = ConfigManager::new(workspace_root.clone());
    
    // Find the task definition
    let task = config_manager.find_task(task_name).await?
        .ok_or_else(|| cue_common::CueError::Config(format!("Task '{}' not found", task_name)))?;
    
    debug!("Task found: {:?}", task);
    
    // Get merged workspace configuration
    let config = config_manager.get_merged_config().await?;
    
    // Initialize cache manager (always relative to workspace root)
    let cache_dir = config.cache_dir
        .clone()
        .map(|path| workspace_root.join(path))
        .unwrap_or_else(|| workspace_root.join(".cue/cache"));
    
    let cache_manager = CacheManager::new(cache_dir).await?;
    
    // Initialize task executor
    let executor = TaskExecutor::new();
    
    // Determine working directory for the task
    let working_dir = if let Some(project_name) = task_name.split(':').next() {
        // Find the project directory
        let projects = config_manager.load_all_projects().await?;
        let mut found_dir = workspace_root.clone();
        for (project_path, _) in projects {
            if project_path.file_name().and_then(|n| n.to_str()) == Some(project_name) {
                found_dir = project_path;
                break;
            }
        }
        found_dir
    } else {
        workspace_root
    };
    
    // Check cache first (unless disabled)
    if task.cache && !no_cache {
        let cache_key = compute_cache_key(&task, &config, args, &executor, &working_dir).await?;
        
        if let Some(cache_entry) = cache_manager.get(&cache_key).await? {
            if ignore_existing_cache {
                info!("Cache hit ignored due to --ignore-existing-cache flag, executing task");
            } else {
                debug!("Cache hit! Restoring outputs from cache");
                
                // Materialize outputs to filesystem
                cache_manager.materialize_outputs(&cache_entry, &working_dir).await?;
                
                // Restore stdout/stderr if available
                if let Some(stdout_hash) = &cache_entry.metadata.stdout_hash {
                    if let Ok(stdout_content) = cache_manager.cas.get_content(stdout_hash).await {
                        debug!("STDOUT: {}", String::from_utf8_lossy(&stdout_content));
                    }
                }
                
                if let Some(stderr_hash) = &cache_entry.metadata.stderr_hash {
                    if let Ok(stderr_content) = cache_manager.cas.get_content(stderr_hash).await {
                        debug!("STDERR: {}", String::from_utf8_lossy(&stderr_content));
                    }
                }
                
                let total_time = start_time.elapsed();
                let total_ms = total_time.as_millis() as u64;
                let saved_ms = cache_entry.metadata.duration_ms as u64;
                
                if saved_ms > 0 {
                    // Show how much time was saved compared to the original execution
                    info!("{} Task took {}ms (would have taken {}ms)", "Cache hit".green(), total_ms, saved_ms);
                } else {
                    info!("{} Task took {}ms", "Cache hit".green(), total_ms);
                }
                return Ok(());
            }
        } else {
            debug!("Cache miss, executing task");
        }
    } else if no_cache {
        debug!("Cache disabled due to --no-cache flag, executing task");
    }
    
    // Execute the task with live output for non-cached runs
    let result = executor.execute_with_live_output(&task, args, &working_dir).await?;
    
    // Store in cache if enabled and not disabled
    if task.cache && !no_cache {
        let cache_key = compute_cache_key(&task, &config, args, &executor, &working_dir).await?;
        
        // Collect actual output files
        let output_files = executor.collect_output_files(&task, &working_dir).await?;
        
        let _entry_id = cache_manager.store_task_outputs(
            &cache_key,
            result.exit_code,
            Some(result.stdout.as_bytes()),
            Some(result.stderr.as_bytes()),
            &output_files,
            result.duration_ms, // Store the actual execution time
        ).await?;
        
        debug!("Task results stored in cache");
    } else if no_cache {
        debug!("Cache storage disabled due to --no-cache flag");
    }
    
    let total_time = start_time.elapsed();
    let total_ms = total_time.as_millis() as u64;
    info!("No cache hit. Task took {}ms", total_ms);
    Ok(())
}

async fn compute_cache_key(
    task: &TaskDefinition, 
    config: &WorkspaceConfig, 
    args: &[String],
    executor: &TaskExecutor,
    working_dir: &PathBuf,
) -> Result<CacheKey> {
    use sha2::{Sha256, Digest};
    use hex;
    
    // Compute inputs hash
    let inputs_hash = executor.compute_inputs_hash(task, working_dir).await?;
    
    // Compute command hash
    let mut hasher = Sha256::new();
    hasher.update(task.command.as_bytes());
    hasher.update(args.join(" ").as_bytes());
    let command_hash = hex::encode(hasher.finalize());
    
    // Compute environment hash (for now, just use a placeholder)
    let environment_hash = "default".to_string();
    
    Ok(CacheKey {
        task_name: task.name.clone(),
        inputs_hash,
        workspace_hash: config.name.clone(),
        command_hash,
        environment_hash,
    })
}

async fn execute_task(task: &TaskDefinition, args: &[String]) -> Result<TaskResult> {
    // This function is no longer used, but keeping for compatibility
    let executor = TaskExecutor::new();
    let working_dir = std::env::current_dir()?;
    let result = executor.execute(task, args, &working_dir).await?;
    
    Ok(TaskResult {
        exit_code: result.exit_code,
        stdout: result.stdout,
        stderr: result.stderr,
        duration_ms: result.duration_ms,
    })
}

#[derive(Debug)]
struct TaskResult {
    exit_code: i32,
    stdout: String,
    stderr: String,
    duration_ms: i64,
}
