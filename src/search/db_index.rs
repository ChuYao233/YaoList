//! 数据库搜索索引 - 路径前缀压缩
//! 
//! 优化目标：80万文件 ≈ 100MB（约125字节/文件）
//! 
//! 存储方案：
//! - 每个存储一个独立数据库文件（避免并发锁冲突）
//! - dirs表：存储目录路径（parent_id + name），复用同目录
//! - files表：存储文件（dir_id + name + name_lower）
//! - 同目录下的文件共享dir_id，大幅减少路径重复
//!
//! 特性：
//! - 每个存储独立SQLite + WAL模式（并发安全）
//! - 路径前缀压缩
//! - 批量插入优化 + 重试机制

use sqlx::{Pool, Sqlite, Row, sqlite::SqlitePoolOptions};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

use super::tokenizer::to_simplified;
use crate::config;

/// 搜索结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchHit {
    pub path: String,
    pub name: String,
    pub is_dir: bool,
    pub size: i64,
    pub modified: i64,
    pub score: f32,
}

/// 索引统计
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IndexStats {
    pub file_count: u64,
    pub dir_count: u64,
    pub total_size: u64,
    pub last_updated: Option<i64>,
}

/// 数据库搜索索引
pub struct DbIndex {
    db: Pool<Sqlite>,
    driver_id: Option<String>,
}

impl DbIndex {
    /// 关闭数据库连接池 / Close database connection pool
    pub async fn close(&self) {
        self.db.close().await;
    }
}

impl DbIndex {
    /// 获取数据库路径 / Get database path from config
    pub fn get_db_path() -> PathBuf {
        config::config().get_search_db_path()
    }
    
    /// 获取搜索数据库目录 / Get search database directory
    pub fn get_db_dir() -> PathBuf {
        config::config().get_search_db_dir()
    }
    
    /// 获取指定存储的数据库路径 / Get database path for specific driver
    pub fn get_driver_db_path(driver_id: &str) -> PathBuf {
        config::config().get_driver_search_db_path(driver_id)
    }
    
    /// 创建指定存储的搜索数据库（每个存储独立数据库）
    pub async fn new_for_driver(driver_id: &str) -> Result<Self, String> {
        let db_path = Self::get_driver_db_path(driver_id);
        
        // 确保目录存在
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        
        let db_url = format!("sqlite:{}?mode=rwc", db_path.to_string_lossy());
        
        let db = SqlitePoolOptions::new()
            .max_connections(2)  // 每个存储单独数据库，连接数可以少一些
            .connect(&db_url)
            .await
            .map_err(|e| e.to_string())?;
        
        // 启用WAL模式，提高并发性能
        sqlx::query("PRAGMA journal_mode=WAL")
            .execute(&db)
            .await
            .map_err(|e| e.to_string())?;
        
        // 设置busy_timeout，避免锁超时
        sqlx::query("PRAGMA busy_timeout=10000")
            .execute(&db)
            .await
            .map_err(|e| e.to_string())?;
        
        // 优化写入性能
        sqlx::query("PRAGMA synchronous=NORMAL")
            .execute(&db)
            .await
            .map_err(|e| e.to_string())?;
        
        tracing::info!("Driver search database created: {:?} (WAL mode)", db_path);
        
        Ok(Self { db, driver_id: Some(driver_id.to_string()) })
    }
    
    /// 删除指定存储的数据库文件
    pub fn delete_driver_db(driver_id: &str) {
        let db_path = Self::get_driver_db_path(driver_id);
        let db_shm = db_path.with_extension("db-shm");
        let db_wal = db_path.with_extension("db-wal");
        
        std::fs::remove_file(&db_path).ok();
        std::fs::remove_file(&db_shm).ok();
        std::fs::remove_file(&db_wal).ok();
        
        tracing::info!("Driver search database deleted: {:?}", db_path);
    }
    
    /// 检查指定存储的数据库是否存在
    pub fn driver_db_exists(driver_id: &str) -> bool {
        Self::get_driver_db_path(driver_id).exists()
    }
    
