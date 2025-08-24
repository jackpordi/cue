use cue_common::Result;
use crate::cache::{cas::ContentAddressableStore, database::CacheDatabase};
use tracing::{info, debug};
use chrono::{Duration, Utc};
use bytesize::ByteSize;

pub struct GarbageCollector {
    cas: ContentAddressableStore,
    db: CacheDatabase,
}

impl GarbageCollector {
    pub fn new(cas: ContentAddressableStore, db: CacheDatabase) -> Self {
        Self { cas, db }
    }
    
    /// Run garbage collection based on size limit and max age
    pub async fn collect(&self, size_limit: Option<u64>, max_age: Option<Duration>) -> Result<GcStats> {
        let mut stats = GcStats::default();
        
        // Collect by age first
        if let Some(max_age) = max_age {
            let age_stats = self.collect_by_age(max_age).await?;
            stats.actions_deleted += age_stats.actions_deleted;
            stats.blobs_deleted += age_stats.blobs_deleted;
            stats.bytes_freed += age_stats.bytes_freed;
        }
        
        // Then collect by size if still needed
        if let Some(size_limit) = size_limit {
            let size_stats = self.collect_by_size(size_limit).await?;
            stats.actions_deleted += size_stats.actions_deleted;
            stats.blobs_deleted += size_stats.blobs_deleted;
            stats.bytes_freed += size_stats.bytes_freed;
        }
        
        // Clean up unreferenced blobs
        let unreferenced_stats = self.cleanup_unreferenced_blobs().await?;
        stats.blobs_deleted += unreferenced_stats.blobs_deleted;
        stats.bytes_freed += unreferenced_stats.bytes_freed;
        
        info!(
            "Garbage collection completed: {} actions, {} blobs, {} freed",
            stats.actions_deleted,
            stats.blobs_deleted,
            ByteSize(stats.bytes_freed)
        );
        
        Ok(stats)
    }
    
    /// Collect cache entries older than the specified age
    async fn collect_by_age(&self, max_age: Duration) -> Result<GcStats> {
        let mut stats = GcStats::default();
        let cutoff_time = Utc::now() - max_age;
        
        // Get all actions ordered by last_access (oldest first)
        let actions = self.db.get_actions_by_access_time(None).await?;
        
        for action in actions {
            if action.last_access < cutoff_time {
                debug!("Deleting old action: {} (last access: {})", action.id, action.last_access);
                
                // Get outputs before deleting the action
                let outputs = self.db.get_outputs(&action.id).await?;
                
                // Delete the action (this will decrement ref counts)
                self.db.delete_action(&action.id).await?;
                stats.actions_deleted += 1;
                
                // Delete blobs that are no longer referenced
                for output in outputs {
                    // Check if blob is still referenced
                    let unreferenced_blobs = self.db.get_unreferenced_blobs().await?;
                    for blob in unreferenced_blobs {
                        if blob.hash == output.blob_hash {
                            // Delete from CAS
                            self.cas.delete_blob(&blob.hash).await?;
                            stats.blobs_deleted += 1;
                            stats.bytes_freed += blob.size;
                            break;
                        }
                    }
                }
            }
        }
        
        Ok(stats)
    }
    
    /// Collect cache entries to stay under the size limit
    async fn collect_by_size(&self, size_limit: u64) -> Result<GcStats> {
        let mut stats = GcStats::default();
        let mut current_size = self.db.get_total_size().await?;
        
        if current_size <= size_limit {
            return Ok(stats);
        }
        
        info!(
            "Cache size {} exceeds limit {}, starting size-based GC",
            ByteSize(current_size),
            ByteSize(size_limit)
        );
        
        // Get actions ordered by last_access (oldest first)
        let actions = self.db.get_actions_by_access_time(None).await?;
        
        for action in actions {
            if current_size <= size_limit {
                break;
            }
            
            debug!("Deleting action for size GC: {} (current: {})", action.id, ByteSize(current_size));
            
            // Get outputs before deleting the action
            let outputs = self.db.get_outputs(&action.id).await?;
            
            // Delete the action (this will decrement ref counts)
            self.db.delete_action(&action.id).await?;
            stats.actions_deleted += 1;
            
            // Delete blobs that are no longer referenced
            for output in outputs {
                let unreferenced_blobs = self.db.get_unreferenced_blobs().await?;
                for blob in unreferenced_blobs {
                    if blob.hash == output.blob_hash {
                        // Delete from CAS
                        self.cas.delete_blob(&blob.hash).await?;
                        stats.blobs_deleted += 1;
                        stats.bytes_freed += blob.size;
                        current_size = current_size.saturating_sub(blob.size);
                        break;
                    }
                }
            }
        }
        
        Ok(stats)
    }
    
    /// Clean up unreferenced blobs
    async fn cleanup_unreferenced_blobs(&self) -> Result<GcStats> {
        let mut stats = GcStats::default();
        
        let unreferenced_blobs = self.db.get_unreferenced_blobs().await?;
        
        for blob in unreferenced_blobs {
            debug!("Deleting unreferenced blob: {}", blob.hash);
            
            // Delete from CAS
            self.cas.delete_blob(&blob.hash).await?;
            stats.blobs_deleted += 1;
            stats.bytes_freed += blob.size;
        }
        
        // Remove from database
        let deleted_count = self.db.delete_unreferenced_blobs().await?;
        debug!("Removed {} unreferenced blobs from database", deleted_count);
        
        Ok(stats)
    }
    
    /// Get cache statistics
    pub async fn get_stats(&self) -> Result<CacheStats> {
        let total_size = self.db.get_total_size().await?;
        let cas_size = self.cas.get_total_size().await?;
        
        // Count actions
        let actions = self.db.get_actions_by_access_time(None).await?;
        let action_count = actions.len();
        
        // Count blobs
        let unreferenced_blobs = self.db.get_unreferenced_blobs().await?;
        let unreferenced_count = unreferenced_blobs.len();
        
        Ok(CacheStats {
            total_size,
            cas_size,
            action_count,
            unreferenced_blob_count: unreferenced_count,
        })
    }
}

#[derive(Debug, Default)]
pub struct GcStats {
    pub actions_deleted: usize,
    pub blobs_deleted: usize,
    pub bytes_freed: u64,
}

#[derive(Debug)]
pub struct CacheStats {
    pub total_size: u64,
    pub cas_size: u64,
    pub action_count: usize,
    pub unreferenced_blob_count: usize,
}
