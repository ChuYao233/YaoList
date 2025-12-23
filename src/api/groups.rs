use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;
use chrono::Utc;
use tower_cookies::Cookies;

use crate::{
    models::UserGroup,
    state::AppState,
    auth::SESSION_COOKIE_NAME,
};

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

#[derive(Debug, Deserialize)]
pub struct ListGroupsQuery {
    #[serde(default = "default_page")]
    page: i64,
    #[serde(default = "default_page_size")]
    page_size: i64,
    search: Option<String>,
}

fn default_page() -> i64 { 1 }
fn default_page_size() -> i64 { 10 }

#[derive(Debug, Deserialize, Serialize)]
pub struct CreateGroupRequest {
    name: String,
    description: Option<String>,
    #[serde(default)]
    is_admin: bool,
    #[serde(default)]
    allow_direct_link: bool,
    #[serde(default)]
    allow_share: bool,
    #[serde(default)]
    show_hidden_files: bool,
    #[serde(default)]
    no_password_access: bool,
    #[serde(default)]
    add_offline_download: bool,
    #[serde(default)]
    create_upload: bool,
    #[serde(default)]
    rename_files: bool,
    #[serde(default)]
    move_files: bool,
    #[serde(default)]
    copy_files: bool,
    #[serde(default)]
    delete_files: bool,
    #[serde(default = "default_true")]
    read_files: bool,
    #[serde(default)]
    read_compressed: bool,
    #[serde(default)]
    extract_files: bool,
    #[serde(default)]
    webdav_enabled: bool,
    #[serde(default)]
    ftp_enabled: bool,
    root_path: Option<String>,
}

fn default_true() -> bool { true }

#[derive(Debug, Deserialize, Serialize)]
pub struct UpdateGroupRequest {
    name: Option<String>,
    description: Option<String>,
    is_admin: Option<bool>,
    allow_direct_link: Option<bool>,
    allow_share: Option<bool>,
    show_hidden_files: Option<bool>,
    no_password_access: Option<bool>,
    add_offline_download: Option<bool>,
    create_upload: Option<bool>,
    rename_files: Option<bool>,
    move_files: Option<bool>,
    copy_files: Option<bool>,
    delete_files: Option<bool>,
    read_files: Option<bool>,
    read_compressed: Option<bool>,
    extract_files: Option<bool>,
    webdav_enabled: Option<bool>,
    ftp_enabled: Option<bool>,
    root_path: Option<String>,
}

pub async fn list_groups(
    State(state): State<Arc<AppState>>,
    cookies: Cookies,
    Query(query): Query<ListGroupsQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&state, &cookies).await?;
    let offset = (query.page - 1) * query.page_size;
    
    let mut sql = "SELECT * FROM user_groups".to_string();
    let mut count_sql = "SELECT COUNT(*) as count FROM user_groups".to_string();
    
    if let Some(search) = &query.search {
        let search_condition = format!(" WHERE name LIKE '%{}%' OR description LIKE '%{}%'", search, search);
        sql.push_str(&search_condition);
        count_sql.push_str(&search_condition);
    }
    
    sql.push_str(" ORDER BY created_at DESC LIMIT ? OFFSET ?");
    
    let groups = sqlx::query_as::<_, UserGroup>(&sql)
        .bind(query.page_size)
        .bind(offset)
        .fetch_all(&state.db)
        .await
        .map_err(|e| {
            eprintln!("Error fetching groups: {:?}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "获取用户组失败"})))
        })?;
    
    let total: (i64,) = sqlx::query_as(&count_sql)
        .fetch_one(&state.db)
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "服务器错误"}))))?;
    
    let total_pages = (total.0 + query.page_size - 1) / query.page_size;

    Ok(Json(json!({
        "groups": groups,
        "total": total.0,
        "page": query.page,
        "page_size": query.page_size,
        "total_pages": total_pages
    })))
}

pub async fn create_group(
    State(state): State<Arc<AppState>>,
    cookies: Cookies,
    Json(req): Json<CreateGroupRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&state, &cookies).await?;
    let now = Utc::now().to_rfc3339();

    let result = sqlx::query(
        "INSERT INTO user_groups (
            name, description, is_admin,
            allow_direct_link, allow_share, show_hidden_files, no_password_access,
            add_offline_download, create_upload, rename_files, move_files,
            copy_files, delete_files, read_files, read_compressed, extract_files,
            webdav_enabled, ftp_enabled, root_path, created_at, updated_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
    )
    .bind(&req.name)
    .bind(&req.description)
    .bind(req.is_admin)
    .bind(req.allow_direct_link)
    .bind(req.allow_share)
    .bind(req.show_hidden_files)
    .bind(req.no_password_access)
    .bind(req.add_offline_download)
    .bind(req.create_upload)
    .bind(req.rename_files)
    .bind(req.move_files)
    .bind(req.copy_files)
    .bind(req.delete_files)
    .bind(req.read_files)
    .bind(req.read_compressed)
    .bind(req.extract_files)
    .bind(req.webdav_enabled)
    .bind(req.ftp_enabled)
    .bind(&req.root_path)
    .bind(&now)
    .bind(&now)
    .execute(&state.db)
    .await
    .map_err(|e| {
        eprintln!("Error creating group: {:?}", e);
        (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "创建用户组失败"})))
    })?;

    Ok(Json(json!({
        "id": result.last_insert_rowid(),
        "message": "用户组创建成功"
    })))
}

