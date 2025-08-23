use serde::{Deserialize, Serialize};
use crate::cache::{CacheEntry, CacheKey};

// API Request/Response types for the cloud service

#[derive(Debug, Serialize, Deserialize)]
pub struct GetCacheRequest {
    pub key: CacheKey,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GetCacheResponse {
    pub found: bool,
    pub entry: Option<CacheEntry>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SetCacheRequest {
    pub entry: CacheEntry,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SetCacheResponse {
    pub success: bool,
    pub id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
    pub uptime: u64,
}

// API endpoints
pub const API_BASE_PATH: &str = "/api/v1";
pub const GET_CACHE_PATH: &str = "/cache/get";
pub const SET_CACHE_PATH: &str = "/cache/set";
pub const HEALTH_PATH: &str = "/health";
