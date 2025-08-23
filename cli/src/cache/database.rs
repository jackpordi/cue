use cue_common::{Result, ActionInfo, BlobInfo, OutputMapping};
use sqlx::{sqlite::SqlitePool, Row};
use std::path::PathBuf;
use tracing::{debug, warn};
use uuid::Uuid;
use chrono::Utc;

#[derive(Clone)]
pub struct CacheDatabase {
    pool: SqlitePool,
}

impl CacheDatabase {
    pub async fn new(db_path: PathBuf) -> Result<Self> {
        // Create parent directory if it doesn't exist
        if let Some(parent) = db_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        
        let database_url = format!("sqlite:{}", db_path.display());
        let pool = SqlitePool::connect(&database_url).await?;
        
        // Initialize database schema
        Self::init_schema(&pool).await?;
        
        Ok(Self { pool })
    }
    
    async fn init_schema(pool: &SqlitePool) -> Result<()> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS actions (
                id TEXT PRIMARY KEY,
                cache_key TEXT UNIQUE NOT NULL,
                exit_code INTEGER NOT NULL,
                stdout_hash TEXT,
                stderr_hash TEXT,
                created_at TEXT NOT NULL,
                last_access TEXT NOT NULL
            )
            "#
        ).execute(pool).await?;
        
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS outputs (
                action_id TEXT NOT NULL,
                blob_hash TEXT NOT NULL,
                relative_path TEXT NOT NULL,
                PRIMARY KEY (action_id, relative_path),
                FOREIGN KEY (action_id) REFERENCES actions(id) ON DELETE CASCADE
            )
            "#
        ).execute(pool).await?;
        
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS blobs (
                hash TEXT PRIMARY KEY,
                size INTEGER NOT NULL,
                ref_count INTEGER NOT NULL DEFAULT 1,
                last_access TEXT NOT NULL
            )
            "#
        ).execute(pool).await?;
        
        // Create indexes for better performance
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_actions_cache_key ON actions(cache_key)").execute(pool).await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_actions_last_access ON actions(last_access)").execute(pool).await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_outputs_action_id ON outputs(action_id)").execute(pool).await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_blobs_ref_count ON blobs(ref_count)").execute(pool).await?;
        
        Ok(())
    }
    
    /// Store an action in the database
    pub async fn store_action(&self, action: &ActionInfo) -> Result<()> {
        sqlx::query(
            r#"
            INSERT OR REPLACE INTO actions (id, cache_key, exit_code, stdout_hash, stderr_hash, created_at, last_access)
            VALUES (?, ?, ?, ?, ?, ?, ?)
            "#
        )
        .bind(action.id.to_string())
        .bind(&action.cache_key)
        .bind(action.exit_code)
        .bind(&action.stdout_hash)
        .bind(&action.stderr_hash)
        .bind(action.created_at.to_rfc3339())
        .bind(action.last_access.to_rfc3339())
        .execute(&self.pool)
        .await?;
        
        debug!("Stored action {} in database", action.id);
        Ok(())
    }
    
    /// Get an action by cache key
    pub async fn get_action(&self, cache_key: &str) -> Result<Option<ActionInfo>> {
        let row = sqlx::query(
            r#"
            SELECT id, cache_key, exit_code, stdout_hash, stderr_hash, created_at, last_access
            FROM actions WHERE cache_key = ?
            "#
        )
        .bind(cache_key)
        .fetch_optional(&self.pool)
        .await?;
        
        if let Some(row) = row {
            let action = ActionInfo {
                id: Uuid::parse_str(&row.get::<String, _>("id"))?,
                cache_key: row.get("cache_key"),
                exit_code: row.get("exit_code"),
                stdout_hash: row.get("stdout_hash"),
                stderr_hash: row.get("stderr_hash"),
                created_at: chrono::DateTime::parse_from_rfc3339(&row.get::<String, _>("created_at"))?.with_timezone(&Utc),
                last_access: chrono::DateTime::parse_from_rfc3339(&row.get::<String, _>("last_access"))?.with_timezone(&Utc),
            };
            
            // Update last_access timestamp
            self.update_action_access(&action.id).await?;
            
            Ok(Some(action))
        } else {
            Ok(None)
        }
    }
    
    /// Update the last_access timestamp for an action
    async fn update_action_access(&self, action_id: &Uuid) -> Result<()> {
        sqlx::query(
            "UPDATE actions SET last_access = ? WHERE id = ?"
        )
        .bind(Utc::now().to_rfc3339())
        .bind(action_id.to_string())
        .execute(&self.pool)
        .await?;
        
        Ok(())
    }
    
    /// Store output mappings for an action
    pub async fn store_outputs(&self, action_id: &Uuid, outputs: &[(String, String)]) -> Result<()> {
        let mut transaction = self.pool.begin().await?;
        
        for (blob_hash, relative_path) in outputs {
            // Insert output mapping
            sqlx::query(
                r#"
                INSERT OR REPLACE INTO outputs (action_id, blob_hash, relative_path)
                VALUES (?, ?, ?)
                "#
            )
            .bind(action_id.to_string())
            .bind(blob_hash)
            .bind(relative_path)
            .execute(&mut *transaction)
            .await?;
            
            // Increment blob reference count
            sqlx::query(
                r#"
                INSERT INTO blobs (hash, size, ref_count, last_access)
                VALUES (?, 0, 1, ?)
                ON CONFLICT(hash) DO UPDATE SET
                    ref_count = ref_count + 1,
                    last_access = ?
                "#
            )
            .bind(blob_hash)
            .bind(Utc::now().to_rfc3339())
            .bind(Utc::now().to_rfc3339())
            .execute(&mut *transaction)
            .await?;
        }
        
        transaction.commit().await?;
        debug!("Stored {} outputs for action {}", outputs.len(), action_id);
        Ok(())
    }
    
    /// Get output mappings for an action
    pub async fn get_outputs(&self, action_id: &Uuid) -> Result<Vec<OutputMapping>> {
        let rows = sqlx::query(
            "SELECT action_id, blob_hash, relative_path FROM outputs WHERE action_id = ?"
        )
        .bind(action_id.to_string())
        .fetch_all(&self.pool)
        .await?;
        
        let outputs: Vec<OutputMapping> = rows
            .into_iter()
            .map(|row| OutputMapping {
                action_id: Uuid::parse_str(&row.get::<String, _>("action_id")).unwrap(),
                blob_hash: row.get("blob_hash"),
                relative_path: row.get("relative_path"),
            })
            .collect();
        
        Ok(outputs)
    }
    
    /// Delete an action and its outputs
    pub async fn delete_action(&self, action_id: &Uuid) -> Result<()> {
        let mut transaction = self.pool.begin().await?;
        
        // Get all outputs for this action
        let outputs = self.get_outputs(action_id).await?;
        
        // Decrement reference counts for all blobs
        for output in &outputs {
            sqlx::query(
                r#"
                UPDATE blobs SET ref_count = ref_count - 1, last_access = ?
                WHERE hash = ? AND ref_count > 0
                "#
            )
            .bind(Utc::now().to_rfc3339())
            .bind(&output.blob_hash)
            .execute(&mut *transaction)
            .await?;
        }
        
        // Delete the action (outputs will be deleted due to CASCADE)
        sqlx::query("DELETE FROM actions WHERE id = ?")
            .bind(action_id.to_string())
            .execute(&mut *transaction)
            .await?;
        
        transaction.commit().await?;
        debug!("Deleted action {} and {} outputs", action_id, outputs.len());
        Ok(())
    }
    
    /// Get actions ordered by last_access (oldest first)
    pub async fn get_actions_by_access_time(&self, limit: Option<i64>) -> Result<Vec<ActionInfo>> {
        let query = if let Some(limit) = limit {
            sqlx::query(
                r#"
                SELECT id, cache_key, exit_code, stdout_hash, stderr_hash, created_at, last_access
                FROM actions ORDER BY last_access ASC LIMIT ?
                "#
            ).bind(limit)
        } else {
            sqlx::query(
                r#"
                SELECT id, cache_key, exit_code, stdout_hash, stderr_hash, created_at, last_access
                FROM actions ORDER BY last_access ASC
                "#
            )
        };
        
        let rows = query.fetch_all(&self.pool).await?;
        
        let actions: Vec<ActionInfo> = rows
            .into_iter()
            .map(|row| ActionInfo {
                id: Uuid::parse_str(&row.get::<String, _>("id")).unwrap(),
                cache_key: row.get("cache_key"),
                exit_code: row.get("exit_code"),
                stdout_hash: row.get("stdout_hash"),
                stderr_hash: row.get("stderr_hash"),
                created_at: chrono::DateTime::parse_from_rfc3339(&row.get::<String, _>("created_at")).unwrap().with_timezone(&Utc),
                last_access: chrono::DateTime::parse_from_rfc3339(&row.get::<String, _>("last_access")).unwrap().with_timezone(&Utc),
            })
            .collect();
        
        Ok(actions)
    }
    
    /// Get blobs with zero reference count
    pub async fn get_unreferenced_blobs(&self) -> Result<Vec<BlobInfo>> {
        let rows = sqlx::query(
            r#"
            SELECT hash, size, ref_count, last_access
            FROM blobs WHERE ref_count = 0
            "#
        )
        .fetch_all(&self.pool)
        .await?;
        
        let blobs: Vec<BlobInfo> = rows
            .into_iter()
            .map(|row| BlobInfo {
                hash: row.get("hash"),
                size: row.get("size"),
                ref_count: row.get("ref_count"),
                last_access: chrono::DateTime::parse_from_rfc3339(&row.get::<String, _>("last_access")).unwrap().with_timezone(&Utc),
            })
            .collect();
        
        Ok(blobs)
    }
    
    /// Delete unreferenced blobs
    pub async fn delete_unreferenced_blobs(&self) -> Result<usize> {
        let result = sqlx::query("DELETE FROM blobs WHERE ref_count = 0")
            .execute(&self.pool)
            .await?;
        
        let deleted_count = result.rows_affected() as usize;
        debug!("Deleted {} unreferenced blobs", deleted_count);
        Ok(deleted_count)
    }
    
    /// Get total cache size
    pub async fn get_total_size(&self) -> Result<u64> {
        let row = sqlx::query("SELECT COALESCE(SUM(size), 0) as total_size FROM blobs")
            .fetch_one(&self.pool)
            .await?;
        
        Ok(row.get::<i64, _>("total_size") as u64)
    }
}
