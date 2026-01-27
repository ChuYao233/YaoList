use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;
use tower_cookies::Cookies;

use crate::state::AppState;
use crate::auth::SESSION_COOKIE_NAME;

/// 验证管理员权限
async fn require_admin(state: &AppState, cookies: &Cookies) -> Result<(), (StatusCode, Json<Value>)> {
    let session_id = cookies.get(SESSION_COOKIE_NAME)
        .map(|c| c.value().to_string())
        .ok_or_else(|| (StatusCode::UNAUTHORIZED, Json(json!({"error": "未登录"}))))?;
    
    let is_admin: Option<bool> = sqlx::query_scalar(
        "SELECT u.is_admin FROM users u 
         JOIN sessions s ON u.id = s.user_id 
         WHERE s.id = ? AND s.expires_at > datetime('now') AND u.enabled = 1"
    )
    .bind(&session_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "服务器错误"}))))?;
    
    if !is_admin.unwrap_or(false) {
        return Err((StatusCode::FORBIDDEN, Json(json!({"error": "需要管理员权限"}))));
    }
    Ok(())
}

/// 保存驱动更新后的配置到数据库 / Save updated driver config to database
async fn save_driver_config(db: &sqlx::SqlitePool, id: &str, updated_config: serde_json::Value) -> Result<(), String> {
    // 获取当前配置 / Get current config
    let current: Option<(String,)> = sqlx::query_as("SELECT config FROM drivers WHERE name = ?")
        .bind(id)
        .fetch_optional(db)
        .await
        .map_err(|e| e.to_string())?;
    
    if let Some((config_str,)) = current {
        // 解析当前配置 / Parse current config
        let mut config: serde_json::Value = serde_json::from_str(&config_str)
            .map_err(|e| e.to_string())?;
        
        // 更新config字段 / Update config field
        if let Some(obj) = config.as_object_mut() {
            obj.insert("config".to_string(), updated_config);
        }
        
        // 保存回数据库 / Save back to database
        let now = Utc::now().to_rfc3339();
        sqlx::query("UPDATE drivers SET config = ?, updated_at = ? WHERE name = ?")
            .bind(serde_json::to_string(&config).map_err(|e| e.to_string())?)
            .bind(&now)
            .bind(id)
            .execute(db)
            .await
            .map_err(|e| e.to_string())?;
        
        tracing::info!("Driver config saved: {}", id);
    }
    
    Ok(())
}

/// 检查是否启用自动更新索引，如果是则触发索引重建
async fn trigger_index_update_if_enabled(state: &Arc<AppState>) {
    // 检查是否启用自动更新索引
    let auto_update = sqlx::query_as::<_, (bool, bool)>(
        "SELECT enabled, auto_update_index FROM search_settings WHERE id = 1"
    )
    .fetch_optional(&state.db)
    .await
    .ok()
    .flatten()
    .map(|(enabled, auto_update)| enabled && auto_update)
    .unwrap_or(false);

    if auto_update {
        tracing::info!("Driver changed, triggering index auto-update");
        // 在后台启动索引重建任务
        let state_clone = state.clone();
        tokio::spawn(async move {
            if let Err(e) = super::search::trigger_rebuild_index(state_clone).await {
                tracing::error!("Auto index update failed: {}", e);
            }
        });
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct CreateDriverRequest {
    pub driver_type: String,
    pub mount_path: Option<String>,
    pub order: Option<i32>,
    pub remark: Option<String>,
    pub config: Value,
}

pub async fn list_drivers(
    State(state): State<Arc<AppState>>,
    cookies: Cookies,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&state, &cookies).await?;
    // 从数据库获取驱动列表，包含完整配置信息
    let db_drivers: Vec<(String, String, String, bool, String)> = sqlx::query_as(
        "SELECT name, version, description, enabled, config FROM drivers"
    )
    .fetch_all(&state.db)
    .await
    .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "服务器错误"}))))?;
    
    // 获取所有驱动错误状态
    let driver_errors = state.storage_manager.get_all_driver_errors().await;
    
    let drivers: Vec<Value> = db_drivers.iter().map(|(name, version, description, enabled, config_str)| {
        let config: Value = serde_json::from_str(config_str).unwrap_or(json!({}));
        let mount_path = config.get("mount_path").and_then(|v| v.as_str()).unwrap_or("");
        let driver_type = config.get("driver_type").and_then(|v| v.as_str()).unwrap_or("unknown");
        
        // 获取该驱动的错误状态
        let error = driver_errors.get(name).cloned();
        let status = if error.is_some() {
            "error"
        } else if *enabled {
            "running"
        } else {
            "disabled"
        };
        
        tracing::debug!("驱动 {} 配置: {}", name, config_str);
        
        json!({
            "id": name,
            "name": mount_path,
            "driver_type": driver_type,
            "version": version,
            "description": description,
            "enabled": enabled,
            "config": config,
            "status": status,
            "error": error
        })
    }).collect();
    
    Ok(Json(json!({
        "drivers": drivers
    })))
}

