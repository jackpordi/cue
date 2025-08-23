use thiserror::Error;

#[derive(Error, Debug)]
pub enum CueError {
    #[error("Cache error: {0}")]
    Cache(String),
    
    #[error("Network error: {0}")]
    Network(String),
    
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),
    
    #[error("UUID error: {0}")]
    Uuid(#[from] uuid::Error),
    
    #[error("Chrono error: {0}")]
    Chrono(#[from] chrono::ParseError),
    
    #[error("Invalid configuration: {0}")]
    Config(String),
    
    #[error("Task execution failed: {0}")]
    TaskExecution(String),
}

pub type Result<T> = std::result::Result<T, CueError>;
