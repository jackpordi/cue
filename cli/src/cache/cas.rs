use cue_common::Result;
use sha2::{Sha256, Digest};
use std::path::{Path, PathBuf};
use std::fs;
use tokio::fs as tokio_fs;
use tracing::{debug, warn};

#[derive(Clone)]
pub struct ContentAddressableStore {
    root: PathBuf,
}

impl ContentAddressableStore {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }
    
    /// Store a file in the CAS and return its hash
    pub async fn store_file(&self, file_path: &Path) -> Result<String> {
        let content = tokio_fs::read(file_path).await?;
        let hash = self.compute_hash(&content);
        let cas_path = self.get_cas_path(&hash);
        
        // Create directory if it doesn't exist
        if let Some(parent) = cas_path.parent() {
            tokio_fs::create_dir_all(parent).await?;
        }
        
        // Write file to CAS
        tokio_fs::write(&cas_path, &content).await?;
        debug!("Stored file {} in CAS as {}", file_path.display(), hash);
        
        Ok(hash)
    }
    
    /// Store content directly in the CAS and return its hash
    pub async fn store_content(&self, content: &[u8]) -> Result<String> {
        let hash = self.compute_hash(content);
        let cas_path = self.get_cas_path(&hash);
        
        // Create directory if it doesn't exist
        if let Some(parent) = cas_path.parent() {
            tokio_fs::create_dir_all(parent).await?;
        }
        
        // Write content to CAS
        tokio_fs::write(&cas_path, content).await?;
        debug!("Stored content in CAS as {}", hash);
        
        Ok(hash)
    }
    
    /// Retrieve a file from the CAS by hash
    pub async fn get_file(&self, hash: &str, target_path: &Path) -> Result<()> {
        let cas_path = self.get_cas_path(hash);
        
        if !cas_path.exists() {
            return Err(cue_common::CueError::Cache(format!("Blob not found: {}", hash)));
        }
        
        // Create target directory if it doesn't exist
        if let Some(parent) = target_path.parent() {
            tokio_fs::create_dir_all(parent).await?;
        }
        
        // Copy file from CAS to target location
        tokio_fs::copy(&cas_path, target_path).await?;
        debug!("Retrieved blob {} to {}", hash, target_path.display());
        
        Ok(())
    }
    
    /// Get content from the CAS by hash
    pub async fn get_content(&self, hash: &str) -> Result<Vec<u8>> {
        let cas_path = self.get_cas_path(hash);
        
        if !cas_path.exists() {
            return Err(cue_common::CueError::Cache(format!("Blob not found: {}", hash)));
        }
        
        let content = tokio_fs::read(&cas_path).await?;
        debug!("Retrieved blob {} ({} bytes)", hash, content.len());
        
        Ok(content)
    }
    
    /// Delete a blob from the CAS
    pub async fn delete_blob(&self, hash: &str) -> Result<()> {
        let cas_path = self.get_cas_path(hash);
        
        if cas_path.exists() {
            tokio_fs::remove_file(&cas_path).await?;
            debug!("Deleted blob {}", hash);
        }
        
        Ok(())
    }
    
    /// Check if a blob exists in the CAS
    pub fn blob_exists(&self, hash: &str) -> bool {
        self.get_cas_path(hash).exists()
    }
    
    /// Get the size of a blob
    pub async fn get_blob_size(&self, hash: &str) -> Result<u64> {
        let cas_path = self.get_cas_path(hash);
        
        if !cas_path.exists() {
            return Err(cue_common::CueError::Cache(format!("Blob not found: {}", hash)));
        }
        
        let metadata = tokio_fs::metadata(&cas_path).await?;
        Ok(metadata.len())
    }
    
    /// Compute the total size of all blobs in the CAS
    pub async fn get_total_size(&self) -> Result<u64> {
        let mut total_size = 0u64;
        
        if self.root.exists() {
            let mut entries = tokio_fs::read_dir(&self.root).await?;
            
            while let Some(entry) = entries.next_entry().await? {
                let path = entry.path();
                if path.is_file() {
                    let metadata = tokio_fs::metadata(&path).await?;
                    total_size += metadata.len();
                }
            }
        }
        
        Ok(total_size)
    }
    
    /// Compute SHA256 hash of content
    fn compute_hash(&self, content: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(content);
        hex::encode(hasher.finalize())
    }
    
    /// Get the CAS path for a given hash
    fn get_cas_path(&self, hash: &str) -> PathBuf {
        if hash.len() < 3 {
            return self.root.join(hash);
        }
        
        let prefix = &hash[..3];
        let suffix = &hash[3..];
        self.root.join(prefix).join(suffix)
    }
}
