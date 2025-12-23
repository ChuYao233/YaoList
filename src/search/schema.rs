//! Search index schema definition / 搜索索引的 Schema 定义

use serde::{Deserialize, Serialize};

/// File document - file information used for indexing / 文件文档
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileDocument {
    /// File unique identifier (driver_id + path combination) / 文件唯一标识
    pub id: String,
    /// Driver ID / 驱动ID
    pub driver_id: i64,
    /// File path / 文件路径
    pub path: String,
    /// File name / 文件名
    pub name: String,
    /// Parent directory path / 父目录路径
    pub parent: String,
    /// Whether it's a directory / 是否为目录
    pub is_dir: bool,
    /// File size (bytes) / 文件大小
    pub size: i64,
    /// Modification time (Unix timestamp) / 修改时间
    pub modified: i64,
}

impl FileDocument {
    /// Generate document ID / 生成文档ID
    pub fn generate_id(driver_id: i64, path: &str) -> String {
        format!("{}:{}", driver_id, path)
    }

    /// Extract filename from path / 从路径中提取文件名
    pub fn extract_name(path: &str) -> String {
        path.rsplit('/').next().unwrap_or(path).to_string()
    }

    /// Extract parent directory from path / 从路径中提取父目录
    pub fn extract_parent(path: &str) -> String {
        if let Some(pos) = path.rfind('/') {
            if pos == 0 {
                "/".to_string()
            } else {
                path[..pos].to_string()
            }
        } else {
            "/".to_string()
        }
    }
}

/// Search result / 搜索结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    /// File document / 文件文档
    pub document: FileDocument,
    /// Relevance score / 相关性分数
    pub score: f32,
    /// Highlight snippet (if any) / 高亮片段
    pub highlights: Vec<String>,
}

/// Search query options / 搜索查询选项
#[derive(Debug, Clone, Default)]
pub struct SearchOptions {
    /// Search keywords / 搜索关键词
    pub query: String,
    /// Limit search to specific driver ID (None means search all) / 限制搜索的驱动ID
    pub driver_id: Option<i64>,
    /// Limit search to specific path prefix / 限制搜索的路径前缀
    pub path_prefix: Option<String>,
    /// Whether to search directories only / 是否只搜索目录
    pub dirs_only: bool,
    /// Whether to search files only / 是否只搜索文件
    pub files_only: bool,
    /// Enable fuzzy search / 启用模糊搜索
    pub fuzzy: bool,
    /// Edit distance for fuzzy search (1-2) / 模糊搜索的编辑距离
    pub fuzzy_distance: u8,
    /// Maximum number of results to return / 最大返回结果数
    pub limit: usize,
    /// Offset (for pagination) / 偏移量
    pub offset: usize,
}

impl SearchOptions {
    pub fn new(query: impl Into<String>) -> Self {
        Self {
            query: query.into(),
            driver_id: None,
            path_prefix: None,
            dirs_only: false,
            files_only: false,
            fuzzy: true,
            fuzzy_distance: 1,
            limit: 50,
            offset: 0,
        }
    }

    pub fn with_driver(mut self, driver_id: i64) -> Self {
        self.driver_id = Some(driver_id);
        self
    }

    pub fn with_path_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.path_prefix = Some(prefix.into());
        self
    }

    pub fn with_limit(mut self, limit: usize) -> Self {
        self.limit = limit;
        self
    }

    pub fn with_offset(mut self, offset: usize) -> Self {
        self.offset = offset;
        self
    }

    pub fn fuzzy(mut self, enabled: bool) -> Self {
        self.fuzzy = enabled;
        self
    }
}
