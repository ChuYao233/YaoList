use axum::{
    routing::{get, post},
    Router,
    extract::DefaultBodyLimit,
    response::{Response, IntoResponse},
    http::{header, StatusCode, Uri},
    body::Body,
};
use sqlx::sqlite::SqlitePool;
use std::sync::Arc;
use tower_http::cors::CorsLayer;
use tower_cookies::CookieManagerLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use rust_embed::RustEmbed;

/// Embed frontend static files (compile-time embed from ../frontend/dist) / 嵌入前端静态文件
#[derive(RustEmbed)]
#[folder = "../frontend/dist"]
struct FrontendAssets;

mod api;
mod auth;
mod db;
mod state;
mod task;

use yaolist_backend::models;
use yaolist_backend::config;
use state::AppState;
use chrono::Utc;

/// 保存驱动更新后的配置到数据库 / Save updated driver config to database
async fn save_driver_config_to_db(db: &SqlitePool, id: &str, updated_config: serde_json::Value) -> Result<(), String> {
    let current: Option<(String,)> = sqlx::query_as("SELECT config FROM drivers WHERE name = ?")
        .bind(id)
        .fetch_optional(db)
        .await
        .map_err(|e| e.to_string())?;
    
    if let Some((config_str,)) = current {
        let mut config: serde_json::Value = serde_json::from_str(&config_str)
            .map_err(|e| e.to_string())?;
        
        if let Some(obj) = config.as_object_mut() {
            obj.insert("config".to_string(), updated_config);
        }
        
        let now = Utc::now().to_rfc3339();
        sqlx::query("UPDATE drivers SET config = ?, updated_at = ? WHERE name = ?")
            .bind(serde_json::to_string(&config).map_err(|e| e.to_string())?)
            .bind(&now)
            .bind(id)
            .execute(db)
            .await
            .map_err(|e| e.to_string())?;
        
        tracing::info!("Driver config saved to database: {}", id);
    }
    
    Ok(())
}

