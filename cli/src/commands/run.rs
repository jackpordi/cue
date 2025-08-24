use cue_common::{Result, TaskDefinition, WorkspaceConfig, CacheKey};
use tracing::{info, debug};
use owo_colors::OwoColorize;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::collections::HashMap;
use std::time::Instant;
use crate::cache::CacheManager;
use crate::config::ConfigManager;
use crate::executor::TaskExecutor;

pub async fn execute(
    task_name: &str, 
    args: &[String], 
    _remote_cache: Option<String>,
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
    
    // If no project specified, run the task on all projects that have it
    if project_name.is_empty() {
        return execute_on_all_projects(task_name_only, args, _remote_cache, ignore_existing_cache, no_cache).await;
    }
    
    info!("Running \"{}\" for project \"{}\"", task_name_only, project_name);
    
    // Find workspace root and initialize config manager
    let start_dir = std::env::current_dir()?;
    let workspace_root = ConfigManager::find_workspace_root(start_dir).await?;
    let mut config_manager = ConfigManager::new(workspace_root.clone());
    
    // Find the task definition
    let task = config_manager.find_task(task_name).await?
        .ok_or_else(|| cue_common::CueError::Config(format!("Task '{}' not found", task_name)))?;
    
    debug!("Task found: {:?}", task);
    
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
        workspace_root.clone()
    };
    
    // Execute the task using the shared function
    execute_single_task(
        &task,
        args,
        &working_dir,
        &workspace_root,
        _remote_cache,
        ignore_existing_cache,
        no_cache,
    ).await
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

#[derive(Debug, Clone)]
enum TaskStatus {
    Running,
    Success,
    Failed(String),
}

const SPINNER_FRAMES: [&str; 4] = ["⠋", "⠙", "⠹", "⠸"];

#[derive(Debug, Clone)]
struct ProgressTracker {
    statuses: Arc<Mutex<HashMap<String, TaskStatus>>>,
    start_time: Arc<Mutex<Instant>>,
    frame: Arc<Mutex<usize>>,
}

impl ProgressTracker {
    fn new(project_names: Vec<String>) -> Self {
        let mut statuses = HashMap::new();
        for project_name in project_names {
            statuses.insert(project_name, TaskStatus::Running);
        }
        
        Self {
            statuses: Arc::new(Mutex::new(statuses)),
            start_time: Arc::new(Mutex::new(Instant::now())),
            frame: Arc::new(Mutex::new(0)),
        }
    }
    
    fn update_status(&self, project_name: &str, status: TaskStatus) {
        if let Ok(mut statuses) = self.statuses.lock() {
            statuses.insert(project_name.to_string(), status);
        }
    }
    
    fn print_progress(&self, task_name: &str) {
        if let Ok(statuses) = self.statuses.lock() {
            // Get current frame for spinner
            let frame_idx = if let Ok(mut frame) = self.frame.lock() {
                let current = *frame;
                *frame = (current + 1) % SPINNER_FRAMES.len();
                current
            } else {
                0
            };
            
            // Clear screen and move cursor to top
            print!("\x1B[2J\x1B[H");
            
            let mut running = 0;
            let mut success = 0;
            let mut failed = 0;
            
            // Print each project on its own line
            for (project_name, status) in statuses.iter() {
                match status {
                    TaskStatus::Running => {
                        println!("- {} : {} {}", project_name, task_name, SPINNER_FRAMES[frame_idx].yellow());
                        running += 1;
                    }
                    TaskStatus::Success => {
                        println!("- {} : {} {}", project_name, task_name, "✓".green());
                        success += 1;
                    }
                    TaskStatus::Failed(_) => {
                        println!("- {} : {} {}", project_name, task_name, "✗".red());
                        failed += 1;
                    }
                }
            }
            
            let elapsed = if let Ok(start_time) = self.start_time.lock() {
                start_time.elapsed()
            } else {
                std::time::Duration::from_secs(0)
            };
            let elapsed_str = if elapsed.as_millis() >= 1000 {
                format!("{:.1}s", elapsed.as_millis() as f64 / 1000.0)
            } else {
                format!("{}ms", elapsed.as_millis())
            };
            
            println!("({} running, {} done, {} failed) - {}", running, success, failed, elapsed_str);
            
            // Flush stdout to ensure the output is displayed
            use std::io::{self, Write};
            io::stdout().flush().ok();
        }
    }
    
