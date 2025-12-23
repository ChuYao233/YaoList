use serde::{Deserialize, Serialize};

/// 用户权限（仅搜索需要的字段）
#[derive(Debug, Clone, Default, sqlx::FromRow)]
pub struct SearchUserPermissions {
    pub show_hidden_files: bool,
}

/// 用户上下文（搜索用）
#[derive(Debug, Clone)]
pub struct SearchUserContext {
    pub permissions: SearchUserPermissions,
    pub root_path: String,
}

impl Default for SearchUserContext {
    fn default() -> Self {
        Self {
            permissions: SearchUserPermissions::default(),
            root_path: "/".to_string(),
        }
    }
}

/// API响应结构
#[derive(Debug, Serialize)]
pub struct ApiResponse<T> {
    pub success: bool,
    pub data: Option<T>,
    pub error: Option<String>,
}

impl<T> ApiResponse<T> {
    pub fn success(data: T) -> Self {
        Self {
            success: true,
            data: Some(data),
            error: None,
        }
    }

    pub fn error(message: &str) -> Self {
        Self {
            success: false,
            data: None,
            error: Some(message.to_string()),
        }
    }
}

/// 索引状态
#[derive(Debug, Serialize)]
pub struct IndexStatus {
    pub status: String,
    pub object_count: u64,
    pub index_size: u64,
    pub last_updated: Option<String>,
    pub error_message: Option<String>,
}

/// 搜索设置
#[derive(Debug, Serialize, Deserialize)]
pub struct SearchSettings {
    pub enabled: bool,
    pub max_results: i64,
    pub search_content: bool,
    pub index_hidden: bool,
}

/// 搜索请求
#[derive(Debug, Deserialize)]
pub struct SearchRequest {
    pub query: String,
    #[serde(default = "default_limit")]
    pub limit: usize,
    #[serde(default = "default_page")]
    pub page: usize,
    #[serde(default)]
    pub current_path: Option<String>,
    #[serde(default)]
    pub filter_type: Option<String>, // "file" or "folder"
}

fn default_limit() -> usize { 50 }
fn default_page() -> usize { 1 }

/// 搜索结果项
#[derive(Debug, Serialize)]
pub struct SearchResultItem {
    pub path: String,
    pub name: String,
    pub is_dir: bool,
    pub size: i64,
    pub modified: i64,
}

/// 搜索响应
#[derive(Debug, Serialize)]
pub struct SearchResponse {
    pub results: Vec<SearchResultItem>,
    pub total: usize,
    pub total_matched: usize,
}
