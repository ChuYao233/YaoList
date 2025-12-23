use axum::{
    extract::State,
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;

use crate::state::AppState;
use crate::api::ApiResponse;

/// GET /api/health - 健康检查
pub async fn health_check() -> Json<Value> {
    Json(json!({
        "status": "ok",
        "message": "YaoList 服务运行正常"
    }))
}

/// WebDAV服务器状态
#[derive(Debug, Clone, Serialize)]
pub struct ServerStatus {
    pub webdav_enabled: bool,
    pub webdav_listen: String,
}

/// 获取服务器状态
pub async fn get_server_status(
    State(state): State<Arc<AppState>>,
) -> Json<ApiResponse<ServerStatus>> {
    let webdav_config = state.webdav_config.read().await;
    
    Json(ApiResponse::success(ServerStatus {
        webdav_enabled: webdav_config.enabled,
        webdav_listen: webdav_config.listen.clone(),
    }))
}

/// WebDAV配置请求
#[derive(Debug, Clone, Deserialize)]
pub struct UpdateWebDavConfigRequest {
    pub enabled: Option<bool>,
    pub listen: Option<String>,
    pub prefix: Option<String>,
}

/// 更新WebDAV配置
pub async fn update_webdav_config(
    State(state): State<Arc<AppState>>,
    Json(req): Json<UpdateWebDavConfigRequest>,
) -> Json<ApiResponse<()>> {
    let mut config = state.webdav_config.write().await;
    
    if let Some(enabled) = req.enabled {
        config.enabled = enabled;
    }
    if let Some(listen) = req.listen {
        config.listen = listen;
    }
    if let Some(prefix) = req.prefix {
        config.prefix = prefix;
    }
    
    // TODO: 保存配置到数据库
    // TODO: 重启WebDAV服务器
    
    Json(ApiResponse::success(()))
}

/// 获取WebDAV配置
pub async fn get_webdav_config(
    State(state): State<Arc<AppState>>,
) -> Json<ApiResponse<yaolist_backend::server::WebDavConfig>> {
    let config = state.webdav_config.read().await;
    Json(ApiResponse::success(config.clone()))
}
