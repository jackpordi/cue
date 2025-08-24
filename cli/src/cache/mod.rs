pub mod cas;
pub mod database;
pub mod gc;

use cue_common::{CacheKey, CacheEntry, ActionInfo, Result};
use std::path::PathBuf;
use tracing::{info, debug};
use uuid::Uuid;
use chrono::Utc;

pub struct CacheManager {
    pub cas: cas::ContentAddressableStore,
    pub db: database::CacheDatabase,
    cache_dir: PathBuf,
}

impl CacheManager {
    pub async fn new(cache_dir: PathBuf) -> Result<Self> {
        debug!("Initializing cache manager at: {}", cache_dir.display());
        
        // Ensure cache directory exists
        tokio::fs::create_dir_all(&cache_dir).await?;
        
        let cas = cas::ContentAddressableStore::new(cache_dir.join("cas"));
        let db = database::CacheDatabase::new(cache_dir.join("db.sqlite")).await?;
        
        debug!("Cache manager initialized successfully");
        
        Ok(Self {
            cas,
            db,
            cache_dir,
        })
    }
    
    /// Get a cache entry by key
    pub async fn get(&self, key: &CacheKey) -> Result<Option<CacheEntry>> {
        let cache_key = self.compute_cache_key(key);
        
        // Look up action in database
        if let Some(action) = self.db.get_action(&cache_key).await? {
            debug!("Cache hit for key: {}", cache_key);
            
            // Get output mappings
            let outputs = self.db.get_outputs(&action.id).await?;
            
            // Convert outputs to HashMap
            let mut output_map = std::collections::HashMap::new();
            for output in outputs {
                debug!("get: Found output: relative_path='{}', blob_hash='{}'", output.relative_path, output.blob_hash);
                output_map.insert(output.relative_path, output.blob_hash);
            }
            
            // Create cache entry
            let entry = CacheEntry {
                id: action.id,
                key: key.clone(),
                outputs: output_map,
                metadata: cue_common::CacheMetadata {
                    duration_ms: action.duration_ms,
                    exit_code: action.exit_code,
                    stdout_hash: action.stdout_hash,
                    stderr_hash: action.stderr_hash,
                    dependencies: vec![], // TODO: Store this in database
                },
                created_at: action.created_at,
                accessed_at: action.last_access,
            };
            
            Ok(Some(entry))
        } else {
            debug!("Cache miss for key: {}", cache_key);
            Ok(None)
        }
    }
    
    /// Store a cache entry
    pub async fn set(&self, entry: CacheEntry) -> Result<()> {
        let cache_key = self.compute_cache_key(&entry.key);
        
        // Create action info
        let action = ActionInfo {
            id: entry.id,
            cache_key: cache_key.clone(),
            exit_code: entry.metadata.exit_code,
            stdout_hash: entry.metadata.stdout_hash.clone(),
            stderr_hash: entry.metadata.stderr_hash.clone(),
            duration_ms: entry.metadata.duration_ms,
            created_at: entry.created_at,
            last_access: entry.accessed_at,
        };
        
        // Store action in database
        self.db.store_action(&action).await?;
        
        // Convert outputs to list format for database
        let outputs: Vec<(String, String)> = entry.outputs
            .iter()
            .map(|(path, hash)| (hash.clone(), path.clone()))
            .collect();
        
        // Store output mappings
        self.db.store_outputs(&entry.id, &outputs, &self.cas).await?;
        
        debug!("Stored cache entry {} with {} outputs", entry.id, outputs.len());
        Ok(())
    }
    
    /// Get stdout content from cache entry
    pub async fn get_stdout(&self, entry: &CacheEntry) -> Result<Option<String>> {
        if let Some(stdout_hash) = &entry.metadata.stdout_hash {
            let content = self.cas.get_content(stdout_hash).await?;
            Ok(Some(String::from_utf8_lossy(&content).to_string()))
        } else {
            Ok(None)
        }
    }
    
    /// Get stderr content from cache entry
    pub async fn get_stderr(&self, entry: &CacheEntry) -> Result<Option<String>> {
        if let Some(stderr_hash) = &entry.metadata.stderr_hash {
            let content = self.cas.get_content(stderr_hash).await?;
            Ok(Some(String::from_utf8_lossy(&content).to_string()))
        } else {
            Ok(None)
        }
    }
    