pub async fn enable_driver(
    State(state): State<Arc<AppState>>,
    cookies: Cookies,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&state, &cookies).await?;
    // 更新数据库启用状态
    sqlx::query("UPDATE drivers SET enabled = 1 WHERE name = ?")
        .bind(&id)
        .execute(&state.db)
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "服务器错误"}))))?;
    
    // 从数据库获取配置并加载驱动（验证逻辑已封装在StorageManager中）
    let driver_config: Option<(String,)> = sqlx::query_as("SELECT config FROM drivers WHERE name = ?")
        .bind(&id)
        .fetch_optional(&state.db)
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "服务器错误"}))))?;
    
    let mut warning: Option<String> = None;
    if let Some((config_str,)) = driver_config {
        if let Ok(config) = serde_json::from_str::<Value>(&config_str) {
            if let Some(driver_type) = config.get("driver_type").and_then(|v| v.as_str()) {
                if let Some(driver_config) = config.get("config") {
                    if let Err(e) = state.storage_manager.create_driver(id.clone(), driver_type, driver_config.clone()).await {
                        warning = Some(e.to_string());
                    } else {
                        // 检查是否有验证错误
                        if let Some(error) = state.storage_manager.get_driver_error(&id).await {
                            warning = Some(error);
                        }
                    }
                }
            }
        }
    }
    
    if let Some(warn) = warning {
        Ok(Json(json!({
            "code": 200,
            "message": format!("存储 {} 已启用，但验证失败", id),
            "warning": warn
        })))
    } else {
        Ok(Json(json!({
            "code": 200,
            "message": format!("存储 {} 已启用", id)
        })))
    }
}

pub async fn disable_driver(
    State(state): State<Arc<AppState>>,
    cookies: Cookies,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&state, &cookies).await?;
    // 更新数据库禁用状态
    sqlx::query("UPDATE drivers SET enabled = 0 WHERE name = ?")
        .bind(&id)
        .execute(&state.db)
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "服务器错误"}))))?;
    
    // 卸载驱动实例并清除错误状态
    let _ = state.storage_manager.remove_driver(&id).await;
    state.storage_manager.clear_driver_error(&id).await;
    
    Ok(Json(json!({
        "code": 200,
        "message": format!("存储 {} 已禁用", id)
    })))
}

pub async fn delete_driver(
    State(state): State<Arc<AppState>>,
    cookies: Cookies,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&state, &cookies).await?;
    // 先卸载驱动实例
    let _ = state.storage_manager.remove_driver(&id).await;
    
    // 从数据库删除
    sqlx::query("DELETE FROM drivers WHERE name = ?")
        .bind(&id)
        .execute(&state.db)
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "服务器错误"}))))?;
    
    // 删除该存储的索引数据库
    yaolist_backend::search::DbIndex::delete_driver_db(&id);
    
    Ok(Json(json!({
        "message": format!("存储 {} 已删除", id)
    })))
}

