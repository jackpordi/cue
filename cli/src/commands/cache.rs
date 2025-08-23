use cue_common::Result;
use tracing::info;
use std::path::PathBuf;
use crate::cache::CacheManager;
use crate::commands::CacheCommands;
use bytesize::ByteSize;
use humantime::Duration as HumanDuration;
use chrono;

pub async fn execute(subcommand: CacheCommands) -> Result<()> {
    match subcommand {
        CacheCommands::Stats => {
            show_cache_stats().await?;
        }
        CacheCommands::Clean { size_limit, max_age } => {
            clean_cache(size_limit, max_age).await?;
        }
    }
    Ok(())
}

async fn show_cache_stats() -> Result<()> {
    let cache_dir = PathBuf::from(".cue/cache");
    let cache_manager = CacheManager::new(cache_dir).await?;
    
    let stats = cache_manager.get_stats().await?;
    
    println!("Cache Statistics:");
    println!("  Total size: {}", ByteSize(stats.total_size));
    println!("  CAS size: {}", ByteSize(stats.cas_size));
    println!("  Actions: {}", stats.action_count);
    println!("  Unreferenced blobs: {}", stats.unreferenced_blob_count);
    
    Ok(())
}

async fn clean_cache(size_limit: Option<String>, max_age: Option<String>) -> Result<()> {
    let cache_dir = PathBuf::from(".cue/cache");
    let cache_manager = CacheManager::new(cache_dir).await?;
    
    // Parse size limit
    let size_limit_bytes = if let Some(size_limit_str) = size_limit {
        parse_size_limit(&size_limit_str)?
    } else {
        None
    };
    
    // Parse max age
    let max_age_duration = if let Some(max_age_str) = max_age {
        parse_duration(&max_age_str)?
    } else {
        None
    };
    
    info!("Cleaning cache with size_limit={:?}, max_age={:?}", 
          size_limit_bytes.map(|s| ByteSize(s)), 
          max_age_duration);
    
    cache_manager.cleanup(size_limit_bytes, max_age_duration).await?;
    
    println!("Cache cleanup completed successfully");
    Ok(())
}

fn parse_size_limit(size_str: &str) -> Result<Option<u64>> {
    if size_str == "0" || size_str.is_empty() {
        return Ok(None);
    }
    
    // Parse size with units (e.g., "10GB", "1.5TB")
    let size_str = size_str.to_uppercase();
    
    let multiplier = if size_str.ends_with("KB") {
        1024
    } else if size_str.ends_with("MB") {
        1024 * 1024
    } else if size_str.ends_with("GB") {
        1024 * 1024 * 1024
    } else if size_str.ends_with("TB") {
        1024u64 * 1024 * 1024 * 1024
    } else {
        1 // Assume bytes if no unit
    };
    
    let number_str = if multiplier == 1 {
        size_str.clone()
    } else {
        size_str[..size_str.len() - 2].to_string()
    };
    
    let number: f64 = number_str.parse()
        .map_err(|_| cue_common::CueError::Config(format!("Invalid size format: {}", size_str)))?;
    
    Ok(Some((number * multiplier as f64) as u64))
}

fn parse_duration(duration_str: &str) -> Result<Option<chrono::Duration>> {
    if duration_str == "0" || duration_str.is_empty() {
        return Ok(None);
    }
    
    let duration = duration_str.parse::<HumanDuration>()
        .map_err(|_| cue_common::CueError::Config(format!("Invalid duration format: {}", duration_str)))?;
    
    let seconds = duration.as_secs();
    Ok(Some(chrono::Duration::seconds(seconds as i64)))
}