    /// 创建独立的搜索数据库（使用WAL模式）- 兼容旧接口
    pub async fn new_standalone() -> Result<Self, String> {
        let db_path = Self::get_db_path();
        
        // 确保目录存在
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        
        let db_url = format!("sqlite:{}?mode=rwc", db_path.to_string_lossy());
        
        let db = SqlitePoolOptions::new()
            .max_connections(4)
            .connect(&db_url)
            .await
            .map_err(|e| e.to_string())?;
        
        // 启用WAL模式，提高并发性能
        sqlx::query("PRAGMA journal_mode=WAL")
            .execute(&db)
            .await
            .map_err(|e| e.to_string())?;
        
        // 设置busy_timeout，避免锁超时
        sqlx::query("PRAGMA busy_timeout=5000")
            .execute(&db)
            .await
            .map_err(|e| e.to_string())?;
        
        // 优化写入性能
        sqlx::query("PRAGMA synchronous=NORMAL")
            .execute(&db)
            .await
            .map_err(|e| e.to_string())?;
        
        tracing::info!("Search database created: {:?} (WAL mode)", db_path);
        
        Ok(Self { db, driver_id: None })
    }
    
    /// 使用现有数据库连接池
    pub fn new(db: Pool<Sqlite>) -> Self {
        Self { db, driver_id: None }
    }