pub async fn reload_driver(
    State(state): State<Arc<AppState>>,
    cookies: Cookies,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&state, &cookies).await?;
    // 先卸载驱动实例
    let _ = state.storage_manager.remove_driver(&id).await;
    
    // 从数据库获取配置并重新加载
    let driver_config: Option<(String, bool)> = sqlx::query_as("SELECT config, enabled FROM drivers WHERE name = ?")
        .bind(&id)
        .fetch_optional(&state.db)
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "服务器错误"}))))?;
    
    if let Some((config_str, enabled)) = driver_config {
        if !enabled {
            return Ok(Json(json!({
                "message": format!("存储 {} 未启用，跳过加载", id)
            })));
        }
        
        if let Ok(config) = serde_json::from_str::<Value>(&config_str) {
            if let Some(driver_type) = config.get("driver_type").and_then(|v| v.as_str()) {
                if let Some(driver_config) = config.get("config") {
                    state.storage_manager.create_driver(id.clone(), driver_type, driver_config.clone()).await
                        .map_err(|e| {
                            tracing::error!("Failed to reload driver: {}", e);
                            (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "重新加载驱动失败"})))
                        })?;
                }
            }
        }
    } else {
        return Ok(Json(json!({
            "code": 404,
            "message": format!("存储 {} 不存在", id)
        })));
    }
    
    Ok(Json(json!({
        "message": format!("存储 {} 已重新加载", id)
    })))
}

pub async fn list_available_drivers(
    State(state): State<Arc<AppState>>,
    cookies: Cookies,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&state, &cookies).await?;
    // 从工厂获取完整的驱动信息，并转换为前端期望的格式
    let factories = state.storage_manager.get_all_factories().await;
    
    let drivers: Vec<Value> = factories.iter().map(|factory| {
        let info = factory.driver_info();
        
        // 将 additional 配置项转换为 config_schema.properties 格式
        let mut properties = serde_json::Map::new();
        let mut required = Vec::new();
        
        for (index, item) in info.additional.iter().enumerate() {
            let mut prop = serde_json::Map::new();
            prop.insert("type".to_string(), json!(item.item_type));
            let display_title = item.title.as_ref().unwrap_or(&item.name);
            prop.insert("title".to_string(), json!(display_title));
            prop.insert("order".to_string(), json!(index)); // 保持驱动定义的顺序
            if let Some(ref help) = item.help {
                prop.insert("description".to_string(), json!(help));
            }
            if let Some(ref default) = item.default {
                prop.insert("default".to_string(), json!(default));
            }
            if let Some(ref options) = item.options {
                let opts: Vec<&str> = options.split(',').collect();
                prop.insert("enum".to_string(), json!(opts));
            }
            if let Some(ref link) = item.link {
                prop.insert("link".to_string(), json!(link));
            }
            if item.required {
                required.push(item.name.clone());
            }
            properties.insert(item.name.clone(), json!(prop));
        }
        
        json!({
            "driver_type": factory.driver_type(),
            "display_name": info.config.name,
            "description": format!("{} 存储驱动", info.config.name),
            "config_schema": {
                "type": "object",
                "properties": properties,
                "required": required
            },
            // 同时保留新格式供将来使用
            "common": info.common,
            "additional": info.additional,
            "config": info.config
        })
    }).collect();
    
    Ok(Json(json!({
        "drivers": drivers
    })))
}

