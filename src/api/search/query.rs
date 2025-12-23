use serde::{Deserialize, Serialize};
use axum::{
    extract::State,
    Json,
};
use std::sync::Arc;
use tower_cookies::Cookies;

use crate::state::AppState;
use crate::models::Meta;
use yaolist_backend::utils::should_hide_file;
use super::types::*;
use super::admin::ApiResponse;


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

#[derive(Debug, Serialize)]
pub struct SearchResultItem {
    pub path: String,
    pub name: String,
    pub is_dir: bool,
    pub size: i64,
    pub modified: i64,
}

#[derive(Debug, Serialize)]
pub struct SearchResponse {
    pub results: Vec<SearchResultItem>,
    pub total: usize,
    pub total_matched: usize, // 匹配的总数（过滤前）
}

pub async fn search(
    State(state): State<Arc<AppState>>,
    cookies: Cookies,
    Json(req): Json<SearchRequest>,
) -> Json<ApiResponse<SearchResponse>> {
    // 检查搜索是否启用
    let enabled = sqlx::query_as::<_, (bool,)>(
        "SELECT enabled FROM search_settings WHERE id = 1"
    )
    .fetch_optional(&state.db)
    .await
    .ok()
    .flatten()
    .map(|(e,)| e)
    .unwrap_or(false);

    if !enabled {
        return Json(ApiResponse::error("搜索功能未启用"));
    }

    // 检查是否有任何存储的索引数据库
    let driver_dbs = yaolist_backend::search::DbIndex::list_driver_dbs();
    if driver_dbs.is_empty() {
        return Json(ApiResponse::error("索引未构建"));
    }

    let query = req.query.trim();
    if query.is_empty() {
        return Json(ApiResponse::error("搜索关键词不能为空"));
    }

    // 获取用户上下文（权限+根路径）- 直接使用files模块的函数确保一致性
    let user_ctx = crate::api::files::get_user_context(&state, &cookies).await;
    let user_root = yaolist_backend::utils::fix_and_clean_path(&user_ctx.root_path);
    let perms = SearchUserPermissions { show_hidden_files: user_ctx.permissions.show_hidden_files };
    
    tracing::debug!("搜索：用户根路径={}", user_root);
    
    // 获取所有元信息的隐藏规则
    let metas: Vec<Meta> = sqlx::query_as(
        "SELECT id, path, password, p_sub, write, w_sub, hide, h_sub, readme, r_sub, header, header_sub, created_at, updated_at FROM metas"
    )
    .fetch_all(&state.db)
    .await
    .unwrap_or_default();

    // 获取当前路径（用于优先排序）
    let current_path = req.current_path.unwrap_or_else(|| "/".to_string());
    
    // 搜索所有存储的索引数据库并合并结果
    let search_limit = std::cmp::min(10000, req.limit * req.page * 3);
    let mut all_hits: Vec<yaolist_backend::search::SearchHit> = Vec::new();
    
    for driver_id in &driver_dbs {
        // 为每个存储打开数据库
        let db_index = match yaolist_backend::search::DbIndex::new_for_driver(driver_id).await {
            Ok(idx) => idx,
            Err(e) => {
                tracing::warn!("Failed to open search db for driver {}: {}", driver_id, e);
                continue;
            }
        };
        
        // 搜索该存储的索引
        match db_index.search(query, search_limit).await {
            Ok((hits, _)) => {
                all_hits.extend(hits);
            }
            Err(e) => {
                tracing::warn!("Search failed for driver {}: {}", driver_id, e);
            }
        }
        
        // 关闭数据库连接
        db_index.close().await;
    }
    
    // 按分数排序
    all_hits.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    
    // 限制总结果数
    if all_hits.len() > search_limit {
        all_hits.truncate(search_limit);
    }
    
    // 过滤：用户根路径 + 隐藏文件
    let mut filtered: Vec<_> = all_hits.iter()
        .filter(|h| {
            // 首先检查是否在用户根路径下
            // 用户根路径为"/"时允许所有路径，否则路径必须以用户根路径开头
            if user_root != "/" {
                let user_root_with_slash = if user_root.ends_with('/') {
                    user_root.clone()
                } else {
                    format!("{}/", user_root)
                };
                if !h.path.starts_with(&user_root_with_slash) && h.path != user_root {
                    return false;
                }
            }
            
            // 有 show_hidden_files 权限的用户可以看到所有文件
            if perms.show_hidden_files {
                return true;
            }
            
            // 检查是否被任何元信息规则隐藏
            let filename = h.path.split('/').last().unwrap_or(&h.name);
            for meta in &metas {
                // 检查路径是否在元信息范围内
                if h.path.starts_with(&meta.path) || h.path == meta.path {
                    let hide_str: &str = meta.hide.as_deref().unwrap_or("");
                    if !hide_str.is_empty() {
                        // 检查是否应用子目录规则
                        let is_sub = h.path.len() > meta.path.len() && 
                            h.path[meta.path.len()..].contains('/');
                        if !is_sub || meta.h_sub {
                            if should_hide_file(filename, hide_str) {
                                return false;
                            }
                        }
                    }
                }
            }
            true
        })
        .collect();
    
    // 根据类型筛选
    if let Some(ref filter_type) = req.filter_type {
        filtered.retain(|h| {
            match filter_type.as_str() {
                "file" => !h.is_dir,
                "folder" => h.is_dir,
                _ => true,
            }
        });
    }
    
    // 优先显示当前路径下的结果
    filtered.sort_by(|a, b| {
        let a_in_current = a.path.starts_with(&current_path);
        let b_in_current = b.path.starts_with(&current_path);
        match (a_in_current, b_in_current) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal),
        }
    });
    
    // 分页
    let total_filtered = filtered.len();
    let skip = (req.page.saturating_sub(1)) * req.limit;
    
    let results: Vec<SearchResultItem> = filtered.into_iter()
        .skip(skip)
        .take(req.limit)
        .map(|h| {
            // 将绝对路径转换为相对于用户根路径的路径
            let display_path = if user_root != "/" && h.path.starts_with(&user_root) {
                let relative = &h.path[user_root.len()..];
                if relative.is_empty() {
                    "/".to_string()
                } else if relative.starts_with('/') {
                    relative.to_string()
                } else {
                    format!("/{}", relative)
                }
            } else {
                h.path.clone()
            };
            SearchResultItem {
                path: display_path,
                name: h.name.clone(),
                is_dir: h.is_dir,
                size: h.size,
                modified: h.modified,
            }
        })
        .collect();
    let total = results.len();
    Json(ApiResponse::success(SearchResponse { results, total, total_matched: total_filtered }))
}

