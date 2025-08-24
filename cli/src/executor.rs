// Task execution for the CLI
// This module will handle running tasks and capturing their output

use cue_common::{Result, TaskDefinition};
use std::path::PathBuf;
use std::process::Stdio;
use tokio::process::Command;
use tokio::time::Instant;
use tracing::{info, debug, warn};
use glob::glob;

pub struct TaskExecutor;

impl TaskExecutor {
    pub fn new() -> Self {
        Self
    }
    
    pub async fn execute(&self, task: &TaskDefinition, args: &[String], working_dir: &PathBuf) -> Result<TaskExecutionResult> {
        self.execute_with_options(task, args, working_dir, false).await
    }
    
    pub async fn execute_with_live_output(&self, task: &TaskDefinition, args: &[String], working_dir: &PathBuf) -> Result<TaskExecutionResult> {
        self.execute_with_options(task, args, working_dir, true).await
    }
    
    async fn execute_with_options(&self, task: &TaskDefinition, args: &[String], working_dir: &PathBuf, live_output: bool) -> Result<TaskExecutionResult> {
        let start_time = Instant::now();
        
        debug!("Executing task: {} with args: {:?} in {}", task.command, args, working_dir.display());
        
        // Build the command using the default shell
        let mut cmd = Command::new("sh");
        cmd.arg("-c")
           .arg(&task.command)
           .current_dir(working_dir);
        
        // Configure stdio based on live_output flag
        if live_output {
            cmd.stdout(Stdio::inherit())
               .stderr(Stdio::inherit());
        } else {
            cmd.stdout(Stdio::piped())
               .stderr(Stdio::piped());
        }
        
        // Add any additional arguments
        if !args.is_empty() {
            cmd.arg("--").args(args);
        }
        
        debug!("Running command: {:?}", cmd);
        
        // Execute the command based on live_output mode
        let (output, stdout, stderr, final_exit_code) = if live_output {
            let status = cmd.status().await.map_err(|e| {
                cue_common::CueError::Execution(format!("Failed to execute command: {}", e))
            })?;
            
            // For live output, we don't capture stdout/stderr
            let exit_code = status.code().unwrap_or(-1);
            (None, String::new(), String::new(), exit_code)
        } else {
            let output = cmd.output().await.map_err(|e| {
                cue_common::CueError::Execution(format!("Failed to execute command: {}", e))
            })?;
            
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            let exit_code = output.status.code().unwrap_or(-1);
            (Some(output), stdout, stderr, exit_code)
        };
        
        let duration = start_time.elapsed();
        let duration_ms = duration.as_millis() as u64;
        
        info!("Task completed with exit code: {} (took {}ms)", final_exit_code, duration_ms);
        
        if !stdout.is_empty() {
            debug!("STDOUT: {}", stdout);
        }
        
        if !stderr.is_empty() {
            debug!("STDERR: {}", stderr);
        }
        
        Ok(TaskExecutionResult {
            exit_code: final_exit_code,
            stdout,
            stderr,
            duration_ms: duration_ms as i64,
        })
    }
    