pub async fn create_driver(
    State(state): State<Arc<AppState>>,
    cookies: Cookies,
    Json(req): Json<CreateDriverRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&state, &cookies).await?;
    let now = Utc::now().to_rfc3339();
    
    // 查询当前最大ID，生成新的数字ID
    let max_id: Option<(i64,)> = sqlx::query_as("SELECT COALESCE(MAX(CAST(name AS INTEGER)), 0) FROM drivers WHERE name GLOB '[0-9]*'")
        .fetch_optional(&state.db)
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "服务器错误"}))))?;
    let driver_id = (max_id.map(|r| r.0).unwrap_or(0) + 1).to_string();
    let display_name = req.mount_path.clone().unwrap_or_else(|| req.driver_type.clone());
    
    // 保存到数据库
    sqlx::query(
        "INSERT INTO drivers (name, version, description, enabled, config, created_at, updated_at) 
         VALUES (?, ?, ?, 1, ?, ?, ?)"
    )
    .bind(&driver_id)
    .bind("1.0.0")
    .bind(&display_name)
    .bind(serde_json::to_string(&json!({
        "driver_type": req.driver_type,
        "mount_path": req.mount_path,
        "order": req.order,
        "remark": req.remark,
        "config": req.config
    })).unwrap())
    .bind(&now)
    .bind(&now)
    .execute(&state.db)
    .await
    .map_err(|e| {
        tracing::error!("Failed to save driver to database: {}", e);
        (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "保存驱动失败"})))
    })?;
    
    // 创建驱动实例（使用唯一ID）
    if let Err(e) = state.storage_manager.create_driver(driver_id.clone(), &req.driver_type, req.config).await {
        let error_msg = e.to_string();
        tracing::error!("Failed to create driver: {}", error_msg);
        return Ok(Json(json!({
            "code": 500,
            "message": format!("驱动创建失败: {}", error_msg),
            "id": driver_id
        })));
    }
    
    // 验证驱动有效性：尝试list根目录
    let mut validation_error: Option<String> = None;
    if let Some(driver) = state.storage_manager.get_driver(&driver_id).await {
        match driver.list("/").await {
            Ok(_) => {
                tracing::info!("Driver verification successful: {}", driver_id);
                // 清除之前的错误
                state.storage_manager.clear_driver_error(&driver_id).await;
            }
            Err(e) => {
                let error_msg = e.to_string();
                tracing::warn!("Driver verification failed: {} - {}", driver_id, error_msg);
                state.storage_manager.set_driver_error(&driver_id, error_msg.clone()).await;
                validation_error = Some(error_msg);
            }
        }
    }
    
    // 触发自动更新索引
    trigger_index_update_if_enabled(&state).await;
    
    if let Some(error) = validation_error {
        Ok(Json(json!({
            "code": 200,
            "message": "驱动创建成功，但验证失败",
            "id": driver_id,
            "warning": format!("连接验证失败: {}", error)
        })))
    } else {
        Ok(Json(json!({
            "code": 200,
            "message": "驱动创建成功",
            "id": driver_id
        })))
    }
}

pub async fn update_driver(
    State(state): State<Arc<AppState>>,
    cookies: Cookies,
    Path(id): Path<String>,
    Json(req): Json<CreateDriverRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&state, &cookies).await?;
    let now = Utc::now().to_rfc3339();
    let display_name = req.mount_path.clone().unwrap_or_else(|| req.driver_type.clone());
    
    // 更新数据库
    let result = sqlx::query(
        "UPDATE drivers SET description = ?, config = ?, updated_at = ? WHERE name = ?"
    )
    .bind(&display_name)
    .bind(serde_json::to_string(&json!({
        "driver_type": req.driver_type,
        "mount_path": req.mount_path,
        "order": req.order,
        "remark": req.remark,
        "config": req.config
    })).unwrap())
    .bind(&now)
    .bind(&id)
    .execute(&state.db)
    .await
    .map_err(|e| {
        tracing::error!("Failed to update driver in database: {}", e);
        (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "更新驱动失败"})))
    })?;
    
    if result.rows_affected() == 0 {
        return Ok(Json(json!({
            "code": 404,
            "message": "驱动不存在"
        })));
    }
    
    // 卸载旧驱动实例
    let _ = state.storage_manager.remove_driver(&id).await;
    
    // 创建新驱动实例
    if let Err(e) = state.storage_manager.create_driver(id.clone(), &req.driver_type, req.config).await {
        let error_msg = e.to_string();
        tracing::error!("Failed to create driver: {}", error_msg);
        return Ok(Json(json!({
            "code": 500,
            "message": format!("驱动更新失败: {}", error_msg),
            "id": id
        })));
    }
    
    // 验证驱动有效性：尝试list根目录
    let mut validation_error: Option<String> = None;
    if let Some(driver) = state.storage_manager.get_driver(&id).await {
        match driver.list("/").await {
            Ok(_) => {
                tracing::info!("Driver verification successful: {}", id);
                // 清除之前的错误
                state.storage_manager.clear_driver_error(&id).await;
                
                // 检查并保存更新的配置（如刷新后的token）
                // Check and save updated config (like refreshed token)
                if let Some(updated_config) = driver.get_updated_config() {
                    if let Err(e) = save_driver_config(&state.db, &id, updated_config).await {
                        tracing::warn!("Failed to save updated driver config: {} - {}", id, e);
                    }
                }
            }
            Err(e) => {
                let error_msg = e.to_string();
                tracing::warn!("Driver verification failed: {} - {}", id, error_msg);
                state.storage_manager.set_driver_error(&id, error_msg.clone()).await;
                validation_error = Some(error_msg);
            }
        }
    }
    
    // 触发自动更新索引
    trigger_index_update_if_enabled(&state).await;
    
    if let Some(error) = validation_error {
        Ok(Json(json!({
            "code": 200,
            "message": "驱动更新成功，但验证失败",
            "id": id,
            "warning": format!("连接验证失败: {}", error)
        })))
    } else {
        Ok(Json(json!({
            "code": 200,
            "message": "驱动更新成功",
            "id": id
        })))
    }
}