    /// Materialize cache entry outputs to the filesystem
    pub async fn materialize_outputs(&self, entry: &CacheEntry, base_path: &PathBuf) -> Result<()> {
        use futures::stream::{FuturesUnordered, StreamExt};
        
        // Pre-create all needed directories to reduce I/O overhead
        let mut directories = std::collections::HashSet::new();
        for (relative_path, _) in &entry.outputs {
            let target_path = base_path.join(relative_path);
            if let Some(parent) = target_path.parent() {
                directories.insert(parent.to_path_buf());
            }
        }
        
        // Create all directories in parallel
        let dir_futures: Vec<_> = directories.into_iter().map(|dir| {
            tokio::fs::create_dir_all(dir)
        }).collect();
        futures::future::join_all(dir_futures).await;
        
        // Create a stream of futures for parallel execution
        let mut futures = FuturesUnordered::new();
        
        for (relative_path, blob_hash) in &entry.outputs {
            debug!("materialize_outputs: Processing relative_path: '{}'", relative_path);
            let target_path = base_path.join(relative_path);
            debug!("materialize_outputs: Target path: {}", target_path.display());
            let cas = self.cas.clone();
            let blob_hash = blob_hash.clone();
            let _relative_path = relative_path.clone();
            
            futures.push(async move {
                // Retrieve file from CAS (directory already created)
                if let Err(e) = cas.get_file_without_dir_creation(&blob_hash, &target_path).await {
                    return Err(cue_common::CueError::Cache(format!(
                        "Failed to materialize file '{}' (hash: {}): {}", 
                        target_path.display(), 
                        blob_hash, 
                        e
                    )));
                }
                debug!("Materialized output: {}", target_path.display());
                Ok(())
            });
        }
        
        // Process all futures concurrently with optimized batch processing
        let mut count = 0;
        let mut buffer = Vec::new();
        
        // Process in larger batches for better throughput and reduce context switching
        while let Some(result) = futures.next().await {
            buffer.push(result);
            count += 1;
            
            // Process batch when we have enough results or when stream is done
            // Increased batch size for better performance
            if buffer.len() >= 100 || futures.is_empty() {
                for result in buffer.drain(..) {
                    result?; // Propagate any errors
                }
            }
        }
        
        // Process any remaining results
        for result in buffer {
            result?;
        }
        
        debug!("Materialized {} outputs for cache entry {}", count, entry.id);
        Ok(())
    }
    
    /// Store task outputs in the cache
    pub async fn store_task_outputs(
        &self,
        key: &CacheKey,
        exit_code: i32,
        stdout: Option<&[u8]>,
        stderr: Option<&[u8]>,
        output_files: &[(PathBuf, PathBuf)], // (source_path, relative_path)
        duration_ms: i64,
    ) -> Result<Uuid> {
        let entry_id = Uuid::new_v4();
        let now = Utc::now();
        
        // Store stdout/stderr in CAS if provided
        let stdout_hash = if let Some(stdout_content) = stdout {
            Some(self.cas.store_content(stdout_content).await?)
        } else {
            None
        };
        
        let stderr_hash = if let Some(stderr_content) = stderr {
            Some(self.cas.store_content(stderr_content).await?)
        } else {
            None
        };
        
        // Store output files in CAS
        let mut outputs = std::collections::HashMap::new();
        for (source_path, relative_path) in output_files {
            let blob_hash = self.cas.store_file(source_path).await?;
            outputs.insert(relative_path.to_string_lossy().to_string(), blob_hash);
        }
        
        // Create cache entry
        let entry = CacheEntry {
            id: entry_id,
            key: key.clone(),
            outputs,
            metadata: cue_common::CacheMetadata {
                duration_ms,
                exit_code,
                stdout_hash,
                stderr_hash,
                dependencies: vec![], // TODO: Get from task definition
            },
            created_at: now,
            accessed_at: now,
        };
        
        // Store in cache
        self.set(entry).await?;
        
        Ok(entry_id)
    }
    
    /// Run garbage collection
    pub async fn cleanup(&self, size_limit: Option<u64>, max_age: Option<chrono::Duration>) -> Result<()> {
        let gc = gc::GarbageCollector::new(self.cas.clone(), self.db.clone());
        let stats = gc.collect(size_limit, max_age).await?;
        
        info!(
            "Cache cleanup completed: {} actions, {} blobs, {} bytes freed",
            stats.actions_deleted,
            stats.blobs_deleted,
            stats.bytes_freed
        );
        
        Ok(())
    }
    
    /// Get cache statistics
    pub async fn get_stats(&self) -> Result<gc::CacheStats> {
        let gc = gc::GarbageCollector::new(self.cas.clone(), self.db.clone());
        gc.get_stats().await
    }
    
    /// Compute cache key from task inputs
    fn compute_cache_key(&self, key: &CacheKey) -> String {
        // Create a deterministic string representation
        let key_data = format!(
            "{}:{}:{}:{}:{}",
            key.task_name,
            key.inputs_hash,
            key.workspace_hash,
            key.command_hash,
            key.environment_hash
        );
        
        // Hash the key data
        use sha2::{Sha256, Digest};
        let mut hasher = Sha256::new();
        hasher.update(key_data.as_bytes());
        hex::encode(hasher.finalize())
    }
}
