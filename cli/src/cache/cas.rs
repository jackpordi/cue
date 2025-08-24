use cue_common::Result;
use sha2::{Sha256, Digest};
use std::path::{Path, PathBuf};
use tokio::fs as tokio_fs;
use tracing::debug;

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
        // Check if it's a symlink
        let metadata = tokio_fs::symlink_metadata(file_path).await?;
        
        let (content, hash) = if metadata.file_type().is_symlink() {
            // For symlinks, store the link target with permissions
            let link_target = tokio_fs::read_link(file_path).await?;
            let permissions = metadata.permissions();
            let mode = self.get_file_mode(&permissions);
            let link_str = format!("SYMLINK:{}:{:o}", link_target.to_string_lossy(), mode);
            let content = link_str.as_bytes().to_vec();
            let hash = self.compute_hash(&content);
            (content, hash)
        } else {
            // For regular files, store content with permissions
            let content = tokio_fs::read(file_path).await?;
            let permissions = metadata.permissions();
            let mode = self.get_file_mode(&permissions);
            
            // Prepend permissions to content
            let mode_str = format!("MODE:{:o}:", mode);
            let mut final_content = mode_str.as_bytes().to_vec();
            final_content.extend_from_slice(&content);
            
            let hash = self.compute_hash(&final_content);
            (final_content, hash)
        };
        
        let cas_path = self.get_cas_path(&hash);
        
        // Create directory if it doesn't exist
        if let Some(parent) = cas_path.parent() {
            tokio_fs::create_dir_all(parent).await?;
        }
        
        // Write file to CAS
        tokio_fs::write(&cas_path, &content).await?;
        debug!("Stored file {} in CAS as {} ({} bytes)", file_path.display(), hash, content.len());
        
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
        
        // Read content from CAS
        let content = tokio_fs::read(&cas_path).await?;
        
        // Check if this is a symlink
        if let Ok(content_str) = std::str::from_utf8(&content) {
            if let Some(symlink_data) = content_str.strip_prefix("SYMLINK:") {
                // Parse symlink target and mode
                let parts: Vec<&str> = symlink_data.splitn(2, ':').collect();
                if parts.len() == 2 {
                    let link_target = parts[0];
                    let mode_str = parts[1];
                    
                    // Create symlink
                    #[cfg(unix)]
                    {
                        // Remove target if it exists
                        if target_path.exists() {
                            if tokio_fs::remove_file(target_path).await.is_err() {
                                let _ = tokio_fs::remove_dir_all(target_path).await;
                            }
                        }
                        tokio_fs::symlink(link_target, target_path).await?;
                        
                        // Skip setting permissions on symlinks for now - this may be causing issues
                        // if let Ok(mode) = u32::from_str_radix(mode_str, 8) {
                        //     self.set_file_mode(target_path, mode).await?;
                        // }
                    }
                    #[cfg(windows)]
                    {
                        // On Windows, we'll create a regular file with the content for compatibility
                        let link_target_path = target_path.parent().unwrap().join(link_target);
                        if link_target_path.exists() {
                            tokio_fs::copy(&link_target_path, target_path).await?;
                        } else {
                            tokio_fs::write(target_path, content).await?;
                        }
                    }
                    debug!("Retrieved symlink {} -> {} to {}", hash, link_target, target_path.display());
                } else {
                    // Fallback for old format without permissions
                    #[cfg(unix)]
                    {
                        // Remove target if it exists
                        if target_path.exists() {
                            if tokio_fs::remove_file(target_path).await.is_err() {
                                let _ = tokio_fs::remove_dir_all(target_path).await;
                            }
                        }
                        tokio_fs::symlink(symlink_data, target_path).await?;
                    }
                    #[cfg(windows)]
                    {
                        let link_target_path = target_path.parent().unwrap().join(symlink_data);
                        if link_target_path.exists() {
                            tokio_fs::copy(&link_target_path, target_path).await?;
                        } else {
                            tokio_fs::write(target_path, content).await?;
                        }
                    }
                    debug!("Retrieved symlink {} -> {} to {}", hash, symlink_data, target_path.display());
                }
            } else if let Some(mode_data) = content_str.strip_prefix("MODE:") {
                // Parse mode and content for regular files
                let parts: Vec<&str> = mode_data.splitn(2, ':').collect();
                if parts.len() == 2 {
                    let mode_str = parts[0];
                    let file_content = &content[format!("MODE:{}:", mode_str).len()..];
                    
                    // Remove target if it exists
                    if target_path.exists() {
                        if tokio_fs::remove_file(target_path).await.is_err() {
                                let _ = tokio_fs::remove_dir_all(target_path).await;
                            }
                    }
                    
                    // Write file content
                    tokio_fs::write(target_path, file_content).await?;
                    
                    // Set permissions
                    if let Ok(mode) = u32::from_str_radix(mode_str, 8) {
                        self.set_file_mode(target_path, mode).await?;
                    }
                    
                    debug!("Retrieved blob {} to {} ({} bytes, mode {:o})", hash, target_path.display(), file_content.len(), u32::from_str_radix(mode_str, 8).unwrap_or(0));
                } else {
                    // Fallback for malformed mode data
                    if target_path.exists() {
                        if tokio_fs::remove_file(target_path).await.is_err() {
                                let _ = tokio_fs::remove_dir_all(target_path).await;
                            }
                    }
                    tokio_fs::write(target_path, &content).await?;
                    debug!("Retrieved blob {} to {} ({} bytes)", hash, target_path.display(), content.len());
                }
            } else {
                // Regular file without mode prefix (old format)
                if target_path.exists() {
                    if tokio_fs::remove_file(target_path).await.is_err() {
                                let _ = tokio_fs::remove_dir_all(target_path).await;
                            }
                }
                tokio_fs::write(target_path, &content).await?;
                debug!("Retrieved blob {} to {} ({} bytes)", hash, target_path.display(), content.len());
            }
        } else {
            // Binary file - check if it has mode prefix
            if content.len() > 5 && content.starts_with(b"MODE:") {
                // Try to parse as binary file with mode
                if let Some(colon_pos) = content[5..].iter().position(|&b| b == b':') {
                    let mode_end = 5 + colon_pos;
                    if let Ok(mode_str) = std::str::from_utf8(&content[5..mode_end]) {
                        if let Ok(mode) = u32::from_str_radix(mode_str, 8) {
                            let file_content = &content[mode_end + 1..];
                            if target_path.exists() {
                                if tokio_fs::remove_file(target_path).await.is_err() {
                                let _ = tokio_fs::remove_dir_all(target_path).await;
                            }
                            }
                            tokio_fs::write(target_path, file_content).await?;
                            self.set_file_mode(target_path, mode).await?;
                            debug!("Retrieved binary blob {} to {} ({} bytes, mode {:o})", hash, target_path.display(), file_content.len(), mode);
                            return Ok(());
                        }
                    }
                }
            }
            
            // Fallback: binary file without mode
            if target_path.exists() {
                if tokio_fs::remove_file(target_path).await.is_err() {
                                let _ = tokio_fs::remove_dir_all(target_path).await;
                            }
            }
            tokio_fs::write(target_path, &content).await?;
            debug!("Retrieved blob {} to {} ({} bytes)", hash, target_path.display(), content.len());
        }
        
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
    
    /// Get file mode (permissions) from std::fs::Permissions
    #[cfg(unix)]
    fn get_file_mode(&self, permissions: &std::fs::Permissions) -> u32 {
        use std::os::unix::fs::PermissionsExt;
        permissions.mode()
    }
    
    #[cfg(not(unix))]
    fn get_file_mode(&self, _permissions: &std::fs::Permissions) -> u32 {
        0o644 // Default permissions for non-Unix systems
    }
    
    /// Set file mode (permissions) on a file
    #[cfg(unix)]
    async fn set_file_mode(&self, file_path: &Path, mode: u32) -> Result<()> {
        use std::os::unix::fs::PermissionsExt;
        let permissions = std::fs::Permissions::from_mode(mode);
        tokio_fs::set_permissions(file_path, permissions).await?;
        Ok(())
    }
    
    #[cfg(not(unix))]
    async fn set_file_mode(&self, _file_path: &Path, _mode: u32) -> Result<()> {
        // No-op on non-Unix systems
        Ok(())
    }
}