pub async fn get_group(
    State(state): State<Arc<AppState>>,
    cookies: Cookies,
    Path(id): Path<i64>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&state, &cookies).await?;
    let group = sqlx::query_as::<_, UserGroup>("SELECT * FROM user_groups WHERE id = ?")
        .bind(id)
        .fetch_optional(&state.db)
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "服务器错误"}))))?
        .ok_or((StatusCode::NOT_FOUND, Json(json!({"error": "用户组不存在"}))))?;

    Ok(Json(json!({
        "group": group
    })))
}

pub async fn update_group(
    State(state): State<Arc<AppState>>,
    cookies: Cookies,
    Path(id): Path<i64>,
    Json(req): Json<UpdateGroupRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&state, &cookies).await?;
    let now = Utc::now().to_rfc3339();
    
    // 获取当前用户组信息
    let current = sqlx::query_as::<_, UserGroup>("SELECT * FROM user_groups WHERE id = ?")
        .bind(id)
        .fetch_optional(&state.db)
        .await
        .map_err(|_e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "服务器错误"}))))?
        .ok_or((StatusCode::NOT_FOUND, Json(json!({"error": "用户组不存在"}))))?;
    
    // 使用当前值或新值
    let name = req.name.unwrap_or(current.name);
    let description = req.description.or(current.description);
    let is_admin = req.is_admin.unwrap_or(current.is_admin);
    let allow_direct_link = req.allow_direct_link.unwrap_or(current.allow_direct_link);
    let allow_share = req.allow_share.unwrap_or(current.allow_share);
    let show_hidden_files = req.show_hidden_files.unwrap_or(current.show_hidden_files);
    let no_password_access = req.no_password_access.unwrap_or(current.no_password_access);
    let add_offline_download = req.add_offline_download.unwrap_or(current.add_offline_download);
    let create_upload = req.create_upload.unwrap_or(current.create_upload);
    let rename_files = req.rename_files.unwrap_or(current.rename_files);
    let move_files = req.move_files.unwrap_or(current.move_files);
    let copy_files = req.copy_files.unwrap_or(current.copy_files);
    let delete_files = req.delete_files.unwrap_or(current.delete_files);
    let read_files = req.read_files.unwrap_or(current.read_files);
    let read_compressed = req.read_compressed.unwrap_or(current.read_compressed);
    let extract_files = req.extract_files.unwrap_or(current.extract_files);
    let webdav_enabled = req.webdav_enabled.unwrap_or(current.webdav_enabled);
    let ftp_enabled = req.ftp_enabled.unwrap_or(current.ftp_enabled);
    let root_path = if req.root_path.is_some() { req.root_path } else { current.root_path };
    
    sqlx::query(
        "UPDATE user_groups SET 
            name = ?, description = ?, is_admin = ?,
            allow_direct_link = ?, allow_share = ?, show_hidden_files = ?, no_password_access = ?,
            add_offline_download = ?, create_upload = ?, rename_files = ?, move_files = ?,
            copy_files = ?, delete_files = ?, read_files = ?, read_compressed = ?, extract_files = ?,
            webdav_enabled = ?, ftp_enabled = ?, root_path = ?, updated_at = ?
         WHERE id = ?"
    )
    .bind(&name)
    .bind(&description)
    .bind(is_admin)
    .bind(allow_direct_link)
    .bind(allow_share)
    .bind(show_hidden_files)
    .bind(no_password_access)
    .bind(add_offline_download)
    .bind(create_upload)
    .bind(rename_files)
    .bind(move_files)
    .bind(copy_files)
    .bind(delete_files)
    .bind(read_files)
    .bind(read_compressed)
    .bind(extract_files)
    .bind(webdav_enabled)
    .bind(ftp_enabled)
    .bind(&root_path)
    .bind(&now)
    .bind(id)
    .execute(&state.db)
    .await
    .map_err(|e| {
        eprintln!("Error updating group: {:?}", e);
        (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "更新用户组失败"})))
    })?;

    Ok(Json(json!({
        "message": "用户组更新成功"
    })))
}

pub async fn delete_group(
    State(state): State<Arc<AppState>>,
    cookies: Cookies,
    Path(id): Path<i64>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&state, &cookies).await?;
    sqlx::query("DELETE FROM user_groups WHERE id = ?")
        .bind(id)
        .execute(&state.db)
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "服务器错误"}))))?;

    Ok(Json(json!({
        "message": "用户组删除成功"
    })))
}

pub async fn list_permissions(
    State(_state): State<Arc<AppState>>,
    _cookies: Cookies,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    // 返回空数组，因为我们不再使用权限表
    Ok(Json(json!({
        "permissions": []
    })))
}