    /// Collect output files based on the task's output patterns
    pub async fn collect_output_files(&self, task: &TaskDefinition, working_dir: &PathBuf) -> Result<Vec<(PathBuf, PathBuf)>> {
        let mut output_files = Vec::new();
        
        for output_pattern in &task.outputs {
            let pattern_path = working_dir.join(output_pattern);
            let pattern_str = pattern_path.to_string_lossy();
            
            debug!("Collecting outputs matching pattern: {}", pattern_str);
            
            // Handle directory patterns (ending with /)
            if output_pattern.ends_with('/') {
                // For directory patterns, we need to collect all files recursively
                let dir_path = working_dir.join(output_pattern.strip_suffix('/').unwrap_or(output_pattern));
                if dir_path.exists() && dir_path.is_dir() {
                    debug!("Collecting all files from directory: {}", dir_path.display());
                    for entry in walkdir::WalkDir::new(&dir_path) {
                        if let Ok(entry) = entry {
                            let path = entry.path();
                            // Use symlink_metadata to avoid following symlinks during collection
                            if let Ok(metadata) = path.symlink_metadata() {
                                if metadata.is_file() || metadata.file_type().is_symlink() {
                                let relative_path = path.strip_prefix(working_dir)
                                    .map_err(|_| cue_common::CueError::Execution("Failed to compute relative path".to_string()))?;
                                output_files.push((path.to_path_buf(), relative_path.to_path_buf()));
                                debug!("Found output file: {}", relative_path.display());
                                }
                            }
                        }
                    }
                }
            } else if output_pattern.ends_with("/*") {
                // Handle wildcard directory patterns
                if let Ok(entries) = glob(&pattern_str) {
                    for entry in entries {
                        if let Ok(path) = entry {
                            // Use symlink_metadata to avoid following symlinks during collection
                            if let Ok(metadata) = path.symlink_metadata() {
                                if metadata.is_file() || metadata.file_type().is_symlink() {
                                let relative_path = path.strip_prefix(working_dir)
                                    .map_err(|_| cue_common::CueError::Execution("Failed to compute relative path".to_string()))?;
                                output_files.push((path.clone(), relative_path.to_path_buf()));
                                debug!("Found output file: {}", relative_path.display());
                                }
                            }
                        }
                    }
                }
            } else {
                // Handle file patterns
                if let Ok(entries) = glob(&pattern_str) {
                    for entry in entries {
                        if let Ok(path) = entry {
                            // Use symlink_metadata to avoid following symlinks during collection
                            if let Ok(metadata) = path.symlink_metadata() {
                                if metadata.is_file() || metadata.file_type().is_symlink() {
                                let relative_path = path.strip_prefix(working_dir)
                                    .map_err(|_| cue_common::CueError::Execution("Failed to compute relative path".to_string()))?;
                                output_files.push((path.clone(), relative_path.to_path_buf()));
                                debug!("Found output file: {}", relative_path.display());
                                }
                            }
                        }
                    }
                }
            }
        }
        
        debug!("Collected {} output files", output_files.len());
        
        // Debug: List all collected files
        for (source_path, relative_path) in &output_files {
            debug!("Collected: {} -> {}", source_path.display(), relative_path.display());
        }
        
        Ok(output_files)
    }
    
    /// Compute hash of input files for cache key generation
    pub async fn compute_inputs_hash(&self, task: &TaskDefinition, working_dir: &PathBuf) -> Result<String> {
        use sha2::{Sha256, Digest};
        use hex;
        
        let mut hasher = Sha256::new();
        
        for input_pattern in &task.inputs {
            let pattern_path = working_dir.join(input_pattern);
            let pattern_str = pattern_path.to_string_lossy();
            
            debug!("Computing hash for input pattern: {}", pattern_str);
            
            if let Ok(entries) = glob(&pattern_str) {
                for entry in entries {
                    if let Ok(path) = entry {
                        if path.is_file() {
                            // Read file content and hash it
                            match tokio::fs::read(&path).await {
                                Ok(content) => {
                                    let mut file_hasher = Sha256::new();
                                    file_hasher.update(&content);
                                    let file_hash = hex::encode(file_hasher.finalize());
                                    
                                    // Include file path and hash in overall hash
                                    hasher.update(path.to_string_lossy().as_bytes());
                                    hasher.update(file_hash.as_bytes());
                                    
                                    debug!("Hashed input file: {} -> {}", path.display(), file_hash);
                                }
                                Err(e) => {
                                    warn!("Failed to read input file {}: {}", path.display(), e);
                                }
                            }
                        }
                    }
                }
            }
        }
        
        let inputs_hash = hex::encode(hasher.finalize());
        debug!("Computed inputs hash: {}", inputs_hash);
        Ok(inputs_hash)
    }
}

#[derive(Debug)]
pub struct TaskExecutionResult {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    pub duration_ms: i64,
}
