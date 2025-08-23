use cue_common::Result;
use tracing::info;
use std::fs;
use std::path::Path;

pub async fn execute(clean_cache: bool) -> Result<()> {
    info!("Cleaning build artifacts");
    
    // Clean build artifacts
    let build_dirs = vec![
        "target",
        "dist",
        "build",
        "out",
    ];
    
    for dir in build_dirs {
        if Path::new(dir).exists() {
            info!("Removing directory: {}", dir);
            fs::remove_dir_all(dir)?;
        }
    }
    
    // Clean cache if requested
    if clean_cache {
        info!("Cleaning cache");
        let cache_dirs = vec![
            ".cue/cache",
            ".cache",
        ];
        
        for dir in cache_dirs {
            if Path::new(dir).exists() {
                info!("Removing cache directory: {}", dir);
                fs::remove_dir_all(dir)?;
            }
        }
    }
    
    info!("Clean completed");
    Ok(())
}