/// Handle embedded static file requests / 处理嵌入的静态文件请求
async fn serve_embedded_file(uri: Uri) -> impl IntoResponse {
    let path = uri.path().trim_start_matches('/');
    
    // Try to get requested file / 尝试获取请求的文件
    if let Some(content) = FrontendAssets::get(path) {
        let mime = mime_guess::from_path(path).first_or_octet_stream();
        return Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, mime.as_ref())
            .body(Body::from(content.data.into_owned()))
            .unwrap();
    }
    
    // If directory or file not found, try return index.html (SPA routing support) / 目录或文件不存在时返回index.html
    if let Some(content) = FrontendAssets::get("index.html") {
        return Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
            .body(Body::from(content.data.into_owned()))
            .unwrap();
    }
    
    Response::builder()
        .status(StatusCode::NOT_FOUND)
        .body(Body::from("Not Found"))
        .unwrap()
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "yaolist_backend=debug,tower_http=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Load configuration / 加载配置
    let app_config = config::load_config().expect("Failed to load configuration");
    tracing::info!("Server will listen on {}:{}", app_config.server.host, app_config.server.port);

    // Create data directory if not exists / 创建数据目录
    let data_dir = app_config.get_data_dir();
    if !data_dir.exists() {
        std::fs::create_dir_all(&data_dir)?;
        tracing::info!("Created data directory: {:?}", data_dir);
    }

    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| app_config.get_database_url());

    let pool = SqlitePool::connect(&database_url).await?;
    
    db::run_migrations(&pool).await?;

    let storage_manager = yaolist_backend::storage::StorageManager::new();
    
    // Register all storage driver factories / 注册所有存储驱动工厂
    yaolist_backend::register_storage_drivers(&storage_manager).await?;
    
    // Load saved driver configs from database / 从数据库加载已保存的驱动配置
    let saved_drivers: Vec<(String, String)> = sqlx::query_as(
        "SELECT name, config FROM drivers WHERE enabled = 1"
    )
    .fetch_all(&pool)
    .await?;
    
    // Load drivers in background with retry mechanism / 后台异步加载驱动，支持重试
    for (name, config_str) in saved_drivers {
        if let Ok(config) = serde_json::from_str::<serde_json::Value>(&config_str) {
            if let Some(driver_type) = config.get("driver_type").and_then(|v| v.as_str()) {
                if let Some(driver_config) = config.get("config") {
                    let sm = storage_manager.clone();
                    let dt = driver_type.to_string();
                    let dc = driver_config.clone();
                    let n = name.clone();
                    
                    let db = pool.clone();
                    // Async load with retry (max 3 attempts, 30s timeout each) / 异步加载支持重试
                    tokio::spawn(async move {
                        let max_retries = 3;
                        let mut attempt = 0;
                        
                        loop {
                            attempt += 1;
                            match tokio::time::timeout(
                                std::time::Duration::from_secs(30),
                                sm.create_driver(n.clone(), &dt, dc.clone())
                            ).await {
                                Ok(Ok(_)) => {
                                    if attempt > 1 {
                                        tracing::info!("Driver {} loaded successfully after {} attempts", n, attempt);
                                    }
                                    // 保存更新的配置（如刷新后的token）/ Save updated config (like refreshed token)
                                    if let Some(driver) = sm.get_driver(&n).await {
                                        if let Some(updated_config) = driver.get_updated_config() {
                                            if let Err(e) = save_driver_config_to_db(&db, &n, updated_config).await {
                                                tracing::warn!("Failed to save driver config: {} - {}", n, e);
                                            }
                                        }
                                    }
                                    break;
                                }
                                Ok(Err(e)) => {
                                    if attempt < max_retries {
                                        tracing::warn!("Driver {} load failed (attempt {}/{}): {}, retrying in 5s...", n, attempt, max_retries, e);
                                        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                                    } else {
                                        tracing::error!("Driver {} load failed after {} attempts: {}", n, max_retries, e);
                                        break;
                                    }
                                }
                                Err(_) => {
                                    if attempt < max_retries {
                                        tracing::warn!("Driver {} load timeout (attempt {}/{}), retrying in 5s...", n, attempt, max_retries);
                                        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                                    } else {
                                        tracing::error!("Driver {} load timeout after {} attempts", n, max_retries);
                                        sm.set_driver_error(&n, "Driver load timeout after 3 attempts".to_string()).await;
                                        break;
                                    }
                                }
                            }
                        }
                    });
                }
            }
        }
    }
    tracing::info!("Drivers loading in background (with retry)...");

    let mut task_manager = task::TaskManager::new();
    task_manager.set_db(pool.clone());
    
    // Load tasks from database / 从数据库加载任务
    task_manager.load_tasks_from_db().await;
    
    // Mark all running tasks as interrupted on startup / 服务器启动时标记运行中任务为中断
    task_manager.interrupt_all_running_tasks().await;
    
    let index_state = Arc::new(state::IndexState::new());
    
    // Initialize load balance manager / 初始化负载均衡管理器
    let load_balance = Arc::new(yaolist_backend::load_balance::LoadBalanceManager::new());
    
    // Initialize download settings / 初始化下载设置
    let download_settings = Arc::new(yaolist_backend::download::DownloadSettings::new());
    if let Err(e) = download_settings.load_from_db(&pool).await {
        tracing::warn!("Failed to load download settings: {}", e);
    }
    tracing::info!("Download settings loaded: domain={}", download_settings.get_download_domain());
    
    let state = Arc::new(AppState {
        db: pool,
        storage_manager,
        task_manager,
        db_index: tokio::sync::RwLock::new(None), // Lazy load
        index_state,
        load_balance,
        webdav_config: tokio::sync::RwLock::new(yaolist_backend::server::WebDavConfig::default()),
        login_security: state::LoginSecurity::new(),
        download_settings,
    });

    let app = Router::new()
        .route("/api/health", get(api::server::health_check))
        .route("/api/settings/public", get(api::settings::get_public_settings))
        .route("/api/settings", post(api::settings::update_settings))
        .route("/api/settings/geoip/status", get(api::settings::get_geoip_status))
        .route("/api/settings/geoip/download", post(api::settings::download_geoip_db))
        .route("/api/settings/geoip/reload", post(api::settings::reload_geoip_db))
        .route("/api/settings/geoip/config", get(api::settings::get_geoip_config))
        .route("/api/settings/geoip/config", post(api::settings::save_geoip_config))
        .route("/api/settings/version", get(api::settings::get_version_info))
        .route("/api/auth/login", post(api::auth::login))
        .route("/api/auth/logout", post(api::auth::logout))
        .route("/api/auth/register", post(api::auth::register))
        .route("/api/auth/check-unique", post(api::auth::check_unique))
        .route("/api/auth/registration-config", get(api::auth::get_registration_config))
        .route("/api/auth/permissions", get(api::auth::permissions))
        .route("/api/auth/captcha", get(api::auth::generate_captcha))
        .route("/api/auth/check-captcha", get(api::auth::check_need_captcha))
        .route("/api/auth/forgot-password", post(api::auth::forgot_password))
        .route("/api/auth/reset-password", post(api::auth::reset_password))
        .route("/api/auth/me", get(api::auth::get_current_user))
        .route("/api/auth/change-password", post(api::auth::change_password))
        .route("/api/auth/update-email", post(api::auth::update_email))
        .route("/api/auth/update-phone", post(api::auth::update_phone))
        .route("/api/auth/2fa/setup", post(api::auth::setup_2fa))
        .route("/api/auth/2fa/enable", post(api::auth::enable_2fa))
        .route("/api/auth/2fa/disable", post(api::auth::disable_2fa))
        .route("/api/users", get(api::users::list_users))
        .route("/api/users", post(api::users::create_user))
        .route("/api/users/:id", get(api::users::get_user))
        .route("/api/users/:id", post(api::users::update_user))
        .route("/api/users/:id/delete", post(api::users::delete_user))
        .route("/api/groups", get(api::groups::list_groups))
        .route("/api/groups", post(api::groups::create_group))
        .route("/api/groups/:id", get(api::groups::get_group))
        .route("/api/groups/:id", post(api::groups::update_group))
        .route("/api/groups/:id/delete", post(api::groups::delete_group))
        .route("/api/permissions", get(api::groups::list_permissions))
        .route("/api/drivers", get(api::drivers::list_drivers))
        .route("/api/drivers", post(api::drivers::create_driver))
        .route("/api/drivers/available", get(api::drivers::list_available_drivers))
        .route("/api/drivers/:id", post(api::drivers::update_driver))
        .route("/api/drivers/:id/enable", post(api::drivers::enable_driver))
        .route("/api/drivers/:id/disable", post(api::drivers::disable_driver))
        .route("/api/drivers/:id/delete", post(api::drivers::delete_driver))
        .route("/api/drivers/:id/reload", post(api::drivers::reload_driver))
        .route("/api/drivers/:id/space", get(api::drivers::get_driver_space))
        .route("/api/mounts", get(api::mounts::list_mounts))
        .route("/api/mounts", post(api::mounts::create_mount))
        .route("/api/mounts/:id", get(api::mounts::get_mount))
        .route("/api/mounts/:id", post(api::mounts::update_mount))
        .route("/api/mounts/:id/delete", post(api::mounts::delete_mount))
        .route("/api/metas", get(api::meta::list_metas))
        .route("/api/metas", post(api::meta::create_meta))
        .route("/api/metas/:id", get(api::meta::get_meta))
        .route("/api/metas/:id", post(api::meta::update_meta))
        .route("/api/metas/:id/delete", post(api::meta::delete_meta))
        .route("/api/meta/path", post(api::meta::get_meta_for_path))
        .route("/api/meta/verify", post(api::meta::verify_meta_password))
        .route("/api/direct_links", get(api::direct_links::list_direct_links))
        .route("/api/direct_links", post(api::direct_links::create_direct_link))
        .route("/api/direct_links/:id", post(api::direct_links::update_direct_link))
        .route("/api/direct_links/:id/delete", post(api::direct_links::delete_direct_link))
        .route("/api/direct_links/:id/toggle", post(api::direct_links::toggle_direct_link))
        // 分享管理API
        .route("/api/shares", get(api::shares::list_shares))
        .route("/api/shares", post(api::shares::create_share))
        .route("/api/shares/:id", post(api::shares::update_share))
        .route("/api/shares/:id/delete", post(api::shares::delete_share))
        .route("/api/shares/:id/toggle", post(api::shares::toggle_share))
        // 分享访问API（公开，无需认证）
        .route("/api/share/:short_id/info", get(api::shares::get_share_info))
        .route("/api/share/:short_id/verify", post(api::shares::verify_share))
        .route("/api/share/:short_id/files", post(api::shares::get_share_files))
        .route("/api/share/:short_id/download/:filename", get(api::shares::get_share_download))
        .route("/api/fs/list", post(api::files::fs_list))
        .route("/api/fs/get", post(api::files::fs_get))
        .route("/api/fs/mkdir", post(api::files::fs_mkdir))
        .route("/api/fs/write", post(api::files::fs_write))
        .route("/api/fs/remove", post(api::files::fs_remove))
        .route("/api/fs/rename", post(api::files::fs_rename))
        .route("/api/fs/move", post(api::files::fs_move))
        .route("/api/fs/copy", post(api::files::fs_copy))
        .route("/api/fs/get_download_url", post(api::files::fs_get_download_url))
        .route("/api/fs/get_direct_link", post(api::files::fs_get_direct_link))
        .route("/api/fs/upload", post(api::files::fs_upload))
        .route("/api/fs/upload/status", post(api::files::fs_upload_status))
        .route("/api/fs/upload/batch", post(api::files::fs_create_batch_upload))
        .route("/api/fs/upload/progress", post(api::files::fs_update_upload_progress))
        .route("/api/fs/upload/complete_file", post(api::files::fs_complete_file))
        .route("/api/admin/fs/list", post(api::files::admin_fs_list))
        .route("/api/tasks/list", post(api::tasks::list_tasks))
        .route("/api/tasks/get", post(api::tasks::get_task))
        .route("/api/tasks/cancel", post(api::tasks::cancel_task))
        .route("/api/tasks/pause", post(api::tasks::pause_task))
        .route("/api/tasks/resume", post(api::tasks::resume_task))
        .route("/api/tasks/clear", post(api::tasks::clear_completed))
        .route("/api/tasks/clear_all", post(api::tasks::clear_all_completed))
        .route("/api/tasks/remove", post(api::tasks::remove_task))
        .route("/api/tasks/retry", post(api::tasks::retry_task))
        .route("/api/tasks/restart", post(api::tasks::restart_task))
        .route("/api/fs/archive/list", post(api::archive::archive_list))
        .route("/api/fs/extract", post(api::extract::extract_archive))
        .route("/api/tasks", get(api::tasks::get_tasks))
        // 备份/恢复API
        .route("/api/admin/backup", get(api::backup::export_backup))
        .route("/api/admin/restore", post(api::backup::import_backup))
        // 搜索管理API
        .route("/api/admin/search/settings", get(api::search::get_search_settings))
        .route("/api/admin/search/settings", post(api::search::update_search_settings))
        .route("/api/admin/search/status", get(api::search::get_index_status))
        .route("/api/admin/search/index/rebuild", post(api::search::rebuild_index))
        .route("/api/admin/search/index/clear", post(api::search::clear_index))
        .route("/api/admin/search/index/stop", post(api::search::stop_indexing))
        .route("/api/search", post(api::search::search))
        .route("/api/search/enabled", get(api::search::is_search_enabled))
        // 通知配置API
        .route("/api/notifications/settings", get(api::notification::get_notification_settings))
        .route("/api/notifications/settings", post(api::notification::save_notification_settings))
        .route("/api/notifications/test/email", post(api::notification::test_email))
        .route("/api/notifications/test/sms", post(api::notification::test_sms))
        .route("/api/notifications/send-code", post(api::notification::send_verification_code))
        .route("/api/notifications/verify-code", post(api::notification::verify_code))
        // 负载均衡API
        .route("/api/load_balance/groups", get(api::load_balance::list_groups))
        .route("/api/load_balance/groups", post(api::load_balance::create_group))
        .route("/api/load_balance/groups/update", post(api::load_balance::update_group))
        .route("/api/load_balance/groups/delete", post(api::load_balance::delete_group))
        .route("/api/load_balance/modes", get(api::load_balance::list_modes))
        .route("/api/load_balance/geoip", post(api::load_balance::lookup_ip_info))
        .route("/download/:token", get(api::files::fs_download))
        .route("/dlink/*path", get(api::files::direct_link_download))
        // WebDAV routes
        .route("/dav", axum::routing::any(api::webdav::webdav_handler))
        .route("/dav/", axum::routing::any(api::webdav::webdav_handler))
        .route("/dav/*path", axum::routing::any(api::webdav::webdav_handler))
        // Embedded frontend static files
        .fallback(serve_embedded_file)
        .layer(DefaultBodyLimit::disable()) // No size limit
        .layer(CookieManagerLayer::new())
        .layer(CorsLayer::permissive())
        .with_state(state.clone());

    let bind_addr = app_config.get_bind_address();
    let listener = tokio::net::TcpListener::bind(&bind_addr)
        .await?;
    
    tracing::info!("Server running at http://{}", bind_addr);
    
    axum::serve(listener, app.into_make_service_with_connect_info::<std::net::SocketAddr>()).await?;

    Ok(())
}
