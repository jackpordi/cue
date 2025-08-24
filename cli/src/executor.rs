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
        let command_start_time = Instant::now();
        
        debug!("Executing task: {} with args: {:?} in {}", task.command, args, working_dir.display());
        
        // Build the command using script to create a pseudo-TTY for color preservation
        let mut cmd = Command::new("script");
        cmd.arg("-q")
           .arg("/dev/null")
           .arg("sh")
           .arg("-c")
           .arg(&task.command)
           .current_dir(working_dir);
        
        // Configure stdio based on live_output flag
        if live_output {
            // For live output, we'll use pipes but stream the output in real-time
            cmd.stdout(Stdio::piped())
               .stderr(Stdio::piped());
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
        let (stdout, stderr, final_exit_code) = if live_output {
            // For live output, spawn the process and stream output in real-time
            let mut child = cmd.spawn().map_err(|e| {
                cue_common::CueError::Execution(format!("Failed to spawn command: {}", e))
            })?;
            
            let mut stdout = Vec::new();
            let mut stderr = Vec::new();
            
            // Stream stdout in real-time
            if let Some(mut stdout_pipe) = child.stdout.take() {
                let stdout_task = tokio::spawn(async move {
                    use tokio::io::AsyncReadExt;
                    let mut buffer = [0; 1024];
                    let mut output = Vec::new();
                    loop {
                        match stdout_pipe.read(&mut buffer).await {
                            Ok(0) => break, // EOF
                            Ok(n) => {
                                let chunk = &buffer[..n];
                                print!("{}", String::from_utf8_lossy(chunk)); // Print to terminal immediately
                                output.extend_from_slice(chunk);
                            }
                            Err(_) => break,
                        }
                    }
                    output
                });
                
                // Stream stderr in real-time
                if let Some(mut stderr_pipe) = child.stderr.take() {
                    let stderr_task = tokio::spawn(async move {
                        use tokio::io::AsyncReadExt;
                        let mut buffer = [0; 1024];
                        let mut output = Vec::new();
                        loop {
                            match stderr_pipe.read(&mut buffer).await {
                                Ok(0) => break, // EOF
                                Ok(n) => {
                                    let chunk = &buffer[..n];
                                    eprint!("{}", String::from_utf8_lossy(chunk)); // Print to stderr immediately
                                    output.extend_from_slice(chunk);
                                }
                                Err(_) => break,
                            }
                        }
                        output
                    });
                    
                    // Wait for both streams and the process
                    let (stdout_result, stderr_result, status) = tokio::join!(
                        stdout_task,
                        stderr_task,
                        child.wait()
                    );
                    
                    stdout = stdout_result.unwrap_or_default();
                    stderr = stderr_result.unwrap_or_default();
                    let exit_code = status.map(|s| s.code().unwrap_or(-1)).unwrap_or(-1);
                    (stdout, stderr, exit_code)
                } else {
                    // No stderr pipe
                    let (stdout_result, status) = tokio::join!(stdout_task, child.wait());
                    stdout = stdout_result.unwrap_or_default();
                    let exit_code = status.map(|s| s.code().unwrap_or(-1)).unwrap_or(-1);
                    (stdout, stderr, exit_code)
                }
            } else {
                // No stdout pipe, just wait for the process
                let status = child.wait().await.map_err(|e| {
                    cue_common::CueError::Execution(format!("Failed to wait for command: {}", e))
                })?;
                let exit_code = status.code().unwrap_or(-1);
                (stdout, stderr, exit_code)
            }
        } else {
            let output = cmd.output().await.map_err(|e| {
                cue_common::CueError::Execution(format!("Failed to execute command: {}", e))
            })?;
            
            let stdout = output.stdout;
            let stderr = output.stderr;
            let exit_code = output.status.code().unwrap_or(-1);
            (stdout, stderr, exit_code)
        };
        
        let command_duration = command_start_time.elapsed();
        let command_duration_ms = command_duration.as_millis() as u64;
        
        debug!("Command completed with exit code: {} (took {}ms)", final_exit_code, command_duration_ms);
        
        if !stdout.is_empty() {
            debug!("STDOUT: {} bytes", stdout.len());
        }
        
        if !stderr.is_empty() {
            debug!("STDERR: {} bytes", stderr.len());
        }
        
        Ok(TaskExecutionResult {
            exit_code: final_exit_code,
            stdout,
            stderr,
            duration_ms: command_duration_ms as i64,
        })
    }
    
    /// Collect output files based on the task's output patterns
    pub async fn collect_output_files(&self, task: &TaskDefinition, working_dir: &PathBuf) -> Result<Vec<(PathBuf, PathBuf)>> {
        use std::sync::Arc;
        
        let mut output_files = Vec::new();
        let working_dir = Arc::new(working_dir.clone()); // Share ownership efficiently
        
        for output_pattern in &task.outputs {
            let pattern_path = working_dir.join(output_pattern);
            let pattern_str = pattern_path.to_string_lossy();
            
            debug!("Collecting outputs matching pattern: {}", pattern_str);
            
            // Handle directory patterns (ending with /)
            if output_pattern.ends_with('/') {
                let dir_path = working_dir.join(output_pattern.strip_suffix('/').unwrap_or(output_pattern));
                if dir_path.exists() && dir_path.is_dir() {
                    debug!("Collecting all files from directory: {}", dir_path.display());
                    
                    // Use parallel processing for large directories
                    let entries: Vec<_> = walkdir::WalkDir::new(&dir_path)
                        .into_iter()
                        .filter_map(|e| e.ok())
                        .collect();
                    
                    // Process entries in parallel batches
                    for chunk in entries.chunks(100) {
                        let chunk_futures: Vec<_> = chunk.iter().map(|entry| {
                            let path = entry.path();
                            let working_dir = Arc::clone(&working_dir); // Share Arc reference
                            
                            async move {
                                if let Ok(metadata) = path.symlink_metadata() {
                                    if metadata.is_file() || metadata.file_type().is_symlink() {
                                        if let Ok(relative_path) = path.strip_prefix(&*working_dir) {
                                            return Some((path.to_path_buf(), relative_path.to_path_buf()));
                                        }
                                    }
                                }
                                None
                            }
                        }).collect();
                        
                        for result in futures::future::join_all(chunk_futures).await {
                            if let Some(file_pair) = result {
                                output_files.push(file_pair);
                            }
                        }
                    }
                }
            } else if output_pattern.ends_with("/*") {
                // Handle wildcard directory patterns
                if let Ok(entries) = glob(&pattern_str) {
                    let paths: Vec<_> = entries.filter_map(|e| e.ok()).collect();
                    
                    // Process glob results in parallel - use Arc to avoid cloning
                    let path_futures: Vec<_> = paths.iter().map(|path| {
                        let path = path.clone(); // We still need to clone for async move
                        let working_dir = Arc::clone(&working_dir); // Share Arc reference
                        
                        async move {
                            if let Ok(metadata) = path.symlink_metadata() {
                                if metadata.is_file() || metadata.file_type().is_symlink() {
                                    if let Ok(relative_path) = path.strip_prefix(&*working_dir) {
                                        return Some((path.clone(), relative_path.to_path_buf())); // Clone needed for tuple
                                    }
                                }
                            }
                            None
                        }
                    }).collect();
                    
                    for result in futures::future::join_all(path_futures).await {
                        if let Some(file_pair) = result {
                            output_files.push(file_pair);
                        }
                    }
                }
            } else {
                // Handle file patterns
                if let Ok(entries) = glob(&pattern_str) {
                    let paths: Vec<_> = entries.filter_map(|e| e.ok()).collect();
                    
                    // Process glob results in parallel - use Arc to avoid cloning
                    let path_futures: Vec<_> = paths.iter().map(|path| {
                        let path = path.clone(); // We still need to clone for async move
                        let working_dir = Arc::clone(&working_dir); // Share Arc reference
                        
                        async move {
                            if let Ok(metadata) = path.symlink_metadata() {
                                if metadata.is_file() || metadata.file_type().is_symlink() {
                                    if let Ok(relative_path) = path.strip_prefix(&*working_dir) {
                                        return Some((path.clone(), relative_path.to_path_buf())); // Clone needed for tuple
                                    }
                                }
                            }
                            None
                        }
                    }).collect();
                    
                    for result in futures::future::join_all(path_futures).await {
                        if let Some(file_pair) = result {
                            output_files.push(file_pair);
                        }
                    }
                }
            }
        }
        
        debug!("Collected {} output files", output_files.len());
        
        // Only log individual files in debug mode and limit the number
        for (source_path, relative_path) in output_files.iter().take(10) {
            debug!("Collected: {} -> {}", source_path.display(), relative_path.display());
        }
        if output_files.len() > 10 {
            debug!("... and {} more files", output_files.len() - 10);
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
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub duration_ms: i64,
}
