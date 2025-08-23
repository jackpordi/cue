use cue_common::{Result, CacheEntry, CacheKey};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;
use chrono::Utc;

#[derive(Clone)]
pub struct CacheService {
    storage: Arc<RwLock<HashMap<String, CacheEntry>>>,
}

impl CacheService {
    pub async fn new() -> Self {
        Self {
            storage: Arc::new(RwLock::new(HashMap::new())),
        }
    }
    
    pub async fn get(&self, key: &CacheKey) -> Result<Option<CacheEntry>> {
        let storage = self.storage.read().await;
        let cache_key = self.generate_cache_key(key);
        
        if let Some(mut entry) = storage.get(&cache_key).cloned() {
            // Update accessed_at
            entry.accessed_at = Utc::now();
            Ok(Some(entry))
        } else {
            Ok(None)
        }
    }
    
    pub async fn set(&self, entry: CacheEntry) -> Result<Uuid> {
        let mut storage = self.storage.write().await;
        let cache_key = self.generate_cache_key(&entry.key);
        
        storage.insert(cache_key, entry.clone());
        
        Ok(entry.id)
    }
    
    fn generate_cache_key(&self, key: &CacheKey) -> String {
        format!("{}:{}:{}", key.task_name, key.inputs_hash, key.workspace_hash)
    }
}