    fn finalize(&self) {
        if let Ok(statuses) = self.statuses.lock() {
            // Clear the current line and print final results
            println!("\r\x1B[K");
            
            let mut success_count = 0;
            let mut failure_count = 0;
            let mut failures = Vec::new();
            
            for (project_name, status) in statuses.iter() {
                match status {
                    TaskStatus::Success => {
                        success_count += 1;
                    }
                    TaskStatus::Failed(error) => {
                        failure_count += 1;
                        failures.push((project_name.clone(), error.clone()));
                    }
                    TaskStatus::Running => {
                        // This shouldn't happen, but handle it gracefully
                        failure_count += 1;
                        failures.push((project_name.clone(), "Task did not complete".to_string()));
                    }
                }
            }
            
            let total_duration = if let Ok(start_time) = self.start_time.lock() {
                start_time.elapsed()
            } else {
                std::time::Duration::from_secs(0)
            };
            let total_str = if total_duration.as_millis() >= 1000 {
                format!("{:.1}s", total_duration.as_millis() as f64 / 1000.0)
            } else {
                format!("{}ms", total_duration.as_millis())
            };
            
            if failure_count == 0 {
                info!("All {} projects completed successfully in {}", success_count, total_str);
            } else {
                info!("Completed {} projects, {} failed in {}", success_count, failure_count, total_str);
                for (project_name, error) in failures {
                    eprintln!("Project \"{}\" failed: {}", project_name, error);
                }
            }
        }
    }
}

/// Execute a task on all projects that have that task defined
async fn execute_on_all_projects(
    task_name: &str,
    args: &[String],
    _remote_cache: Option<String>,
    ignore_existing_cache: bool,
    no_cache: bool,
) -> Result<()> {
    info!("Running \"{}\" on all projects", task_name);
    
    // Find workspace root and initialize config manager
    let start_dir = std::env::current_dir()?;
    let workspace_root = ConfigManager::find_workspace_root(start_dir).await?;
    let mut config_manager = ConfigManager::new(workspace_root.clone());
    
    // Load workspace config first
    config_manager.load_workspace_config().await?;
    
    // Load all projects
    let projects = config_manager.load_all_projects().await?;
    
    // Find projects that have the specified task
    let mut projects_with_task = Vec::new();
    for (project_path, project_config) in projects {
        if project_config.tasks.contains_key(task_name) {
            projects_with_task.push((project_path, project_config));
        }
    }
    
    if projects_with_task.is_empty() {
        return Err(cue_common::CueError::Config(
            format!("No projects found with task '{}'", task_name)
        ));
    }
    
    info!("Found {} projects with task \"{}\"", projects_with_task.len(), task_name);
    
    // Create progress tracker
    let project_names: Vec<String> = projects_with_task.iter()
        .map(|(_, config)| config.project.name.clone())
        .collect();
    let progress_tracker = ProgressTracker::new(project_names);
    
    // Execute tasks concurrently
    let mut handles = Vec::new();
    
    for (project_path, project_config) in projects_with_task {
        let project_name = project_config.project.name.clone();
        let full_task_name = format!("{}:{}", project_name, task_name);
        
        // Create task definition
        let project_task = &project_config.tasks[task_name];
        let task = TaskDefinition {
            name: full_task_name.clone(),
            command: project_task.command.clone(),
            inputs: project_task.inputs.clone().unwrap_or_default(),
            outputs: project_task.outputs.clone().unwrap_or_default(),
            dependencies: project_task.dependencies.clone().unwrap_or_default(),
            cache: project_task.cache.unwrap_or(true),
        };
        
        let progress_tracker = progress_tracker.clone();
        let workspace_root = workspace_root.clone();
        let args = args.to_vec();
        let _remote_cache = _remote_cache.clone();
        let project_name_clone = project_name.clone();
        
        // Spawn task execution
        let handle = tokio::spawn(async move {
            let result = execute_single_task_silent(
                &task,
                &args,
                &project_path,
                &workspace_root,
                _remote_cache,
                ignore_existing_cache,
                no_cache,
            ).await;
            
            // Update progress
            let status = match &result {
                Ok(_) => TaskStatus::Success,
                Err(e) => TaskStatus::Failed(e.to_string()),
            };
            progress_tracker.update_status(&project_name_clone, status);
            
            result
        });
        
        handles.push((project_name, handle));
    }
    
    // Monitor progress and update display
    let progress_tracker_clone = progress_tracker.clone();
    let task_name_clone = task_name.to_string();
    let progress_handle = tokio::spawn(async move {
        loop {
            progress_tracker_clone.print_progress(&task_name_clone);
            
            // Check if all tasks are complete
            if let Ok(statuses) = progress_tracker_clone.statuses.lock() {
                let all_complete = statuses.values().all(|status| {
                    matches!(status, TaskStatus::Success | TaskStatus::Failed(_))
                });
                if all_complete {
                    break;
                }
            }
            
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
        }
    });
    
    // Wait for all tasks to complete
    let mut results = Vec::new();
    for (project_name, handle) in handles {
        match handle.await {
            Ok(result) => {
                results.push((project_name, result));
            }
            Err(e) => {
                results.push((project_name, Err(cue_common::CueError::Execution(
                    format!("Task panicked: {}", e)
                ))));
            }
        }
    }
    
    // Wait for progress display to finish
    progress_handle.await.ok();
    
    // Print final results
    progress_tracker.finalize();
    
    // Check if any tasks failed
    let failure_count = results.iter().filter(|(_, result)| result.is_err()).count();
    if failure_count > 0 {
        return Err(cue_common::CueError::Execution(
            format!("{} projects failed", failure_count)
        ));
    }
    
    Ok(())
}