    /// 初始化表结构（路径前缀压缩版本）
    /// 只在表不存在时创建，不删除已有数据
    pub async fn init(&self) -> Result<(), String> {
        // 目录表：存储目录路径（路径前缀压缩）
        sqlx::query(r#"
            CREATE TABLE IF NOT EXISTS search_dirs (
                id INTEGER PRIMARY KEY,
                parent_id INTEGER,
                name TEXT NOT NULL,
                UNIQUE(parent_id, name)
            )
        "#)
        .execute(&self.db)
        .await
        .map_err(|e| e.to_string())?;
        
        // 文件表：只存dir_id + 文件名
        sqlx::query(r#"
            CREATE TABLE IF NOT EXISTS search_files (
                dir_id INTEGER NOT NULL,
                name TEXT NOT NULL,
                name_lower TEXT NOT NULL,
                is_dir INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY(dir_id, name)
            ) WITHOUT ROWID
        "#)
        .execute(&self.db)
        .await
        .map_err(|e| e.to_string())?;

        // 索引：name_lower用于LIKE搜索
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_files_name ON search_files(name_lower)")
            .execute(&self.db)
            .await
            .map_err(|e| e.to_string())?;

        // 元数据表：存储索引更新时间等信息
        sqlx::query(r#"
            CREATE TABLE IF NOT EXISTS search_meta (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            )
        "#)
        .execute(&self.db)
        .await
        .map_err(|e| e.to_string())?;

        Ok(())
    }
    
    /// 重置表结构（清空重建时调用）
    pub async fn reset_tables(&self) -> Result<(), String> {
        sqlx::query("DROP TABLE IF EXISTS search_files").execute(&self.db).await.ok();
        sqlx::query("DROP TABLE IF EXISTS search_dirs").execute(&self.db).await.ok();
        sqlx::query("DROP TABLE IF EXISTS search_index").execute(&self.db).await.ok();
        sqlx::query("DROP TABLE IF EXISTS search_meta").execute(&self.db).await.ok();
        self.init().await
    }

    /// 设置索引更新时间 / Set index last updated time
    pub async fn set_last_updated(&self) -> Result<(), String> {
        let now = chrono::Utc::now().timestamp();
        sqlx::query(
            "INSERT OR REPLACE INTO search_meta (key, value) VALUES ('last_updated', ?)"
        )
        .bind(now.to_string())
        .execute(&self.db)
        .await
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// 获取索引更新时间 / Get index last updated time
    pub async fn get_last_updated(&self) -> Option<i64> {
        let result: Option<(String,)> = sqlx::query_as(
            "SELECT value FROM search_meta WHERE key = 'last_updated'"
        )
        .fetch_optional(&self.db)
        .await
        .ok()
        .flatten();
        
        result.and_then(|(v,)| v.parse::<i64>().ok())
    }

    /// 清空索引（清空表数据）
    pub async fn clear(&self) -> Result<(), String> {
        // 清空所有表数据
        sqlx::query("DELETE FROM search_files").execute(&self.db).await.ok();
        sqlx::query("DELETE FROM search_dirs").execute(&self.db).await.ok();
        sqlx::query("DELETE FROM search_meta").execute(&self.db).await.ok();
        
        // VACUUM压缩
        sqlx::query("VACUUM").execute(&self.db).await.ok();
        
        Ok(())
    }
    
    /// 删除数据库文件（彻底清除）
    pub fn delete_db_files() {
        let db_path = Self::get_db_path();
        let db_shm = db_path.with_extension("db-shm");
        let db_wal = db_path.with_extension("db-wal");
        
        std::fs::remove_file(&db_path).ok();
        std::fs::remove_file(&db_shm).ok();
        std::fs::remove_file(&db_wal).ok();
        
        tracing::info!("Search database files deleted");
    }
    
    /// 删除所有存储的索引数据库文件
    pub fn delete_all_driver_db_files() {
        let search_dir = Self::get_db_dir();
        if let Ok(entries) = std::fs::read_dir(&search_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    if name.starts_with("index_") && name.ends_with(".db") {
                        std::fs::remove_file(&path).ok();
                        // 同时删除 WAL 和 SHM 文件
                        std::fs::remove_file(path.with_extension("db-shm")).ok();
                        std::fs::remove_file(path.with_extension("db-wal")).ok();
                    }
                }
            }
        }
        tracing::info!("All driver search database files deleted");
    }
    
    /// 列出所有存储的索引数据库
    pub fn list_driver_dbs() -> Vec<String> {
        let search_dir = Self::get_db_dir();
        let mut drivers = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&search_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    if name.starts_with("index_") && name.ends_with(".db") {
                        // 提取 driver_id
                        let driver_id = name.trim_start_matches("index_").trim_end_matches(".db");
                        drivers.push(driver_id.to_string());
                    }
                }
            }
        }
        drivers
    }
    
    /// 获取数据库文件大小（兼容旧接口）
    pub fn get_db_size(&self) -> u64 {
        // 如果是特定存储的数据库，返回该存储的大小
        if let Some(ref driver_id) = self.driver_id {
            return Self::get_driver_db_size(driver_id);
        }
        // 否则返回所有存储的总大小
        Self::get_all_driver_db_size()
    }
    
    /// 获取指定存储的数据库文件大小
    pub fn get_driver_db_size(driver_id: &str) -> u64 {
        let db_path = Self::get_driver_db_path(driver_id);
        let db_shm = db_path.with_extension("db-shm");
        let db_wal = db_path.with_extension("db-wal");
        
        let size1 = std::fs::metadata(&db_path).map(|m| m.len()).unwrap_or(0);
        let size2 = std::fs::metadata(&db_shm).map(|m| m.len()).unwrap_or(0);
        let size3 = std::fs::metadata(&db_wal).map(|m| m.len()).unwrap_or(0);
        
        size1 + size2 + size3
    }
    
    /// 获取所有存储的数据库文件总大小
    pub fn get_all_driver_db_size() -> u64 {
        let search_dir = Self::get_db_dir();
        let mut total_size: u64 = 0;
        
        if let Ok(entries) = std::fs::read_dir(&search_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    // 统计所有 index_*.db 及其关联文件
                    if name.starts_with("index_") && (name.ends_with(".db") || name.ends_with(".db-shm") || name.ends_with(".db-wal")) {
                        total_size += std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                    }
                }
            }
        }
        
        total_size
    }
    
    /// 获取或创建目录ID
    async fn get_or_create_dir(&self, tx: &mut sqlx::Transaction<'_, Sqlite>, path: &str) -> Result<i64, String> {
        if path == "/" || path.is_empty() {
            return Ok(0); // 根目录ID为0
        }
        
        // 解析父目录和目录名
        let path = path.trim_end_matches('/');
        let (parent_path, dir_name) = match path.rfind('/') {
            Some(pos) if pos > 0 => (&path[..pos], &path[pos+1..]),
            Some(_) => ("/", &path[1..]),
            None => ("/", path),
        };
        
        // 递归获取父目录ID
        let parent_id = Box::pin(self.get_or_create_dir(tx, parent_path)).await?;
        
        // 查找现有目录
        let existing: Option<(i64,)> = sqlx::query_as(
            "SELECT id FROM search_dirs WHERE parent_id = ? AND name = ?"
        )
        .bind(parent_id)
        .bind(dir_name)
        .fetch_optional(&mut **tx)
        .await
        .map_err(|e| e.to_string())?;
        
        if let Some((id,)) = existing {
            return Ok(id);
        }
        
        // 插入新目录
        let result = sqlx::query(
            "INSERT INTO search_dirs (parent_id, name) VALUES (?, ?)"
        )
        .bind(parent_id)
        .bind(dir_name)
        .execute(&mut **tx)
        .await
        .map_err(|e| e.to_string())?;
        
        Ok(result.last_insert_rowid())
    }

    /// 批量插入索引（路径前缀压缩版本）- 带重试机制
    pub async fn insert_batch(&self, entries: &[(String, String, bool, i64, i64)]) -> Result<(), String> {
        if entries.is_empty() {
            return Ok(());
        }

        // 重试机制：最多重试3次
        let max_retries = 3;
        let mut last_error = String::new();
        
        for attempt in 0..max_retries {
            match self.do_insert_batch(entries).await {
                Ok(()) => return Ok(()),
                Err(e) => {
                    last_error = e.clone();
                    if e.contains("database is locked") || e.contains("SQLITE_BUSY") {
                        // 数据库锁定，等待后重试
                        let delay = 100 * (attempt + 1) as u64;
                        tracing::debug!("Database locked, retrying in {}ms (attempt {}/{})", delay, attempt + 1, max_retries);
                        tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
                        continue;
                    }
                    // 其他错误直接返回
                    return Err(e);
                }
            }
        }
        
        Err(format!("Batch insert failed after {} retries: {}", max_retries, last_error))
    }
    
    /// 实际执行批量插入
    async fn do_insert_batch(&self, entries: &[(String, String, bool, i64, i64)]) -> Result<(), String> {
        let mut tx = self.db.begin().await.map_err(|e| e.to_string())?;
        
        // 缓存目录ID，减少重复查询
        let mut dir_cache: HashMap<String, i64> = HashMap::new();
        dir_cache.insert("/".to_string(), 0);

        for (path, name, is_dir, _size, _modified) in entries {
            // 获取父目录路径
            let parent_path = match path.rfind('/') {
                Some(pos) if pos > 0 => &path[..pos],
                _ => "/",
            };
            
            // 获取目录ID（优先从缓存）
            let dir_id = if let Some(&id) = dir_cache.get(parent_path) {
                id
            } else {
                let id = self.get_or_create_dir(&mut tx, parent_path).await?;
                dir_cache.insert(parent_path.to_string(), id);
                id
            };
            
            let name_lower = name.to_lowercase();

            sqlx::query(
                "INSERT OR REPLACE INTO search_files (dir_id, name, name_lower, is_dir) VALUES (?, ?, ?, ?)"
            )
            .bind(dir_id)
            .bind(name)
            .bind(&name_lower)
            .bind(if *is_dir { 1 } else { 0 })
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;
        }

        tx.commit().await.map_err(|e| e.to_string())?;
        Ok(())
    }

    /// 重建目录路径
    async fn build_path(&self, dir_id: i64) -> String {
        if dir_id == 0 {
            return "/".to_string();
        }
        
        let mut parts: Vec<String> = Vec::new();
        let mut current_id = dir_id;
        
        while current_id > 0 {
            let row: Option<(i64, String)> = sqlx::query_as(
                "SELECT parent_id, name FROM search_dirs WHERE id = ?"
            )
            .bind(current_id)
            .fetch_optional(&self.db)
            .await
            .ok()
            .flatten();
            
            match row {
                Some((parent_id, name)) => {
                    parts.push(name);
                    current_id = parent_id;
                }
                None => break,
            }
        }
        
        parts.reverse();
        format!("/{}", parts.join("/"))
    }

    /// 搜索（LIKE查询 + 路径重建）
    pub async fn search(&self, query: &str, limit: usize) -> Result<(Vec<SearchHit>, usize), String> {
        let query_lower = query.to_lowercase();
        let query_simplified = to_simplified(&query_lower);
        let like_pattern = format!("%{}%", query_lower);

        // 查询结果，按匹配度排序
        let rows = sqlx::query(
            r#"
            SELECT dir_id, name, is_dir, name_lower,
                CASE 
                    WHEN name_lower = ? THEN 100
                    WHEN name_lower LIKE ? THEN 80
                    WHEN name_lower LIKE ? THEN 60
                    ELSE 30
                END as score
            FROM search_files 
            WHERE name_lower LIKE ?
            ORDER BY score DESC, length(name) ASC
            LIMIT ?
            "#
        )
        .bind(&query_lower)
        .bind(format!("{}%", query_lower))
        .bind(&like_pattern)
        .bind(&like_pattern)
        .bind(limit as i64)
        .fetch_all(&self.db)
        .await
        .map_err(|e| e.to_string())?;

        let mut results: Vec<SearchHit> = Vec::with_capacity(rows.len());
        
        // 缓存目录路径，减少重复查询
        let mut path_cache: HashMap<i64, String> = HashMap::new();
        
        for row in &rows {
            let dir_id: i64 = row.get("dir_id");
            let name: String = row.get("name");
            
            // 获取目录路径（优先从缓存）
            let dir_path = if let Some(p) = path_cache.get(&dir_id) {
                p.clone()
            } else {
                let p = self.build_path(dir_id).await;
                path_cache.insert(dir_id, p.clone());
                p
            };
            
            let full_path = if dir_path == "/" {
                format!("/{}", name)
            } else {
                format!("{}/{}", dir_path, name)
            };
            
            results.push(SearchHit {
                path: full_path,
                name,
                is_dir: row.get::<i32, _>("is_dir") == 1,
                size: 0,
                modified: 0,
                score: row.get::<i32, _>("score") as f32,
            });
        }

        // 简繁匹配补充
        if results.len() < limit && query_simplified != query_lower {
            let like_simplified = format!("%{}%", query_simplified);
            let extra_rows = sqlx::query(
                r#"
                SELECT dir_id, name, is_dir,
                    CASE 
                        WHEN name_lower = ? THEN 95
                        WHEN name_lower LIKE ? THEN 75
                        ELSE 25
                    END as score
                FROM search_files 
                WHERE name_lower LIKE ?
                ORDER BY score DESC, length(name) ASC
                LIMIT ?
                "#
            )
            .bind(&query_simplified)
            .bind(format!("{}%", query_simplified))
            .bind(&like_simplified)
            .bind((limit - results.len()) as i64)
            .fetch_all(&self.db)
            .await
            .unwrap_or_default();

            for row in extra_rows {
                let dir_id: i64 = row.get("dir_id");
                let name: String = row.get("name");
                
                let dir_path = if let Some(p) = path_cache.get(&dir_id) {
                    p.clone()
                } else {
                    let p = self.build_path(dir_id).await;
                    path_cache.insert(dir_id, p.clone());
                    p
                };
                
                let full_path = if dir_path == "/" {
                    format!("/{}", name)
                } else {
                    format!("{}/{}", dir_path, name)
                };
                
                // 避免重复
                if !results.iter().any(|r| r.path == full_path) {
                    results.push(SearchHit {
                        path: full_path,
                        name,
                        is_dir: row.get::<i32, _>("is_dir") == 1,
                        size: 0,
                        modified: 0,
                        score: row.get::<i32, _>("score") as f32,
                    });
                }
            }
        }

        let total = results.len();
        Ok((results, total))
    }

    /// 删除指定路径的索引（暂不实现，清空重建即可）
    pub async fn delete_by_path(&self, _path_prefix: &str) -> Result<u64, String> {
        // 路径前缀压缩后删除比较复杂，暂时不实现
        Ok(0)
    }

    /// 获取统计信息
    pub async fn get_stats(&self) -> IndexStats {
        let row = sqlx::query(
            "SELECT COUNT(*) as total, SUM(CASE WHEN is_dir = 1 THEN 1 ELSE 0 END) as dirs FROM search_files"
        )
        .fetch_one(&self.db)
        .await;

        // 从元数据表获取实际的更新时间
        let last_updated = self.get_last_updated().await;

        match row {
            Ok(r) => {
                let total: i64 = r.get("total");
                let dirs: i64 = r.try_get("dirs").unwrap_or(0);
                IndexStats {
                    file_count: (total - dirs) as u64,
                    dir_count: dirs as u64,
                    total_size: 0,
                    last_updated,
                }
            }
            Err(_) => IndexStats::default(),
        }
    }

    /// 索引是否存在
    pub async fn exists(&self) -> bool {
        let row = sqlx::query("SELECT COUNT(*) as cnt FROM search_files")
            .fetch_one(&self.db)
            .await;
        match row {
            Ok(r) => {
                let cnt: i64 = r.get("cnt");
                cnt > 0
            }
            Err(_) => false,
        }
    }
}
