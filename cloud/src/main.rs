use axum::{
    routing::{get, post},
    Router,
    http::StatusCode,
    Json,
    extract::State,
};
use cue_common::{
    schema::{GetCacheRequest, GetCacheResponse, SetCacheRequest, SetCacheResponse, HealthResponse},
    API_BASE_PATH, GET_CACHE_PATH, SET_CACHE_PATH, HEALTH_PATH,
};
use std::net::SocketAddr;
use tracing::{info, error};

mod cache;
mod database;

use cache::CacheService;

#[tokio::main]
async fn main() {
    // Initialize logging
    tracing_subscriber::fmt::init();
    
    info!("Starting cue cloud service");
    
    // Initialize cache service
    let cache_service = CacheService::new().await;
    
    // Build our application with a route
    let app = Router::new()
        .route(&format!("{}{}", API_BASE_PATH, HEALTH_PATH), get(health))
        .route(&format!("{}{}", API_BASE_PATH, GET_CACHE_PATH), post(get_cache))
        .route(&format!("{}{}", API_BASE_PATH, SET_CACHE_PATH), post(set_cache))
        .with_state(cache_service);
    
    // Run it
    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    info!("Listening on {}", addr);
    
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "healthy".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        uptime: 0, // TODO: Track actual uptime
    })
}

async fn get_cache(
    State(cache_service): State<CacheService>,
    Json(payload): Json<GetCacheRequest>,
) -> Result<Json<GetCacheResponse>, StatusCode> {
    match cache_service.get(&payload.key).await {
        Ok(entry) => Ok(Json(GetCacheResponse {
            found: entry.is_some(),
            entry,
        })),
        Err(e) => {
            error!("Failed to get cache: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

async fn set_cache(
    State(cache_service): State<CacheService>,
    Json(payload): Json<SetCacheRequest>,
) -> Result<Json<SetCacheResponse>, StatusCode> {
    match cache_service.set(payload.entry).await {
        Ok(id) => Ok(Json(SetCacheResponse {
            success: true,
            id: Some(id.to_string()),
        })),
        Err(e) => {
            error!("Failed to set cache: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}