/// Execute a single task silently (suppresses stdout/stderr output)
async fn execute_single_task_silent(
    task: &TaskDefinition,
    args: &[String],
    project_path: &PathBuf,
    workspace_root: &PathBuf,
    _remote_cache: Option<String>,
    ignore_existing_cache: bool,
    no_cache: bool,
) -> Result<()> {
    let _start_time = std::time::Instant::now();
    
    // Initialize config manager
    let mut config_manager = ConfigManager::new(workspace_root.clone());
    
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
    
    // Use project path as working directory
    let working_dir = project_path.clone();
    
    // Check cache first (unless disabled)
    if task.cache && !no_cache {
        let cache_key = compute_cache_key(&task, &config, args, &executor, &working_dir).await?;
        
        if let Some(cache_entry) = cache_manager.get(&cache_key).await? {
            if ignore_existing_cache {
                debug!("Cache hit ignored due to --ignore-existing-cache flag, executing task");
            } else {
                debug!("Cache hit! Restoring outputs from cache");
                
                // Materialize outputs to filesystem (silently)
                cache_manager.materialize_outputs(&cache_entry, &working_dir).await?;
                
                // Don't restore stdout/stderr for silent execution
                return Ok(());
            }
        } else {
            debug!("Cache miss, executing task");
        }
    } else if no_cache {
        debug!("Cache disabled due to --no-cache flag, executing task");
    }
    
    // Execute the task silently (no live output)
    let command_start = std::time::Instant::now();
    let result = executor.execute_silent(&task, args, &working_dir).await?;
    let _command_duration = command_start.elapsed();
    
    // Store in cache if enabled and not disabled
    let cache_start = std::time::Instant::now();
    if task.cache && !no_cache {
        let cache_key = compute_cache_key(&task, &config, args, &executor, &working_dir).await?;
        
        // Collect actual output files
        let output_files = executor.collect_output_files(&task, &working_dir).await?;
        
        let _entry_id = cache_manager.store_task_outputs(
            &cache_key,
            result.exit_code,
            Some(&result.stdout),
            Some(&result.stderr),
            &output_files,
            result.duration_ms, // Store the actual execution time
        ).await?;
        
        debug!("Task results stored in cache");
    } else if no_cache {
        debug!("Cache storage disabled due to --no-cache flag");
    }
    let _cache_duration = cache_start.elapsed();
    
    // Check if task failed and return error with stderr if available
    if result.exit_code != 0 {
        let error_msg = if !result.stderr.is_empty() {
            String::from_utf8_lossy(&result.stderr).to_string()
        } else {
            format!("Task failed with exit code {}", result.exit_code)
        };
        return Err(cue_common::CueError::Execution(error_msg));
    }
    
    Ok(())
}

