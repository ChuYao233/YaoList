//! 负载均衡API端点

use axum::{
    extract::State,
    Json,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tower_cookies::Cookies;

use crate::state::AppState;
use crate::auth::SESSION_COOKIE_NAME;
use yaolist_backend::models::UserPermissions;
use yaolist_backend::load_balance::{BalanceGroupConfig, LoadBalanceMode, BalanceDriver, DriverCapability};
use yaolist_backend::geoip::{lookup_ip, GeoInfo};

async fn check_admin(state: &AppState, cookies: &Cookies) -> bool {
    let session_id = match cookies.get(SESSION_COOKIE_NAME) {
        Some(c) => c.value().to_string(),
        None => return false,
    };
    
    let perms = sqlx::query_as::<_, UserPermissions>(
        r#"SELECT 
            MAX(g.read_files) as read_files, MAX(g.create_upload) as create_upload,
            MAX(g.rename_files) as rename_files, MAX(g.move_files) as move_files,
            MAX(g.copy_files) as copy_files, MAX(g.delete_files) as delete_files,
            MAX(g.allow_direct_link) as allow_direct_link, MAX(g.allow_share) as allow_share,
            MAX(g.is_admin) as is_admin, MAX(g.show_hidden_files) as show_hidden_files,
            MAX(g.extract_files) as extract_files
        FROM users u
        INNER JOIN sessions s ON u.id = s.user_id
        INNER JOIN user_group_members ugm ON u.id = ugm.user_id
        INNER JOIN user_groups g ON CAST(g.id AS TEXT) = ugm.group_id
        WHERE s.id = ? AND s.expires_at > datetime('now')
        GROUP BY u.id"#
    )
    .bind(&session_id)
    .fetch_optional(&state.db)
    .await
    .ok()
    .flatten();
    
    perms.map(|p| p.is_admin).unwrap_or(false)
}

#[derive(Serialize)]
pub struct ApiResponse<T> {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
}

impl<T> ApiResponse<T> {
    pub fn success(data: T) -> Self {
        Self { code: 200, message: "success".to_string(), data: Some(data) }
    }
    pub fn error(msg: &str) -> Self {
        Self { code: 400, message: msg.to_string(), data: None }
    }
}

#[derive(Deserialize)]
pub struct CreateGroupRequest {
    pub name: String,
    pub mode: String,
    pub drivers: Vec<DriverConfig>,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool { true }

#[derive(Deserialize, Serialize)]
pub struct DriverConfig {
    pub driver_id: String,
    pub driver_name: String,
    pub mount_path: String,
    #[serde(default = "default_weight")]
    pub weight: u32,
    #[serde(default)]
    pub order: i32,
    #[serde(default)]
    pub is_china_node: bool,
    #[serde(default)]
    pub can_redirect: bool,
}

fn default_weight() -> u32 { 1 }

#[derive(Serialize)]
pub struct GroupListResponse {
    pub groups: Vec<BalanceGroupConfig>,
}

#[derive(Serialize)]
pub struct GeoIpResponse {
    pub ip: String,
    pub info: GeoInfo,
}

/// 获取所有负载均衡组
pub async fn list_groups(
    State(state): State<Arc<AppState>>,
    cookies: Cookies,
) -> Json<ApiResponse<GroupListResponse>> {
    // 检查管理员权限
    if !check_admin(&state, &cookies).await {
        return Json(ApiResponse::error("需要管理员权限"));
    }
    
    let groups = state.load_balance.get_all_groups().await;
    Json(ApiResponse::success(GroupListResponse { groups }))
}

/// 创建负载均衡组
pub async fn create_group(
    State(state): State<Arc<AppState>>,
    cookies: Cookies,
    Json(req): Json<CreateGroupRequest>,
) -> Json<ApiResponse<()>> {
    if !check_admin(&state, &cookies).await {
        return Json(ApiResponse::error("需要管理员权限"));
    }
    
    if req.name.is_empty() {
        return Json(ApiResponse::error("组名不能为空"));
    }
    
    let mode = LoadBalanceMode::from(req.mode.as_str());
    let drivers: Vec<BalanceDriver> = req.drivers.into_iter().map(|d| BalanceDriver {
        driver_id: d.driver_id,
        driver_name: d.driver_name,
        mount_path: d.mount_path,
        weight: d.weight,
        capability: DriverCapability {
            can_redirect: d.can_redirect,
            can_range_read: true,
            can_direct_link: d.can_redirect,
        },
        order: d.order,
        is_china_node: d.is_china_node,
    }).collect();
    
    let config = BalanceGroupConfig {
        name: req.name.clone(),
        mode,
        drivers,
        enabled: req.enabled,
    };
    
    // 保存到数据库
    let config_json = serde_json::to_string(&config).unwrap_or_default();
    let now = chrono::Utc::now().to_rfc3339();
    
    let result = sqlx::query(
        "INSERT INTO load_balance_groups (name, config, enabled, created_at, updated_at) VALUES (?, ?, ?, ?, ?)"
    )
    .bind(&req.name)
    .bind(&config_json)
    .bind(req.enabled)
    .bind(&now)
    .bind(&now)
    .execute(&state.db)
    .await;
    
    if let Err(e) = result {
        return Json(ApiResponse::error(&format!("保存失败: {}", e)));
    }
    
    // 添加到内存
    state.load_balance.create_group(config).await;
    
    Json(ApiResponse::success(()))
}

/// 更新负载均衡组
pub async fn update_group(
    State(state): State<Arc<AppState>>,
    cookies: Cookies,
    Json(req): Json<CreateGroupRequest>,
) -> Json<ApiResponse<()>> {
    if !check_admin(&state, &cookies).await {
        return Json(ApiResponse::error("需要管理员权限"));
    }
    
    let mode = LoadBalanceMode::from(req.mode.as_str());
    let drivers: Vec<BalanceDriver> = req.drivers.into_iter().map(|d| BalanceDriver {
        driver_id: d.driver_id,
        driver_name: d.driver_name,
        mount_path: d.mount_path,
        weight: d.weight,
        capability: DriverCapability {
            can_redirect: d.can_redirect,
            can_range_read: true,
            can_direct_link: d.can_redirect,
        },
        order: d.order,
        is_china_node: d.is_china_node,
    }).collect();
    
    let config = BalanceGroupConfig {
        name: req.name.clone(),
        mode,
        drivers,
        enabled: req.enabled,
    };
    
    let config_json = serde_json::to_string(&config).unwrap_or_default();
    let now = chrono::Utc::now().to_rfc3339();
    
    let result = sqlx::query(
        "UPDATE load_balance_groups SET config = ?, enabled = ?, updated_at = ? WHERE name = ?"
    )
    .bind(&config_json)
    .bind(req.enabled)
    .bind(&now)
    .bind(&req.name)
    .execute(&state.db)
    .await;
    
    if let Err(e) = result {
        return Json(ApiResponse::error(&format!("更新失败: {}", e)));
    }
    
    state.load_balance.update_group(config).await;
    
    Json(ApiResponse::success(()))
}

#[derive(Deserialize)]
pub struct DeleteGroupRequest {
    pub name: String,
}

/// 删除负载均衡组
pub async fn delete_group(
    State(state): State<Arc<AppState>>,
    cookies: Cookies,
    Json(req): Json<DeleteGroupRequest>,
) -> Json<ApiResponse<()>> {
    if !check_admin(&state, &cookies).await {
        return Json(ApiResponse::error("需要管理员权限"));
    }
    
    let _ = sqlx::query("DELETE FROM load_balance_groups WHERE name = ?")
        .bind(&req.name)
        .execute(&state.db)
        .await;
    
    state.load_balance.delete_group(&req.name).await;
    
    Json(ApiResponse::success(()))
}

#[derive(Deserialize)]
pub struct LookupIpRequest {
    pub ip: String,
}

/// 查询IP地理信息
pub async fn lookup_ip_info(
    State(state): State<Arc<AppState>>,
    cookies: Cookies,
    Json(req): Json<LookupIpRequest>,
) -> Json<ApiResponse<GeoIpResponse>> {
    if !check_admin(&state, &cookies).await {
        return Json(ApiResponse::error("需要管理员权限"));
    }
    
    let ip: std::net::IpAddr = match req.ip.parse() {
        Ok(ip) => ip,
        Err(_) => return Json(ApiResponse::error("无效的IP地址")),
    };
    
    let info = lookup_ip(ip);
    
    Json(ApiResponse::success(GeoIpResponse {
        ip: req.ip,
        info,
    }))
}

/// 获取负载均衡模式列表
pub async fn list_modes() -> Json<ApiResponse<Vec<ModeInfo>>> {
    let modes = vec![
        ModeInfo { id: "weighted_round_robin".to_string(), name: "加权轮询".to_string(), description: "按权重比例轮询分配请求".to_string() },
        ModeInfo { id: "geo_region".to_string(), name: "地区分流".to_string(), description: "中国大陆/海外用户分流到不同驱动".to_string() },
    ];
    Json(ApiResponse::success(modes))
}

#[derive(Serialize)]
pub struct ModeInfo {
    pub id: String,
    pub name: String,
    pub description: String,
}