/// GET /api/drivers/:id/space - 获取驱动空间信息
pub async fn get_driver_space(
    State(state): State<Arc<AppState>>,
    cookies: Cookies,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&state, &cookies).await?;
    
    // 先检查驱动配置是否存在（使用name字段）
    let driver_exists: Option<(String,)> = sqlx::query_as("SELECT name FROM drivers WHERE name = ?")
        .bind(&id)
        .fetch_optional(&state.db)
        .await
        .ok()
        .flatten();
    
    if driver_exists.is_none() {
        return Ok(Json(json!({
            "code": 404,
            "data": null,
            "message": "驱动不存在"
        })));
    }
    
    // 获取驱动实例
    let driver = match state.storage_manager.get_driver(&id).await {
        Some(d) => d,
        None => {
            // 驱动配置存在但未成功加载
            return Ok(Json(json!({
                "code": 503,
                "data": null,
                "message": "驱动未成功加载，可能连接失败"
            })));
        }
    };
    
    // 获取是否在前台显示
    let show_in_frontend = driver.show_space_in_frontend();
    
    // 调用驱动原语获取空间信息
    match driver.get_space_info().await {
        Ok(Some(info)) => {
            Ok(Json(json!({
                "code": 200,
                "data": {
                    "used": info.used,
                    "total": info.total,
                    "free": info.free,
                    "show_in_frontend": show_in_frontend
                }
            })))
        }
        Ok(None) => {
            Ok(Json(json!({
                "code": 200,
                "data": null,
                "message": "驱动不支持获取空间信息"
            })))
        }
        Err(e) => {
            tracing::error!("Failed to get driver space info: {}", e);
            Ok(Json(json!({
                "code": 500,
                "data": null,
                "message": format!("获取空间信息失败: {}", e)
            })))
        }
    }
}

/// POST /api/driver/thunder/send_sms - 迅雷发送短信验证码
pub async fn thunder_send_sms(
    State(state): State<Arc<AppState>>,
    cookies: Cookies,
    Json(payload): Json<Value>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&state, &cookies).await?;

    let username = payload.get("username")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let password = payload.get("password")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if username.is_empty() || password.is_empty() {
        return Ok(Json(json!({
            "code": 400,
            "message": "请先填写手机号和密码"
        })));
    }

    // 创建临时客户端发送验证码
    let input = format!("{}{}", username, password);
    let device_id = format!("{:x}", md5::compute(input.as_bytes()));
    let client = yaolist_backend::drivers::thunder::client::ThunderClient::new(
        device_id,
        String::new(),
        String::new(),
    );

    // 尝试登录，触发短信发送
    let result: Result<String, anyhow::Error> = client.core_login(username, password).await;
    match result {
        Ok(_) => {
            // 登录成功，不需要验证码
            Ok(Json(json!({
                "code": 200,
                "message": "登录成功，无需验证码"
            })))
        }
        Err(e) => {
            let msg: String = e.to_string();
            if msg.contains("验证码已发送") {
                Ok(Json(json!({
                    "code": 200,
                    "message": msg
                })))
            } else {
                Ok(Json(json!({
                    "code": 400,
                    "message": msg
                })))
            }
        }
    }
}