/// Execute a single task (extracted from the main execute function for reuse)
async fn execute_single_task(
    task: &TaskDefinition,
    args: &[String],
    project_path: &PathBuf,
    workspace_root: &PathBuf,
    _remote_cache: Option<String>,
    ignore_existing_cache: bool,
    no_cache: bool,
) -> Result<()> {
    let start_time = std::time::Instant::now();
    
    // Initialize config manager
    let mut config_manager = ConfigManager::new(workspace_root.clone());
    
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
    
    // Use project path as working directory
    let working_dir = project_path.clone();
    
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
                        if !stdout_content.is_empty() {
                            // Write raw bytes to preserve colors
                            use std::io::Write;
                            std::io::stdout().write_all(&stdout_content).ok();
                        }
                    }
                }
                
                if let Some(stderr_hash) = &cache_entry.metadata.stderr_hash {
                    if let Ok(stderr_content) = cache_manager.cas.get_content(stderr_hash).await {
                        if !stderr_content.is_empty() {
                            // Write raw bytes to preserve colors
                            use std::io::Write;
                            std::io::stderr().write_all(&stderr_content).ok();
                        }
                    }
                }
                
                // For cache hits, show the time taken vs the original execution time
                let cache_hit_time = start_time.elapsed();
                let cache_hit_ms = cache_hit_time.as_millis() as u64;
                let original_duration_ms = cache_entry.metadata.duration_ms as u64;
                
                if original_duration_ms > cache_hit_ms {
                    let saved_ms = original_duration_ms - cache_hit_ms;
                    let cache_hit_str = if cache_hit_ms >= 1000 { format!("{:.1}s", cache_hit_ms as f64 / 1000.0) } else { format!("{}ms", cache_hit_ms) };
                    let saved_str = if saved_ms >= 1000 { format!("{:.1}s", saved_ms as f64 / 1000.0) } else { format!("{}ms", saved_ms) };
                    info!("{} Task took {} (saved {})", "Cache hit".green(), cache_hit_str, saved_str);
                } else {
                    let cache_hit_str = if cache_hit_ms >= 1000 { format!("{:.1}s", cache_hit_ms as f64 / 1000.0) } else { format!("{}ms", cache_hit_ms) };
                    info!("{} Task took {}", "Result restored from cache".green(), cache_hit_str);
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
    let command_start = std::time::Instant::now();
    let result = executor.execute_with_live_output(&task, args, &working_dir).await?;
    let command_duration = command_start.elapsed();
    
    // Store in cache if enabled and not disabled
    let cache_start = std::time::Instant::now();
    if task.cache && !no_cache {
        let cache_key = compute_cache_key(&task, &config, args, &executor, &working_dir).await?;
        
        // Collect actual output files
        let output_files = executor.collect_output_files(&task, &working_dir).await?;
        
        let _entry_id = cache_manager.store_task_outputs(
            &cache_key,
            result.exit_code,
            Some(&result.stdout),
            Some(&result.stderr),
            &output_files,
            result.duration_ms, // Store the actual execution time
        ).await?;
        
        debug!("Task results stored in cache");
    } else if no_cache {
        debug!("Cache storage disabled due to --no-cache flag");
    }
    let cache_duration = cache_start.elapsed();
    
    // Calculate total time (from start of function to here)
    let total_duration = start_time.elapsed();
    
    // Format timing strings
    let total_str = if total_duration.as_millis() >= 1000 { format!("{:.1}s", total_duration.as_millis() as f64 / 1000.0) } else { format!("{}ms", total_duration.as_millis()) };
    let command_str = if command_duration.as_millis() >= 1000 { format!("{:.1}s", command_duration.as_millis() as f64 / 1000.0) } else { format!("{}ms", command_duration.as_millis()) };
    let cache_str = if cache_duration.as_millis() >= 1000 { format!("{:.1}s", cache_duration.as_millis() as f64 / 1000.0) } else { format!("{}ms", cache_duration.as_millis()) };
    
    info!("No cache hit. Total: {}, Command: {}, Cache: {}", total_str, command_str, cache_str);
    Ok(())
}


