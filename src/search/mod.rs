//! Search module - only provides search capabilities (primitives), does not control flow / 搜索模块
//! 
//! Architecture principles / 架构原则：
//! - Search module only exposes primitive operations: index_document, search, delete_document
//! - Core controls scanning tasks, progress, concurrency, error recovery
//! - Call direction: Core → Search (unidirectional) / 调用方向
//!
//! Index features / 索引特性：
//! - Database index: uses SQLite storage, LIKE queries + index acceleration (recommended)
//! - File index: backup solution, streaming read/write
//! - Supports multilingual search (Chinese, Japanese, Korean, English, etc.)
//! - Supports simplified/traditional matching

pub mod engine;
pub mod schema;
pub mod tokenizer;
pub mod db_index;

pub use engine::SearchEngine;
pub use schema::{FileDocument, SearchResult};
pub use db_index::{DbIndex, SearchHit, IndexStats};

/// Search capability declaration / 搜索能力声明
pub struct SearchCapability {
    pub supports_chinese: bool,
    pub supports_fuzzy: bool,
    pub supports_phrase: bool,
    pub max_results: usize,
}

impl Default for SearchCapability {
    fn default() -> Self {
        Self {
            supports_chinese: true,
            supports_fuzzy: true,
            supports_phrase: true,
            max_results: 1000,
        }
    }
}
