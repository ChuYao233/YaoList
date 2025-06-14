use axum::{
    extract::{Query, Json, Extension, DefaultBodyLimit},
    http::{StatusCode, HeaderMap},
    response::IntoResponse,
    routing::{get, post, put},
    Router,
};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::sync::Arc;
use tracing::{error, Level};
use tower_http::cors::CorsLayer;
use bcrypt::{hash, verify, DEFAULT_COST};
use sqlx::FromRow;
use std::collections::HashMap;
use tokio::sync::RwLock as AsyncRwLock;
use futures::future::BoxFuture;
use tracing_subscriber::FmtSubscriber;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::TraceLayer;
use tower::ServiceBuilder;
use rand::Rng;

mod drivers;
use drivers::{Driver, FileInfo};

#[derive(Debug, Serialize, Deserialize, Clone)]
struct ListParams {
    path: String,
}

#[derive(Deserialize)]
struct FileInfoParams {
    path: String,
}

#[derive(Deserialize)]
struct UserRegister {
    username: String,
    password: String,
}

#[derive(Deserialize)]
struct UserLogin {
    username: String,
    password: String,
}

#[derive(Deserialize)]
struct CreateUser {
    username: String,
    password: String,
    permissions: i32,
    enabled: bool,
    user_path: String,
}

#[derive(Deserialize)]
struct UpdateUser {
    username: Option<String>,
    password: Option<String>,
    permissions: Option<i32>,
    enabled: Option<bool>,
    user_path: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, FromRow)]
struct UserResponse {
    id: i64,
    username: String,
    permissions: i32,
    enabled: bool,
    user_path: String,
    created_at: Option<String>,
}

// 权限位定义
const PERM_UPLOAD: i32 = 1 << 0; // 1 创建目录或上传
const PERM_DOWNLOAD: i32 = 1 << 1; // 2 下载(包括在线预览)
const PERM_DELETE: i32 = 1 << 2; // 4 删除
const PERM_COPY: i32 = 1 << 3; // 8 复制
const PERM_MOVE: i32 = 1 << 4; // 16 移动
const PERM_RENAME: i32 = 1 << 5; // 32 重命名
const PERM_LIST: i32 = 1 << 6; // 64 列表

// 存储缓存
type StorageCache = Arc<AsyncRwLock<HashMap<i64, Storage>>>;

// 全局存储缓存
static STORAGE_CACHE: once_cell::sync::Lazy<StorageCache> = 
    once_cell::sync::Lazy::new(|| Arc::new(AsyncRwLock::new(HashMap::new())));

// 重新加载存储缓存
async fn reload_storage_cache(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    let storages: Vec<Storage> = sqlx::query_as::<_, Storage>(
        "SELECT id, name, storage_type, config, mount_path, enabled, created_at FROM storages"
    )
    .fetch_all(pool)
    .await?;

    let mut cache = STORAGE_CACHE.write().await;
    cache.clear();
    for storage in storages {
        cache.insert(storage.id, storage);
    }
    println!("🔄 存储缓存已重新加载，共 {} 个存储", cache.len());
    Ok(())
}

// 添加 index 处理函数
async fn index() -> impl IntoResponse {
    "Yaolist API Server"
}

// FileInfo 现在使用 drivers::FileInfo

#[derive(Deserialize)]
struct ChangePassword {
    old_password: String,
    new_password: String,
}

#[derive(Debug, Serialize, Deserialize, FromRow, Clone)]
struct Storage {
    id: i64,
    name: String,
    storage_type: String, // "local" 等
    config: String, // JSON配置
    mount_path: String, // 挂载路径
    enabled: bool,
    created_at: String,
}

#[derive(Debug, Deserialize)]
struct CreateStorage {
    name: String,
    storage_type: String,
    config: serde_json::Value,
    mount_path: String,
    enabled: bool,
}

#[derive(Debug, Deserialize)]
struct UpdateStorage {
    name: String,
    storage_type: String,
    config: serde_json::Value,
    mount_path: String,
    enabled: bool,
}

#[derive(Debug, Serialize, Deserialize, FromRow, Clone)]
struct SiteSetting {
    id: i64,
    setting_key: String,
    setting_value: String,
    setting_type: String,
    description: Option<String>,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, Deserialize)]
struct UpdateSiteSetting {
    setting_value: String,
}

#[derive(Debug, Deserialize)]
struct BatchUpdateSiteSettings {
    settings: std::collections::HashMap<String, String>,
}

// 添加通用认证函数
async fn authenticate_user(
    headers: &HeaderMap,
    pool: &SqlitePool,
    required_permission: i32,
) -> Result<(String, i32), (StatusCode, String)> {
    // 首先检查是否已登录
    let username = if let Some(username) = headers.get("x-username").and_then(|v| v.to_str().ok()) {
        // 用户已登录，直接使用用户名
        username.to_string()
    } else {
        // 用户未登录，尝试使用游客账号
        let guest_user: Option<(String, bool)> = sqlx::query_as("SELECT username, enabled FROM users WHERE username = 'guest'")
            .fetch_optional(pool)
            .await
            .unwrap();
        
        if let Some((guest_username, enabled)) = guest_user {
            if enabled {
                guest_username
            } else {
                return Err((StatusCode::UNAUTHORIZED, "未登录，请登录后访问".to_string()));
            }
        } else {
            return Err((StatusCode::UNAUTHORIZED, "未登录".to_string()));
        }
    };

    // 检查用户权限
    let user: Option<(i64, i32)> = sqlx::query_as("SELECT id, permissions FROM users WHERE username = ?")
        .bind(&username)
        .fetch_optional(pool)
        .await
        .unwrap();

    match user {
        Some((_id, permissions)) => {
            // 如果需要下载权限，同时也需要列表权限
            let actual_required_permission = if required_permission & PERM_DOWNLOAD != 0 {
                required_permission | PERM_LIST
            } else {
                required_permission
            };
            
            if permissions & actual_required_permission != actual_required_permission {
                return Err((StatusCode::FORBIDDEN, "无权限执行此操作".to_string()));
            }
            Ok((username, permissions))
        }
        None => Err((StatusCode::UNAUTHORIZED, "用户不存在".to_string())),
    }
}

#[axum::debug_handler]
async fn user_profile(headers: HeaderMap, Extension(pool): Extension<SqlitePool>) -> impl IntoResponse {
    let username = headers.get("x-username").and_then(|v| v.to_str().ok());
    if username.is_none() {
        return (StatusCode::UNAUTHORIZED, "未登录").into_response();
    }
    let username = username.unwrap();
    let user: Option<UserResponse> = sqlx::query_as::<_, UserResponse>(
        "SELECT id, username, permissions, enabled, user_path, created_at FROM users WHERE username = ?"
    )
    .bind(username)
    .fetch_optional(&pool)
    .await
    .unwrap();
    if let Some(user) = user {
        (StatusCode::OK, axum::Json(user)).into_response()
    } else {
        (StatusCode::UNAUTHORIZED, "用户不存在").into_response()
    }
}

#[axum::debug_handler]
async fn change_password(
    headers: HeaderMap,
    Extension(pool): Extension<SqlitePool>,
    Json(payload): Json<ChangePassword>,
) -> impl IntoResponse {
    let username = headers.get("x-username").and_then(|v| v.to_str().ok());
    if username.is_none() {
        return (StatusCode::UNAUTHORIZED, "未登录").into_response();
    }
    let username = username.unwrap();
    // 查询原密码
    let hashed_password: Option<String> = sqlx::query_scalar(
        "SELECT password FROM users WHERE username = ?"
    )
    .bind(username)
    .fetch_optional(&pool)
    .await
    .unwrap();
    if let Some(hashed) = hashed_password {
        if !verify(&payload.old_password, &hashed).unwrap_or(false) {
            return (StatusCode::BAD_REQUEST, "原密码错误").into_response();
        }
        let new_hashed = hash(&payload.new_password, DEFAULT_COST).unwrap();
        let _ = sqlx::query("UPDATE users SET password = ? WHERE username = ?")
            .bind(&new_hashed)
            .bind(username)
            .execute(&pool)
            .await;
        // 返回特殊状态码表示需要重新登录
        (StatusCode::RESET_CONTENT, "密码修改成功，请重新登录").into_response()
    } else {
        (StatusCode::UNAUTHORIZED, "用户不存在").into_response()
    }
}

#[tokio::main]
async fn main() {
    // ASCII艺术logo
    println!(r#"
██╗   ██╗ █████╗  ██████╗ ██╗     ██╗███████╗████████╗
╚██╗ ██╔╝██╔══██╗██╔═══██╗██║     ██║██╔════╝╚══██╔══╝
 ╚████╔╝ ███████║██║   ██║██║     ██║███████╗   ██║   
  ╚██╔╝  ██╔══██║██║   ██║██║     ██║╚════██║   ██║   
   ██║   ██║  ██║╚██████╔╝███████╗██║███████║   ██║   
   ╚═╝   ╚═╝  ╚═╝ ╚═════╝ ╚══════╝╚═╝╚══════╝   ╚═╝   
                                                       
文件管理系统 by ChuYao233
    "#);
    
    // 初始化日志 - 设置为INFO级别，减少debug输出
    let _subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)  // 改为INFO级别
        .with_target(false)
        .with_thread_ids(false)
        .with_file(false)
        .with_line_number(false)
        .with_thread_names(false)
        .with_level(false)  // 不显示日志级别
        .with_ansi(true)
        .with_writer(std::io::stdout)
        .compact()  // 使用紧凑格式
        .init();

    println!("🚀 服务器启动中...");

    // 获取当前工作目录
    let current_dir = std::env::current_dir().expect("Failed to get current directory");
    // info!("当前工作目录: {:?}", current_dir);  // 注释掉debug输出

    // 确保数据目录存在
    let data_dir = current_dir.join("data");
    if !data_dir.exists() {
        // info!("创建数据目录: {:?}", data_dir);  // 注释掉debug输出
        if let Err(e) = std::fs::create_dir_all(&data_dir) {
            error!("创建数据目录失败: {}", e);
            panic!("Failed to create data directory: {}", e);
        }
    }

    // 初始化数据库连接
    let db_path = data_dir.join("yaolist.db");
    // info!("数据库文件路径: {:?}", db_path);  // 注释掉debug输出
    
    // 确保数据库文件所在目录存在
    if let Some(parent) = db_path.parent() {
        if !parent.exists() {
            // info!("创建数据库目录: {:?}", parent);  // 注释掉debug输出
            if let Err(e) = std::fs::create_dir_all(parent) {
                error!("创建数据库目录失败: {}", e);
                panic!("Failed to create database directory: {}", e);
            }
        }
    }

    // 尝试创建数据库文件（如果不存在）
    if !db_path.exists() {
        // info!("创建数据库文件: {:?}", db_path);  // 注释掉debug输出
        if let Err(e) = std::fs::File::create(&db_path) {
            error!("创建数据库文件失败: {}", e);
            panic!("Failed to create database file: {}", e);
        }
    }

    let database_url = format!("sqlite:{}", db_path.to_str().unwrap());
    // info!("数据库连接URL: {}", database_url);  // 注释掉debug输出

    let pool = match SqlitePool::connect(&database_url).await {
        Ok(pool) => {
            println!("📊 数据库连接成功");
            pool
        }
        Err(e) => {
            error!("数据库连接失败: {}", e);
            panic!("Failed to connect to database: {}", e);
        }
    };

    // 创建用户表
    // info!("创建用户表...");  // 注释掉debug输出
    match sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS users (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            username TEXT UNIQUE NOT NULL,
            password TEXT NOT NULL,
            permissions INTEGER NOT NULL DEFAULT 1,
            enabled BOOLEAN NOT NULL DEFAULT 1,
            user_path TEXT NOT NULL DEFAULT '/',
            created_at DATETIME DEFAULT CURRENT_TIMESTAMP
        )
        "#
    )
    .execute(&pool)
    .await
    {
        Ok(_) => {}, // info!("用户表创建成功"),  // 注释掉debug输出
        Err(e) => {
            error!("用户表创建失败: {}", e);
            panic!("Failed to create users table: {}", e);
        }
    }

    // 检查并添加新字段（为了兼容旧数据库）
    let _ = sqlx::query("ALTER TABLE users ADD COLUMN enabled BOOLEAN NOT NULL DEFAULT 1").execute(&pool).await;
    let _ = sqlx::query("ALTER TABLE users ADD COLUMN user_path TEXT NOT NULL DEFAULT '/'").execute(&pool).await;
    let _ = sqlx::query("ALTER TABLE users ADD COLUMN created_at DATETIME DEFAULT CURRENT_TIMESTAMP").execute(&pool).await;

    // 创建存储表
    match sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS storages (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT UNIQUE NOT NULL,
            storage_type TEXT NOT NULL,
            config TEXT NOT NULL,
            mount_path TEXT UNIQUE NOT NULL,
            enabled BOOLEAN NOT NULL DEFAULT 1,
            created_at DATETIME DEFAULT CURRENT_TIMESTAMP
        )
        "#
    )
    .execute(&pool)
    .await
    {
        Ok(_) => {},
        Err(e) => {
            error!("存储表创建失败: {}", e);
            panic!("Failed to create storages table: {}", e);
        }
    }

    // 创建站点设置表
    match sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS site_settings (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            setting_key TEXT UNIQUE NOT NULL,
            setting_value TEXT NOT NULL,
            setting_type TEXT NOT NULL DEFAULT 'string',
            description TEXT,
            created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
            updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
        )
        "#
    )
    .execute(&pool)
    .await
    {
        Ok(_) => {},
        Err(e) => {
            error!("站点设置表创建失败: {}", e);
            panic!("Failed to create site_settings table: {}", e);
        }
    }

    // 检查是否有用户，无则创建默认账号
    let user_count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM users")
        .fetch_one(&pool)
        .await
        .expect("查询用户数量失败");
    if user_count.0 == 0 {
        // 创建管理员账号
        let admin_password: String = rand::thread_rng()
            .sample_iter(&rand::distributions::Alphanumeric)
            .take(12)
            .map(char::from)
            .collect();
        let hashed_admin = bcrypt::hash(&admin_password, DEFAULT_COST).unwrap();
        sqlx::query("INSERT INTO users (username, password, permissions, enabled, user_path, created_at) VALUES (?, ?, ?, ?, ?, CURRENT_TIMESTAMP)")
            .bind("admin")
            .bind(&hashed_admin)
            .bind(0xFFFF_FFFFu32 as i32) // 管理员拥有所有权限
            .bind(true)
            .bind("/") // 管理员可以访问根路径
            .execute(&pool)
            .await
            .expect("创建管理员账号失败");
        
        // 创建游客账号（无密码，默认禁用）
        sqlx::query("INSERT INTO users (username, password, permissions, enabled, user_path, created_at) VALUES (?, ?, ?, ?, ?, CURRENT_TIMESTAMP)")
            .bind("guest")
            .bind("") // 游客无密码
            .bind(PERM_LIST | PERM_DOWNLOAD) // 游客只有列表和下载权限
            .bind(false) // 默认禁用
            .bind("/") // 游客访问根路径
            .execute(&pool)
            .await
            .expect("创建游客账号失败");
        
        println!("👤 已自动创建管理员账号：admin，初始密码：{}", admin_password);
        println!("👤 已自动创建游客账号：guest（无密码，默认启用）");
    }

    // 检查是否有存储配置，无则创建默认存储
    let storage_count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM storages")
        .fetch_one(&pool)
        .await
        .expect("查询存储数量失败");
    if storage_count.0 == 0 {
        // 不再自动创建默认存储
        println!("💾 首次启动，未创建默认存储");
    }

    // 初始化存储缓存
    if let Err(e) = reload_storage_cache(&pool).await {
        eprintln!("初始化存储缓存失败: {}", e);
    }

    // 初始化/更新站点设置
    let default_settings = vec![
        ("site_title", "YaoList", "string", "站点标题"),
        ("pagination_type", "infinite", "string", "分页类型：infinite(无限滚动) 或 pagination(分页)"),
        ("items_per_page", "50", "number", "默认每页显示数量"),
        ("site_icon", "https://api.ylist.org/logo/logo.svg", "string", "站点图标URL"),
        ("favicon", "https://api.ylist.org/logo/logo.svg", "string", "网站图标URL"),
        ("theme_color", "#1976d2", "string", "主题色"),
        ("allow_registration", "true", "boolean", "是否允许用户注册"),
        ("site_description", "现代化的文件管理系统", "string", "站点描述"),
        ("preview_text_types", "txt,htm,html,xml,java,properties,sql,js,md,json,conf,ini,vue,php,py,bat,gitignore,yml,go,sh,c,cpp,h,hpp,tsx,vtt,srt,ass,rs,lrc", "string", "文本预览类型"),
        ("preview_audio_types", "mp3,flac,ogg,m4a,wav,opus,wma", "string", "音频预览类型"),
        ("preview_video_types", "mp4,mkv,avi,mov,rmvb,webm,flv", "string", "视频预览类型"),
        ("preview_image_types", "jpg,tiff,jpeg,png,gif,bmp,svg,ico,swf,webp", "string", "图片预览类型"),
        ("preview_proxy_types", "m3u8", "string", "代理预览类型"),
        ("preview_proxy_ignore_headers", "authorization,referer", "string", "代理忽略头部"),
        ("preview_external", "{}", "json", "外部预览配置"),
        ("preview_iframe", "{\"doc,docx,xls,xlsx,ppt,pptx\":{\"Microsoft\":\"https://view.officeapps.live.com/op/view.aspx?src=$e_url\",\"Google\":\"https://docs.google.com/gview?url=$e_url&embedded=true\"},\"pdf\":{\"PDF.js\":\"https://alist-org.github.io/pdf.js/web/viewer.html?file=$e_url\"},\"epub\":{\"EPUB.js\":\"https://alist-org.github.io/static/epub.js/viewer.html?url=$e_url\"}}", "json", "Iframe预览配置"),
        ("preview_audio_cover", "https://api.ylist.org/logo/logo.svg", "string", "音频封面"),
        ("preview_auto_play_audio", "false", "boolean", "自动播放音频"),
        ("preview_auto_play_video", "false", "boolean", "自动播放视频"),
        ("preview_default_archive", "false", "boolean", "默认情况下预览档案"),
        ("preview_readme_render", "true", "boolean", "ReadMe自动渲染"),
        ("preview_readme_filter_script", "true", "boolean", "过滤ReadMe文件中的脚本"),
        ("enable_top_message", "false", "boolean", "启用顶部自定义信息"),
        ("top_message", "", "text", "顶部自定义信息内容"),
        ("enable_bottom_message", "false", "boolean", "启用底部自定义信息"),
        ("bottom_message", "", "text", "底部自定义信息内容"),
        ("background_url", "", "string", "页面背景图片URL"),
        ("enable_glass_effect", "false", "boolean", "启用毛玻璃效果"),
    ];

    // 检查并插入缺失的设置项
    let mut inserted_count = 0;
    for (key, value, setting_type, description) in default_settings {
        // 检查设置是否已存在
        let existing: Option<(i64,)> = sqlx::query_as("SELECT id FROM site_settings WHERE setting_key = ?")
            .bind(key)
            .fetch_optional(&pool)
            .await
            .expect("查询站点设置失败");
        
        if existing.is_none() {
            // 插入新设置
            sqlx::query(
                "INSERT INTO site_settings (setting_key, setting_value, setting_type, description) VALUES (?, ?, ?, ?)"
            )
            .bind(key)
            .bind(value)
            .bind(setting_type)
            .bind(description)
            .execute(&pool)
            .await
            .expect("插入站点设置失败");
            inserted_count += 1;
        }
    }
    
    if inserted_count > 0 {
        println!("⚙️ 已添加 {} 个新的站点设置项", inserted_count);
    }

    // 配置CORS
    let cors = CorsLayer::new()
        .allow_origin(tower_http::cors::Any)
        .allow_methods(tower_http::cors::Any)
        .allow_headers(tower_http::cors::Any);

    let app = Router::new()
        .route("/api/files", get(list_files))
        .route("/api/fileinfo", get(file_info))
        .route("/api/download", get(download_file))
        .route("/api/upload", post(upload_file).layer(DefaultBodyLimit::max(1024 * 1024 * 1024))) // 1GB limit
        .route("/api/register", post(register_user))
        .route("/api/login", post(login_user))
        .route("/api/guest-login", get(guest_login))
        .route("/api/delete", post(delete_file))
        .route("/api/rename", post(rename_file))
        .route("/api/create-folder", post(create_folder))
        .route("/api/user/profile", get(user_profile))
        .route("/api/user/password", post(change_password))
        .route("/api/site-info", get(get_public_site_info))
        .route("/api/admin/users", get(list_users).post(create_user_admin))
        .route("/api/admin/users/:id", put(update_user_admin).delete(delete_user_admin))
        .route("/api/admin/storages", get(list_storages).post(create_storage))
        .route("/api/admin/storages/:id", put(update_storage).delete(delete_storage))
        .route("/api/admin/drivers", get(get_available_drivers_api))
        .route("/api/admin/site-settings", get(get_site_settings).put(batch_update_site_settings))
        .route("/api/admin/site-settings/:key", put(update_site_setting))
        .route("/api/transfer", post(transfer_file))
        // 驱动路由
        .merge(drivers::get_all_routes())
        // 静态资源服务，放最后，支持前端 history 刷新
        .nest_service("/", {
            let dist_path = std::env::current_dir().unwrap().join("dist");
            println!("📂 静态文件目录: {}", dist_path.display());
            if !dist_path.exists() {
                println!("❌ 静态文件目录不存在！");
            }
            ServeDir::new(dist_path.clone())
                .not_found_service(ServeFile::new(dist_path.join("index.html")))
                .with_buf_chunk_size(8192)
        })
        .layer(
            ServiceBuilder::new()
                .layer(TraceLayer::new_for_http()
                    .make_span_with(|request: &axum::http::Request<_>| {
                        let method = request.method();
                        let uri = request.uri();
                        let user_agent = request.headers()
                            .get("user-agent")
                            .and_then(|v| v.to_str().ok())
                            .unwrap_or("unknown");
                        
                        tracing::info_span!(
                            "http_request",
                            method = %method,
                            uri = %uri,
                            user_agent = %user_agent,
                        )
                    })
                    .on_request(|_request: &axum::http::Request<_>, _span: &tracing::Span| {
                        // 请求开始时的日志已经通过 make_span_with 处理
                    })
                    .on_response(|response: &axum::http::Response<_>, latency: std::time::Duration, _span: &tracing::Span| {
                        let status = response.status();
                        if status.is_client_error() || status.is_server_error() {
                            tracing::error!("❌ {} - {:.2}ms", status, latency.as_millis());
                        } else {
                            tracing::info!("✅ {} - {:.2}ms", status, latency.as_millis());
                        }
                    })
                    .on_failure(|error: tower_http::classify::ServerErrorsFailureClass, latency: std::time::Duration, _span: &tracing::Span| {
                        tracing::error!("💥 Request failed: {:?} - {:.2}ms", error, latency.as_millis());
                    })
                )
                .layer(Extension(pool))
                .layer(cors)
        );

    // 启动服务器
    let addr = "0.0.0.0:3000";
    println!("🌐 服务器监听地址: {}", addr);
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    println!("✅ 服务器启动成功，等待连接...");
    axum::serve(listener, app).await.unwrap();
}

#[axum::debug_handler]
async fn list_files(
    Query(params): Query<ListParams>,
    headers: HeaderMap,
    Extension(pool): Extension<SqlitePool>,
) -> Result<Json<Vec<FileInfo>>, (StatusCode, String)> {
    // 验证用户权限
    let (username, _) = authenticate_user(&headers, &pool, PERM_LIST).await?;

    let request_path = if params.path.trim().is_empty() || params.path == "/" {
        "/".to_string()
    } else {
        params.path.clone()
    };

    // 如果是根路径，返回所有一级目录
    if request_path == "/" {
        let cache = STORAGE_CACHE.read().await;
        let mut files = Vec::new();
        let mut seen_paths = std::collections::HashSet::new();
        
        // 收集所有一级目录
        for storage in cache.values() {
            // 只显示启用的存储
            if !storage.enabled {
                continue;
            }
            
            let mount_path = storage.mount_path.trim_matches('/');
            if mount_path.is_empty() || mount_path == "/" {
                continue;
            }
            
            // 获取第一级目录
            let first_segment = mount_path.split('/').next().unwrap_or(mount_path);
            if seen_paths.insert(first_segment.to_string()) {
                files.push(FileInfo {
                    name: first_segment.to_string(),
                    path: format!("/{}", first_segment),
                    size: 0,
                    is_dir: true,
                    modified: storage.created_at.clone(),
                });
            }
        }
        
        return Ok(Json(files));
    }

    // 处理子目录
    let path_segments: Vec<&str> = request_path.trim_matches('/').split('/').collect();
    let first_segment = path_segments[0];
    let cache = STORAGE_CACHE.read().await;
    let mut files = Vec::new();
    let mut seen_paths = std::collections::HashSet::new();

    // 如果是访问第一级目录（如 /ftp）
    if path_segments.len() == 1 {
        // 查找所有以此目录开头的存储
        for storage in cache.values() {
            let mount_path = storage.mount_path.trim_matches('/');
            let mount_segments: Vec<&str> = mount_path.split('/').collect();
            
            // 如果存储路径以当前目录开头
            if mount_segments.first() == Some(&first_segment) {
                if mount_segments.len() > 1 {
                    // 添加下一级目录
                    let second_segment = mount_segments[1];
                    if seen_paths.insert(second_segment.to_string()) {
                        files.push(FileInfo {
                            name: second_segment.to_string(),
                            path: format!("/{}/{}", first_segment, second_segment),
                            size: 0,
                            is_dir: true,
                            modified: storage.created_at.clone(),
                        });
                    }
                }
            }
        }

        if !files.is_empty() {
            return Ok(Json(files));
        }
    }

    // 处理更深层的目录
    if let Some(storage) = find_storage_for_path(&request_path).await {
        if let Some(driver) = create_driver_from_storage(&storage) {
            let relative_path = request_path.trim_start_matches(&format!("/{}", storage.mount_path.trim_matches('/')));
            if let Ok(mut storage_files) = driver.list(relative_path).await {
                for file in &mut storage_files {
                    file.path = format!("{}/{}", request_path.trim_end_matches('/'), file.name);
                }
                return Ok(Json(storage_files));
            }
        }
    }

    Err((StatusCode::NOT_FOUND, "路径不存在".to_string()))
}

async fn file_info(
    Query(params): Query<FileInfoParams>,
    headers: HeaderMap,
    Extension(pool): Extension<SqlitePool>,
) -> Result<Json<FileInfo>, (StatusCode, String)> {
    // 验证用户权限
    let (username, _) = authenticate_user(&headers, &pool, PERM_DOWNLOAD).await?;

    let path = &params.path;
    
    // 检查用户权限
    let user: Option<(i64, i32)> = sqlx::query_as("SELECT id, permissions FROM users WHERE username = ?")
        .bind(&username)
        .fetch_optional(&pool)
        .await
        .unwrap();

    if let Some((_id, permissions)) = user {
        if permissions & PERM_LIST == 0 {
            return Err((StatusCode::FORBIDDEN, "无列表权限".to_string()));
        }
    } else {
        return Err((StatusCode::UNAUTHORIZED, "用户不存在".to_string()));
    }
    
    // 查找对应的存储
    let storage = match find_storage_for_path(path).await {
        Some(storage) => storage,
        None => return Err((StatusCode::NOT_FOUND, "未找到对应的存储".to_string())),
    };

    let driver = match create_driver_from_storage(&storage) {
        Some(driver) => driver,
        None => return Err((StatusCode::INTERNAL_SERVER_ERROR, "无法创建存储驱动".to_string())),
    };

    // 计算相对于存储根目录的路径
    let relative_path = if storage.mount_path == "/" {
        path.trim_start_matches('/').to_string()
    } else {
        path.strip_prefix(&storage.mount_path)
            .unwrap_or("")
            .trim_start_matches('/')
            .to_string()
    };

    let relative_path = if relative_path.is_empty() { "/" } else { &relative_path };

    // 获取文件信息
    match driver.get_file_info(relative_path).await {
        Ok(file_info) => {
            Ok(Json(file_info))
        }
        Err(e) => {
            Err((StatusCode::NOT_FOUND, e.to_string()))
        }
    }
}

#[axum::debug_handler]
async fn register_user(
    Extension(pool): Extension<SqlitePool>,
    Json(payload): Json<UserRegister>,
) -> Result<Json<UserResponse>, (StatusCode, String)> {
    let hashed_password = hash(payload.password.as_bytes(), DEFAULT_COST).unwrap();
    let result = sqlx::query(
        r#"
        INSERT INTO users (username, password, permissions, enabled, user_path, created_at)
        VALUES (?, ?, ?, ?, ?, CURRENT_TIMESTAMP)
        "#,
    )
    .bind(&payload.username)
    .bind(&hashed_password)
    .bind(PERM_LIST | PERM_DOWNLOAD) // 默认权限：列表和下载
    .bind(true) // 默认启用
    .bind("/") // 默认根路径
    .execute(&pool)
    .await;

    match result {
        Ok(_) => Ok(Json(UserResponse {
            id: 0, // This will be set by the database
            username: payload.username,
            permissions: PERM_LIST | PERM_DOWNLOAD, // Default permissions
            enabled: true,
            user_path: "/".to_string(),
            created_at: None,
        })),
        Err(_) => Err((StatusCode::BAD_REQUEST, "用户名已存在".to_string())),
    }
}

#[axum::debug_handler]
async fn login_user(
    Extension(pool): Extension<SqlitePool>,
    Json(payload): Json<UserLogin>,
) -> Result<Json<UserResponse>, (StatusCode, String)> {
    // info!("尝试登录用户: {}", payload.username);  // 注释掉debug输出
    
    let result = sqlx::query_as::<_, UserResponse>(
        r#"
        SELECT id, username, permissions, enabled, user_path, created_at FROM users WHERE username = ?
        "#,
    )
    .bind(&payload.username)
    .fetch_optional(&pool)
    .await
    .map_err(|e| {
        error!("数据库查询失败: {}", e);
        (StatusCode::INTERNAL_SERVER_ERROR, "数据库错误".to_string())
    })?;

    match result {
        Some(user_data) => {
            // 检查用户是否启用
            if !user_data.enabled {
                return Err((StatusCode::FORBIDDEN, "账号已被禁用".to_string()));
            }

            let hashed_password: String = sqlx::query_scalar(
                r#"
                SELECT password FROM users WHERE username = ?
                "#,
            )
            .bind(&payload.username)
            .fetch_one(&pool)
            .await
            .map_err(|e| {
                error!("获取密码失败: {}", e);
                (StatusCode::INTERNAL_SERVER_ERROR, "数据库错误".to_string())
            })?;

            // 特殊处理游客账号（无密码）
            if payload.username == "guest" && hashed_password.is_empty() {
                // info!("游客登录成功");  // 注释掉debug输出
                Ok(Json(user_data))
            } else if verify(&payload.password, &hashed_password).map_err(|e| {
                error!("密码验证失败: {}", e);
                (StatusCode::INTERNAL_SERVER_ERROR, "密码验证错误".to_string())
            })? {
                // info!("用户登录成功: {}", payload.username);  // 注释掉debug输出
                Ok(Json(user_data))
            } else {
                // info!("密码错误: {}", payload.username);  // 注释掉debug输出
                Err((StatusCode::UNAUTHORIZED, "用户不存在或密码错误".to_string()))
            }
        }
        None => {
            // info!("用户不存在: {}", payload.username);  // 注释掉debug输出
            Err((StatusCode::UNAUTHORIZED, "用户不存在或密码错误".to_string()))
        }
    }
}

// 上传文件接口
async fn upload_file(
    headers: HeaderMap,
    Extension(pool): Extension<SqlitePool>,
    mut multipart: axum::extract::Multipart,
) -> impl IntoResponse {
    println!("开始处理上传请求");
    
    // 认证检查
    let username = if let Some(username) = headers.get("x-username").and_then(|v| v.to_str().ok()) {
        // 用户已登录，直接使用用户名
        username.to_string()
    } else {
        // 用户未登录，尝试使用游客账号
        let guest_user: Option<(String, bool)> = sqlx::query_as("SELECT username, enabled FROM users WHERE username = 'guest'")
            .fetch_optional(&pool)
            .await
            .unwrap();
        
        if let Some((guest_username, enabled)) = guest_user {
            if enabled {
                guest_username
            } else {
                return (StatusCode::UNAUTHORIZED, "未登录，请登录后访问").into_response();
            }
        } else {
            return (StatusCode::UNAUTHORIZED, "未登录").into_response();
        }
    };
    println!("上传用户: {}", username);
    
    let user: Option<(i64, i32)> = sqlx::query_as("SELECT id, permissions FROM users WHERE username = ?")
        .bind(username)
        .fetch_optional(&pool)
        .await
        .unwrap();
    if let Some((_id, permissions)) = user {
        if permissions & PERM_UPLOAD == 0 {
            println!("上传失败: 无上传权限");
            return (StatusCode::FORBIDDEN, "无上传权限").into_response();
        }
    } else {
        println!("上传失败: 用户不存在");
        return (StatusCode::UNAUTHORIZED, "用户不存在").into_response();
    }

    let mut upload_path = String::new();
    let mut relative_file_path = String::new();
    let mut file_data: Option<(String, Vec<u8>)> = None;

    // 解析multipart数据
    loop {
        match multipart.next_field().await {
            Ok(Some(field)) => {
                let name = field.name().unwrap_or("").to_string();
                println!("处理字段: {}", name);
                
                if name == "path" {
                    upload_path = match field.text().await {
                        Ok(path) => {
                            println!("上传路径: {}", path);
                            path
                        },
                        Err(e) => {
                            println!("读取路径失败: {}", e);
                            String::new()
                        },
                    };
                } else if name == "relative_path" {
                    relative_file_path = match field.text().await {
                        Ok(path) => {
                            println!("相对文件路径: {}", path);
                            path
                        },
                        Err(e) => {
                            println!("读取相对路径失败: {}", e);
                            String::new()
                        },
                    };
                } else if name == "file" {
                    let filename = field.file_name().unwrap_or("unknown").to_string();
                    println!("上传文件名: {}", filename);
                    
                    let data = match field.bytes().await {
                        Ok(bytes) => {
                            println!("文件大小: {} bytes", bytes.len());
                            bytes.to_vec()
                        },
                        Err(e) => {
                            println!("读取文件数据失败: {}", e);
                            return (StatusCode::BAD_REQUEST, format!("读取文件数据失败: {}", e)).into_response();
                        }
                    };
                    file_data = Some((filename, data));
                }
            },
            Ok(None) => {
                println!("multipart数据解析完成");
                break;
            },
            Err(e) => {
                println!("multipart解析错误: {}", e);
                return (StatusCode::BAD_REQUEST, format!("解析上传数据失败: {}", e)).into_response();
            }
        }
    }

    let (filename, data) = match file_data {
        Some(data) => data,
        None => {
            println!("上传失败: 未找到上传文件");
            return (StatusCode::BAD_REQUEST, "未找到上传文件").into_response();
        },
    };

    // 查找对应的存储
    let storage = match find_storage_for_path(&upload_path).await {
        Some(storage) => {
            println!("找到存储: {}", storage.name);
            storage
        },
        None => {
            println!("上传失败: 未找到对应的存储, 路径: {}", upload_path);
            return (StatusCode::NOT_FOUND, "未找到对应的存储").into_response();
        },
    };

    let driver = match create_driver_from_storage(&storage) {
        Some(driver) => driver,
        None => {
            println!("上传失败: 无法创建存储驱动");
            return (StatusCode::INTERNAL_SERVER_ERROR, "无法创建存储驱动").into_response();
        },
    };

    // 计算相对于存储根目录的路径
    let relative_path = if storage.mount_path == "/" {
        upload_path.trim_start_matches('/').to_string()
    } else {
        upload_path.strip_prefix(&storage.mount_path)
            .unwrap_or("")
            .trim_start_matches('/')
            .to_string()
    };

    // 处理文件夹上传
    let (final_parent_path, final_filename) = if !relative_file_path.is_empty() {
        // 有相对路径，说明是文件夹上传
        let path_parts: Vec<&str> = relative_file_path.split('/').collect();
        if path_parts.len() > 1 {
            // 需要创建文件夹结构
            let folder_path = path_parts[..path_parts.len()-1].join("/");
            let final_parent = if relative_path.is_empty() {
                folder_path
            } else {
                format!("{}/{}", relative_path, folder_path)
            };
            
            // 创建必要的文件夹
            println!("创建文件夹结构: {}", final_parent);
            if let Err(e) = create_folder_structure(&*driver, &final_parent).await {
                println!("创建文件夹结构失败: {}", e);
                return (StatusCode::INTERNAL_SERVER_ERROR, format!("创建文件夹结构失败: {}", e)).into_response();
            }
            
            (final_parent, filename)
        } else {
            // 只有文件名，没有文件夹
            let parent_path = if relative_path.is_empty() { "/".to_string() } else { relative_path };
            (parent_path, filename)
        }
    } else {
        // 普通文件上传
        let parent_path = if relative_path.is_empty() { "/".to_string() } else { relative_path };
        (parent_path, filename)
    };

    // 上传文件
    let parent_path_str = if final_parent_path == "/" { "/" } else { &final_parent_path };
    println!("上传到路径: {}, 文件名: {}", parent_path_str, final_filename);
    
    if let Err(e) = driver.upload_file(parent_path_str, &final_filename, &data).await {
        println!("上传失败: {}", e);
        return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response();
    }

    println!("上传成功: {}", final_filename);
    (StatusCode::OK, "上传成功").into_response()
}

// 辅助函数：递归创建文件夹结构
async fn create_folder_structure(driver: &dyn Driver, path: &str) -> anyhow::Result<()> {
    if path.is_empty() || path == "/" {
        return Ok(());
    }
    
    let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    let mut current_path = String::new();
    
    for part in parts {
        let parent_path = if current_path.is_empty() { "/" } else { &current_path };
        
        // 尝试创建文件夹（如果已存在会被忽略）
        if let Err(e) = driver.create_folder(parent_path, part).await {
            // 如果错误不是"文件夹已存在"类型的错误，则返回错误
            let error_msg = e.to_string().to_lowercase();
            if !error_msg.contains("exists") && !error_msg.contains("已存在") {
                return Err(e);
            }
        }
        
        current_path = if current_path.is_empty() {
            part.to_string()
        } else {
            format!("{}/{}", current_path, part)
        };
    }
    
    Ok(())
}

// 删除文件接口
#[derive(Deserialize)]
struct DeleteParams {
    path: String,
}

#[axum::debug_handler]
async fn delete_file(
    headers: HeaderMap,
    Extension(pool): Extension<SqlitePool>,
    Json(payload): Json<DeleteParams>,
) -> Result<Json<()>, (StatusCode, String)> {
    let username = headers.get("x-username").and_then(|v| v.to_str().ok());
    if username.is_none() {
        return Err((StatusCode::UNAUTHORIZED, "未登录".to_string()));
    }
    let username = username.unwrap();

    let user: Option<(i64, i32)> = sqlx::query_as("SELECT id, permissions FROM users WHERE username = ?")
        .bind(username)
        .fetch_optional(&pool)
        .await
        .unwrap();

    if let Some((_id, permissions)) = user {
        if permissions & PERM_DELETE == 0 {
            return Err((StatusCode::FORBIDDEN, "无删除权限".to_string()));
        }
    } else {
        return Err((StatusCode::UNAUTHORIZED, "用户不存在".to_string()));
    }

    // 查找对应的存储
    let storage = find_storage_for_path(&payload.path).await
        .ok_or((StatusCode::NOT_FOUND, "未找到对应的存储".to_string()))?;

    let driver = create_driver_from_storage(&storage)
        .ok_or((StatusCode::INTERNAL_SERVER_ERROR, "无法创建存储驱动".to_string()))?;

    // 计算相对于存储根目录的路径
    let relative_path = if storage.mount_path == "/" {
        payload.path.trim_start_matches('/').to_string()
    } else {
        payload.path.strip_prefix(&storage.mount_path)
            .unwrap_or("")
            .trim_start_matches('/')
            .to_string()
    };

    // 删除文件
    driver.delete(&relative_path).await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(()))
}

// 重命名接口
#[derive(Deserialize)]
struct RenameParams {
    old_path: String,
    new_path: String,
}

#[axum::debug_handler]
async fn rename_file(
    headers: HeaderMap,
    Extension(pool): Extension<SqlitePool>,
    Json(payload): Json<RenameParams>,
) -> Result<Json<()>, (StatusCode, String)> {
    let username = headers.get("x-username").and_then(|v| v.to_str().ok());
    if username.is_none() {
        return Err((StatusCode::UNAUTHORIZED, "未登录".to_string()));
    }
    let username = username.unwrap();

    let user: Option<(i64, i32)> = sqlx::query_as("SELECT id, permissions FROM users WHERE username = ?")
        .bind(username)
        .fetch_optional(&pool)
        .await
        .unwrap();

    if let Some((_id, permissions)) = user {
        if permissions & PERM_RENAME == 0 {
            return Err((StatusCode::FORBIDDEN, "无重命名权限".to_string()));
        }
    } else {
        return Err((StatusCode::UNAUTHORIZED, "用户不存在".to_string()));
    }

    // 查找对应的存储
    let storage = find_storage_for_path(&payload.old_path).await
        .ok_or((StatusCode::NOT_FOUND, "未找到对应的存储".to_string()))?;

    let driver = create_driver_from_storage(&storage)
        .ok_or((StatusCode::INTERNAL_SERVER_ERROR, "无法创建存储驱动".to_string()))?;

    // 计算相对于存储根目录的路径
    let old_relative_path = if storage.mount_path == "/" {
        payload.old_path.trim_start_matches('/').to_string()
    } else {
        payload.old_path.strip_prefix(&storage.mount_path)
            .unwrap_or("")
            .trim_start_matches('/')
            .to_string()
    };

    let new_relative_path = if storage.mount_path == "/" {
        payload.new_path.trim_start_matches('/').to_string()
    } else {
        payload.new_path.strip_prefix(&storage.mount_path)
            .unwrap_or("")
            .trim_start_matches('/')
            .to_string()
    };

    // 从新路径中提取文件名
    let new_name = std::path::Path::new(&new_relative_path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(&new_relative_path);
    
    // 重命名文件
    driver.rename(&old_relative_path, new_name).await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(()))
}

// 创建文件夹接口
#[derive(Deserialize)]
struct CreateFolderParams {
    parent_path: String,
    folder_name: String,
}

#[axum::debug_handler]
async fn create_folder(
    headers: HeaderMap,
    Extension(pool): Extension<SqlitePool>,
    Json(payload): Json<CreateFolderParams>,
) -> Result<Json<()>, (StatusCode, String)> {
    let username = headers.get("x-username").and_then(|v| v.to_str().ok());
    if username.is_none() {
        return Err((StatusCode::UNAUTHORIZED, "未登录".to_string()));
    }
    let username = username.unwrap();

    let user: Option<(i64, i32)> = sqlx::query_as("SELECT id, permissions FROM users WHERE username = ?")
        .bind(username)
        .fetch_optional(&pool)
        .await
        .unwrap();

    if let Some((_id, permissions)) = user {
        if permissions & PERM_UPLOAD == 0 {
            return Err((StatusCode::FORBIDDEN, "无创建文件夹权限".to_string()));
        }
    } else {
        return Err((StatusCode::UNAUTHORIZED, "用户不存在".to_string()));
    }

    // 查找对应的存储
    let storage = find_storage_for_path(&payload.parent_path).await
        .ok_or((StatusCode::NOT_FOUND, "未找到对应的存储".to_string()))?;

    let driver = create_driver_from_storage(&storage)
        .ok_or((StatusCode::INTERNAL_SERVER_ERROR, "无法创建存储驱动".to_string()))?;

    // 计算相对于存储根目录的路径
    let relative_path = if storage.mount_path == "/" {
        payload.parent_path.trim_start_matches('/').to_string()
    } else {
        payload.parent_path.strip_prefix(&storage.mount_path)
            .unwrap_or("")
            .trim_start_matches('/')
            .to_string()
    };

    println!("🔧 路径计算: storage.mount_path={}, payload.parent_path={}, relative_path={}", 
        storage.mount_path, payload.parent_path, relative_path);

    // 创建文件夹
    let parent_path = if relative_path.is_empty() { "/" } else { &relative_path };
    println!("🔧 创建文件夹: parent_path={}, folder_name={}", parent_path, payload.folder_name);
    
    driver.create_folder(parent_path, &payload.folder_name).await
        .map_err(|e| {
            println!("❌ 创建文件夹失败: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
        })?;

    println!("✅ 文件夹创建成功");
    Ok(Json(()))
}

// 存储管理API
#[axum::debug_handler]
async fn list_storages(
    headers: HeaderMap,
    Extension(pool): Extension<SqlitePool>,
) -> Result<Json<Vec<Storage>>, (StatusCode, String)> {
    let username = headers.get("x-username").and_then(|v| v.to_str().ok());
    if username.is_none() {
        return Err((StatusCode::UNAUTHORIZED, "未登录".to_string()));
    }
    let username = username.unwrap();

    // 检查是否为管理员
    let user: Option<(i64, i32)> = sqlx::query_as("SELECT id, permissions FROM users WHERE username = ?")
        .bind(username)
        .fetch_optional(&pool)
        .await
        .unwrap();

    if let Some((_id, permissions)) = user {
        if permissions != 0xFFFF_FFFFu32 as i32 {
            return Err((StatusCode::FORBIDDEN, "需要管理员权限".to_string()));
        }
    } else {
        return Err((StatusCode::UNAUTHORIZED, "用户不存在".to_string()));
    }

    let storages: Vec<Storage> = sqlx::query_as::<_, Storage>(
        "SELECT id, name, storage_type, config, mount_path, enabled, created_at FROM storages ORDER BY created_at DESC"
    )
    .fetch_all(&pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(storages))
}

#[axum::debug_handler]
async fn create_storage(
    headers: HeaderMap,
    Extension(pool): Extension<SqlitePool>,
    Json(payload): Json<CreateStorage>,
) -> Result<Json<Storage>, (StatusCode, String)> {
    let username = headers.get("x-username").and_then(|v| v.to_str().ok());
    if username.is_none() {
        return Err((StatusCode::UNAUTHORIZED, "未登录".to_string()));
    }
    let username = username.unwrap();

    // 检查是否为管理员
    let user: Option<(i64, i32)> = sqlx::query_as("SELECT id, permissions FROM users WHERE username = ?")
        .bind(username)
        .fetch_optional(&pool)
        .await
        .unwrap();

    if let Some((_id, permissions)) = user {
        if permissions != 0xFFFF_FFFFu32 as i32 {
            return Err((StatusCode::FORBIDDEN, "需要管理员权限".to_string()));
        }
    } else {
        return Err((StatusCode::UNAUTHORIZED, "用户不存在".to_string()));
    }

    // 创建存储
    let config_json = serde_json::to_string(&payload.config)
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("配置格式错误: {}", e)))?;

    let result = sqlx::query(
        "INSERT INTO storages (name, storage_type, config, mount_path, enabled) VALUES (?, ?, ?, ?, ?)"
    )
    .bind(&payload.name)
    .bind(&payload.storage_type)
    .bind(&config_json)
    .bind(&payload.mount_path)
    .bind(payload.enabled)
    .execute(&pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let storage: Storage = sqlx::query_as(
        "SELECT id, name, storage_type, config, mount_path, enabled, created_at FROM storages WHERE id = ?"
    )
    .bind(result.last_insert_rowid())
    .fetch_one(&pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // 重新加载存储缓存
    if let Err(e) = reload_storage_cache(&pool).await {
        eprintln!("重新加载存储缓存失败: {}", e);
    }

    Ok(Json(storage))
}

#[axum::debug_handler]
async fn update_storage(
    headers: HeaderMap,
    Extension(pool): Extension<SqlitePool>,
    axum::extract::Path(id): axum::extract::Path<i64>,
    Json(payload): Json<UpdateStorage>,
) -> Result<Json<Storage>, (StatusCode, String)> {
    let username = headers.get("x-username").and_then(|v| v.to_str().ok());
    if username.is_none() {
        return Err((StatusCode::UNAUTHORIZED, "未登录".to_string()));
    }
    let username = username.unwrap();

    // 检查是否为管理员
    let user: Option<(i64, i32)> = sqlx::query_as("SELECT id, permissions FROM users WHERE username = ?")
        .bind(username)
        .fetch_optional(&pool)
        .await
        .unwrap();

    if let Some((_id, permissions)) = user {
        if permissions != 0xFFFF_FFFFu32 as i32 {
            return Err((StatusCode::FORBIDDEN, "需要管理员权限".to_string()));
        }
    } else {
        return Err((StatusCode::UNAUTHORIZED, "用户不存在".to_string()));
    }

    // 获取当前存储信息
    let current_storage: Option<Storage> = sqlx::query_as(
        "SELECT id, name, storage_type, config, mount_path, enabled, created_at FROM storages WHERE id = ?"
    )
    .bind(id)
    .fetch_optional(&pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    if current_storage.is_none() {
        return Err((StatusCode::NOT_FOUND, "存储不存在".to_string()));
    }

    let current_storage = current_storage.unwrap();

    // 如果只是切换启用状态，保留原有配置
    let config_json = if payload.enabled != current_storage.enabled {
        current_storage.config
    } else {
        serde_json::to_string(&payload.config)
            .map_err(|e| (StatusCode::BAD_REQUEST, format!("配置格式错误: {}", e)))?
    };

    // 更新存储
    sqlx::query(
        "UPDATE storages SET name = ?, storage_type = ?, config = ?, mount_path = ?, enabled = ? WHERE id = ?"
    )
    .bind(&payload.name)
    .bind(&payload.storage_type)
    .bind(&config_json)
    .bind(&payload.mount_path)
    .bind(payload.enabled)
    .bind(id)
    .execute(&pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let storage: Storage = sqlx::query_as(
        "SELECT id, name, storage_type, config, mount_path, enabled, created_at FROM storages WHERE id = ?"
    )
    .bind(id)
    .fetch_one(&pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // 重新加载存储缓存
    if let Err(e) = reload_storage_cache(&pool).await {
        eprintln!("重新加载存储缓存失败: {}", e);
    }

    println!("✅ 存储更新成功");
    Ok(Json(storage))
}

#[axum::debug_handler]
async fn delete_storage(
    headers: HeaderMap,
    Extension(pool): Extension<SqlitePool>,
    axum::extract::Path(id): axum::extract::Path<i64>,
) -> Result<Json<()>, (StatusCode, String)> {
    let username = headers.get("x-username").and_then(|v| v.to_str().ok());
    if username.is_none() {
        return Err((StatusCode::UNAUTHORIZED, "未登录".to_string()));
    }
    let username = username.unwrap();

    // 检查是否为管理员
    let user: Option<(i64, i32)> = sqlx::query_as("SELECT id, permissions FROM users WHERE username = ?")
        .bind(username)
        .fetch_optional(&pool)
        .await
        .unwrap();

    if let Some((_id, permissions)) = user {
        if permissions != 0xFFFF_FFFFu32 as i32 {
            return Err((StatusCode::FORBIDDEN, "需要管理员权限".to_string()));
        }
    } else {
        return Err((StatusCode::UNAUTHORIZED, "用户不存在".to_string()));
    }

    // 删除存储
    sqlx::query("DELETE FROM storages WHERE id = ?")
        .bind(id)
        .execute(&pool)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // 重新加载存储缓存
    if let Err(e) = reload_storage_cache(&pool).await {
        eprintln!("重新加载存储缓存失败: {}", e);
    }

    Ok(Json(()))
}

// 获取可用驱动类型API
#[axum::debug_handler]
async fn get_available_drivers_api(
    headers: HeaderMap,
    Extension(pool): Extension<SqlitePool>,
) -> Result<Json<Vec<drivers::DriverInfo>>, (StatusCode, String)> {
    let username = headers.get("x-username").and_then(|v| v.to_str().ok());
    if username.is_none() {
        return Err((StatusCode::UNAUTHORIZED, "未登录".to_string()));
    }
    let username = username.unwrap();

    // 检查是否为管理员
    let user: Option<(i64, i32)> = sqlx::query_as("SELECT id, permissions FROM users WHERE username = ?")
        .bind(username)
        .fetch_optional(&pool)
        .await
        .unwrap();

    if let Some((_id, permissions)) = user {
        if permissions != 0xFFFF_FFFFu32 as i32 {
            return Err((StatusCode::FORBIDDEN, "需要管理员权限".to_string()));
        }
    } else {
        return Err((StatusCode::UNAUTHORIZED, "用户不存在".to_string()));
    }

    Ok(Json(drivers::get_available_drivers()))
}

// 根据路径查找对应的存储
async fn find_storage_for_path(path: &str) -> Option<Storage> {
    let cache = STORAGE_CACHE.read().await;
    let path = path.trim_matches('/');

    // 如果路径为空，返回根存储
    if path.is_empty() {
        return cache.values()
            .find(|s| (s.mount_path == "/" || s.mount_path.is_empty()) && s.enabled)
            .cloned();
    }

    // 1. 首先尝试完全匹配
    if let Some(storage) = cache.values()
        .find(|s| s.mount_path.trim_matches('/') == path && s.enabled)
        .cloned() {
        return Some(storage);
    }

    // 2. 然后尝试前缀匹配
    let mut best_match: Option<Storage> = None;
    let mut best_match_len = 0;

    for storage in cache.values() {
        // 只匹配启用的存储
        if !storage.enabled {
            continue;
        }
        
        let storage_path = storage.mount_path.trim_matches('/');
        
        // 如果是根存储，记录但继续查找更具体的匹配
        if storage_path.is_empty() {
            if best_match.is_none() {
                best_match = Some(storage.clone());
            }
            continue;
        }

        // 检查路径是否以存储路径开头
        if path.starts_with(storage_path) {
            let current_len = storage_path.len();
            if current_len > best_match_len {
                best_match_len = current_len;
                best_match = Some(storage.clone());
            }
        }
    }

    // 3. 如果是访问第一级目录（如 /ftp），返回所有相关存储
    let first_segment = path.split('/').next().unwrap_or(path);
    if path == first_segment {
        // 查找所有以此目录开头的存储
        let matching_storages: Vec<_> = cache.values()
            .filter(|s| {
                let storage_path = s.mount_path.trim_matches('/');
                storage_path == first_segment || storage_path.starts_with(&format!("{}/", first_segment))
            })
            .collect();

        if !matching_storages.is_empty() {
            // 返回最短的匹配（通常是父目录）
            return matching_storages.into_iter()
                .min_by_key(|s| s.mount_path.len())
                .cloned();
        }
    }

    best_match
}

// 从存储配置创建驱动
fn create_driver_from_storage(storage: &Storage) -> Option<Box<dyn Driver>> {
    if let Ok(config) = serde_json::from_str::<serde_json::Value>(&storage.config) {
        drivers::create_driver(&storage.storage_type, config).ok()
    } else {
        None
    }
}

async fn download_file(
    Query(params): Query<std::collections::HashMap<String, String>>,
    headers: HeaderMap,
    Extension(pool): Extension<SqlitePool>,
) -> impl IntoResponse {
    let path = params.get("path").cloned().unwrap_or_else(|| "".to_string());
    
    // 验证用户权限
    match authenticate_user(&headers, &pool, PERM_DOWNLOAD).await {
        Ok(_) => (),
        Err((status, message)) => return (status, message).into_response(),
    };
    
    // 查找对应的存储
    let storage = match find_storage_for_path(&path).await {
        Some(storage) => storage,
        None => return (StatusCode::NOT_FOUND, "未找到对应的存储".to_string()).into_response(),
    };

    let driver = match create_driver_from_storage(&storage) {
        Some(driver) => driver,
        None => return (StatusCode::INTERNAL_SERVER_ERROR, "无法创建存储驱动".to_string()).into_response(),
    };

    // 计算相对于存储根目录的路径
    let relative_path = if storage.mount_path == "/" {
        path.trim_start_matches('/').to_string()
    } else {
        path.strip_prefix(&storage.mount_path)
            .unwrap_or("")
            .trim_start_matches('/')
            .to_string()
    };

    println!("📁 相对路径: {} -> {}", path, relative_path);

    // 检查是否有特殊的下载URL（如OneDrive）
    match driver.get_download_url(&relative_path).await {
        Ok(Some(download_url)) => {
            // 重定向到特殊下载链接
            let mut headers = HeaderMap::new();
            headers.insert("location", download_url.parse().unwrap());
            (StatusCode::FOUND, headers, "").into_response()
        },
        Ok(None) => {
            // 检查是否有 Range 请求头
            let range_header = headers.get("range").and_then(|v| v.to_str().ok());
            
            if let Some(range_str) = range_header {
                // 解析 Range 请求头
                if let Some(range_str) = range_str.strip_prefix("bytes=") {
                    let (start, end) = if let Some((start_str, end_str)) = range_str.split_once('-') {
                        let start = if start_str.is_empty() { None } else { start_str.parse::<u64>().ok() };
                        let end = if end_str.is_empty() { None } else { end_str.parse::<u64>().ok() };
                        (start, end)
                    } else {
                        (None, None)
                    };
                    
                    // 尝试使用支持 Range 的流式下载
                    match driver.stream_download_with_range(&relative_path, start, end).await {
                        Ok(Some((stream, filename, file_size, content_length))) => {
                            let mut response_headers = HeaderMap::new();
                            
                            // 设置 Content-Type 为视频类型以支持预览
                            let content_type = if filename.ends_with(".mp4") {
                                "video/mp4"
                            } else if filename.ends_with(".avi") {
                                "video/x-msvideo"
                            } else if filename.ends_with(".mkv") {
                                "video/x-matroska"
                            } else if filename.ends_with(".mov") {
                                "video/quicktime"
                            } else {
                                "application/octet-stream"
                            };
                            
                            response_headers.insert("content-type", content_type.parse().unwrap());
                            response_headers.insert("accept-ranges", "bytes".parse().unwrap());
                            
                            if let Some(content_len) = content_length {
                                response_headers.insert("content-length", content_len.to_string().parse().unwrap());
                                let actual_start = start.unwrap_or(0);
                                let actual_end = actual_start + content_len - 1;
                                response_headers.insert("content-range", 
                                    format!("bytes {}-{}/{}", actual_start, actual_end, file_size).parse().unwrap());
                                
                                let body = axum::body::Body::from_stream(stream);
                                return (StatusCode::PARTIAL_CONTENT, response_headers, body).into_response();
                            }
                        },
                        Ok(None) => {
                            // Range 流式下载不支持，继续使用普通流式下载
                        },
                        Err(e) => {
                            println!("❌ Range 流式下载失败: {}", e);
                            return (StatusCode::INTERNAL_SERVER_ERROR, format!("Range 下载失败: {}", e)).into_response();
                        }
                    }
                }
            }
            
            // 首先尝试流式下载
                            match driver.stream_download(&relative_path).await {
                Ok(Some((stream, filename))) => {
                    // 使用流式下载
                    let mut response_headers = HeaderMap::new();
                    
                    // 设置 Content-Type 为视频类型以支持预览
                    let content_type = if filename.ends_with(".mp4") {
                        "video/mp4"
                    } else if filename.ends_with(".avi") {
                        "video/x-msvideo"
                    } else if filename.ends_with(".mkv") {
                        "video/x-matroska"
                    } else if filename.ends_with(".mov") {
                        "video/quicktime"
                    } else {
                        "application/octet-stream"
                    };
                    
                    response_headers.insert("content-type", content_type.parse().unwrap());
                    response_headers.insert("accept-ranges", "bytes".parse().unwrap());
                    response_headers.insert("transfer-encoding", "chunked".parse().unwrap());
                    
                    // 设置正确的文件名，支持中文文件名
                    let encoded_filename = urlencoding::encode(&filename);
                    response_headers.insert("content-disposition", 
                        format!("inline; filename=\"{}\"; filename*=UTF-8''{}", filename, encoded_filename).parse().unwrap());
                    
                    let body = axum::body::Body::from_stream(stream);
                    (StatusCode::OK, response_headers, body).into_response()
                },
                Ok(None) => {
                    // 流式下载不可用，使用标准文件下载
                    match driver.download(&relative_path).await {
                        Ok(mut file) => {
                            use tokio::io::AsyncReadExt;
                            
                            // 读取整个文件内容
                            let mut buffer = Vec::new();
                            match file.read_to_end(&mut buffer).await {
                                Ok(_) => {
                                    let filename = std::path::Path::new(&relative_path).file_name()
                                        .unwrap_or_else(|| std::ffi::OsStr::new("download"))
                                        .to_string_lossy();
                                    
                                    let mut response_headers = HeaderMap::new();
                                    
                                    // 设置正确的文件名，支持中文文件名
                                    let encoded_filename = urlencoding::encode(&filename);
                                    response_headers.insert("content-disposition", 
                                        format!("attachment; filename=\"{}\"; filename*=UTF-8''{}", filename, encoded_filename).parse().unwrap());
                                    response_headers.insert("content-type", "application/octet-stream".parse().unwrap());
                                    response_headers.insert("content-length", buffer.len().to_string().parse().unwrap());
                                    
                                    (StatusCode::OK, response_headers, buffer).into_response()
                                },
                                Err(e) => {
                                    println!("❌ 读取文件内容失败: {}", e);
                                    (StatusCode::INTERNAL_SERVER_ERROR, format!("读取文件失败: {}", e)).into_response()
                                }
                            }
                        },
                        Err(e) => {
                            println!("❌ 下载文件失败: {}", e);
                            (StatusCode::NOT_FOUND, e.to_string()).into_response()
                        },
                    }
                },
                Err(e) => {
                    println!("❌ 流式下载失败: {}", e);
                    (StatusCode::INTERNAL_SERVER_ERROR, format!("流式下载失败: {}", e)).into_response()
                },
            }
        },
        Err(e) => {
            println!("❌ 获取下载URL失败: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response()
        },
    }
}

// 站点设置管理API
#[axum::debug_handler]
async fn get_site_settings(
    headers: HeaderMap,
    Extension(pool): Extension<SqlitePool>,
) -> Result<Json<Vec<SiteSetting>>, (StatusCode, String)> {
    let username = headers.get("x-username").and_then(|v| v.to_str().ok());
    if username.is_none() {
        return Err((StatusCode::UNAUTHORIZED, "未登录".to_string()));
    }
    let username = username.unwrap();

    // 检查是否为管理员
    let user: Option<(i64, i32)> = sqlx::query_as("SELECT id, permissions FROM users WHERE username = ?")
        .bind(username)
        .fetch_optional(&pool)
        .await
        .unwrap();

    if let Some((_id, permissions)) = user {
        if permissions != 0xFFFF_FFFFu32 as i32 {
            return Err((StatusCode::FORBIDDEN, "需要管理员权限".to_string()));
        }
    } else {
        return Err((StatusCode::UNAUTHORIZED, "用户不存在".to_string()));
    }

    let settings: Vec<SiteSetting> = sqlx::query_as::<_, SiteSetting>(
        "SELECT id, setting_key, setting_value, setting_type, description, created_at, updated_at FROM site_settings WHERE setting_key IN ('site_title', 'site_description', 'theme_color', 'site_icon', 'favicon', 'background_url', 'enable_glass_effect', 'allow_registration', 'items_per_page', 'preview_text_types', 'preview_audio_types', 'preview_video_types', 'preview_image_types', 'preview_proxy_types', 'preview_proxy_ignore_headers', 'preview_external', 'preview_iframe', 'preview_audio_cover', 'preview_auto_play_audio', 'preview_auto_play_video', 'preview_default_archive', 'preview_readme_render', 'preview_readme_filter_script', 'enable_top_message', 'top_message', 'enable_bottom_message', 'bottom_message')"
    )
    .fetch_all(&pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(settings))
}

#[axum::debug_handler]
async fn update_site_setting(
    headers: HeaderMap,
    Extension(pool): Extension<SqlitePool>,
    axum::extract::Path(key): axum::extract::Path<String>,
    Json(payload): Json<UpdateSiteSetting>,
) -> Result<Json<SiteSetting>, (StatusCode, String)> {
    let username = headers.get("x-username").and_then(|v| v.to_str().ok());
    if username.is_none() {
        return Err((StatusCode::UNAUTHORIZED, "未登录".to_string()));
    }
    let username = username.unwrap();

    // 检查是否为管理员
    let user: Option<(i64, i32)> = sqlx::query_as("SELECT id, permissions FROM users WHERE username = ?")
        .bind(username)
        .fetch_optional(&pool)
        .await
        .unwrap();

    if let Some((_id, permissions)) = user {
        if permissions != 0xFFFF_FFFFu32 as i32 {
            return Err((StatusCode::FORBIDDEN, "需要管理员权限".to_string()));
        }
    } else {
        return Err((StatusCode::UNAUTHORIZED, "用户不存在".to_string()));
    }

    // 更新设置
    sqlx::query(
        "UPDATE site_settings SET setting_value = ?, updated_at = CURRENT_TIMESTAMP WHERE setting_key = ?"
    )
    .bind(&payload.setting_value)
    .bind(&key)
    .execute(&pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // 返回更新后的设置
    let setting: SiteSetting = sqlx::query_as(
        "SELECT id, setting_key, setting_value, setting_type, description, created_at, updated_at FROM site_settings WHERE setting_key = ?"
    )
    .bind(&key)
    .fetch_one(&pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(setting))
}

#[axum::debug_handler]
async fn batch_update_site_settings(
    headers: HeaderMap,
    Extension(pool): Extension<SqlitePool>,
    Json(payload): Json<BatchUpdateSiteSettings>,
) -> Result<Json<Vec<SiteSetting>>, (StatusCode, String)> {
    let username = headers.get("x-username").and_then(|v| v.to_str().ok());
    if username.is_none() {
        return Err((StatusCode::UNAUTHORIZED, "未登录".to_string()));
    }
    let username = username.unwrap();

    // 检查是否为管理员
    let user: Option<(i64, i32)> = sqlx::query_as("SELECT id, permissions FROM users WHERE username = ?")
        .bind(username)
        .fetch_optional(&pool)
        .await
        .unwrap();

    if let Some((_id, permissions)) = user {
        if permissions != 0xFFFF_FFFFu32 as i32 {
            return Err((StatusCode::FORBIDDEN, "需要管理员权限".to_string()));
        }
    } else {
        return Err((StatusCode::UNAUTHORIZED, "用户不存在".to_string()));
    }

    // 批量更新设置
    for (key, value) in payload.settings.iter() {
        sqlx::query(
            "UPDATE site_settings SET setting_value = ?, updated_at = CURRENT_TIMESTAMP WHERE setting_key = ?"
        )
        .bind(value)
        .bind(key)
        .execute(&pool)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    }

    // 返回所有设置
    let settings: Vec<SiteSetting> = sqlx::query_as::<_, SiteSetting>(
        "SELECT id, setting_key, setting_value, setting_type, description, created_at, updated_at FROM site_settings ORDER BY setting_key"
    )
    .fetch_all(&pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(settings))
}

// 公开的站点信息API（不需要管理员权限）
#[derive(Debug, Serialize)]
struct PublicSiteInfo {
    site_title: String,
    site_description: String,
    theme_color: String,
    site_icon: String,
    favicon: String,
    background_url: String,
    enable_glass_effect: bool,
    allow_registration: bool,
    items_per_page: i32,
    preview_text_types: String,
    preview_audio_types: String,
    preview_video_types: String,
    preview_image_types: String,
    preview_proxy_types: String,
    preview_proxy_ignore_headers: String,
    preview_external: String,
    preview_iframe: String,
    preview_audio_cover: String,
    preview_auto_play_audio: bool,
    preview_auto_play_video: bool,
    preview_default_archive: bool,
    preview_readme_render: bool,
    preview_readme_filter_script: bool,
    enable_top_message: bool,
    top_message: String,
    enable_bottom_message: bool,
    bottom_message: String,
}

// 用户管理API
#[axum::debug_handler]
async fn list_users(
    headers: HeaderMap,
    Extension(pool): Extension<SqlitePool>,
) -> Result<Json<Vec<UserResponse>>, (StatusCode, String)> {
    let username = headers.get("x-username").and_then(|v| v.to_str().ok());
    if username.is_none() {
        return Err((StatusCode::UNAUTHORIZED, "未登录".to_string()));
    }
    let username = username.unwrap();

    // 检查是否为管理员
    let user: Option<(i64, i32)> = sqlx::query_as("SELECT id, permissions FROM users WHERE username = ?")
        .bind(username)
        .fetch_optional(&pool)
        .await
        .unwrap();

    if let Some((_id, permissions)) = user {
        if permissions != 0xFFFF_FFFFu32 as i32 {
            return Err((StatusCode::FORBIDDEN, "需要管理员权限".to_string()));
        }
    } else {
        return Err((StatusCode::UNAUTHORIZED, "用户不存在".to_string()));
    }

    let users: Vec<UserResponse> = sqlx::query_as::<_, UserResponse>(
        "SELECT id, username, permissions, enabled, user_path, created_at FROM users ORDER BY created_at DESC"
    )
    .fetch_all(&pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(users))
}

#[axum::debug_handler]
async fn create_user_admin(
    headers: HeaderMap,
    Extension(pool): Extension<SqlitePool>,
    Json(payload): Json<CreateUser>,
) -> Result<Json<UserResponse>, (StatusCode, String)> {
    let username = headers.get("x-username").and_then(|v| v.to_str().ok());
    if username.is_none() {
        return Err((StatusCode::UNAUTHORIZED, "未登录".to_string()));
    }
    let username = username.unwrap();

    // 检查是否为管理员
    let user: Option<(i64, i32)> = sqlx::query_as("SELECT id, permissions FROM users WHERE username = ?")
        .bind(username)
        .fetch_optional(&pool)
        .await
        .unwrap();

    if let Some((_id, permissions)) = user {
        if permissions != 0xFFFF_FFFFu32 as i32 {
            return Err((StatusCode::FORBIDDEN, "需要管理员权限".to_string()));
        }
    } else {
        return Err((StatusCode::UNAUTHORIZED, "用户不存在".to_string()));
    }

    // 检查用户路径
    if payload.user_path != "/" {
        return Err((StatusCode::NOT_IMPLEMENTED, "用户路径设置功能正在开发中，目前仅支持根路径 '/'".to_string()));
    }

    // 创建用户
    let hashed_password = hash(payload.password.as_bytes(), DEFAULT_COST).unwrap();
    let result = sqlx::query(
        "INSERT INTO users (username, password, permissions, enabled, user_path, created_at) VALUES (?, ?, ?, ?, ?, CURRENT_TIMESTAMP)"
    )
    .bind(&payload.username)
    .bind(&hashed_password)
    .bind(payload.permissions)
    .bind(payload.enabled)
    .bind("/") // 强制使用根路径
    .execute(&pool)
    .await
    .map_err(|e| (StatusCode::BAD_REQUEST, format!("创建用户失败: {}", e)))?;

    let user: UserResponse = sqlx::query_as(
        "SELECT id, username, permissions, enabled, user_path, created_at FROM users WHERE id = ?"
    )
    .bind(result.last_insert_rowid())
    .fetch_one(&pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(user))
}

#[axum::debug_handler]
async fn update_user_admin(
    headers: HeaderMap,
    Extension(pool): Extension<SqlitePool>,
    axum::extract::Path(user_id): axum::extract::Path<i64>,
    Json(payload): Json<UpdateUser>,
) -> Result<Json<UserResponse>, (StatusCode, String)> {
    let username = headers.get("x-username").and_then(|v| v.to_str().ok());
    if username.is_none() {
        return Err((StatusCode::UNAUTHORIZED, "未登录".to_string()));
    }
    let username = username.unwrap();

    // 检查是否为管理员
    let user: Option<(i64, i32)> = sqlx::query_as("SELECT id, permissions FROM users WHERE username = ?")
        .bind(username)
        .fetch_optional(&pool)
        .await
        .unwrap();

    if let Some((_id, permissions)) = user {
        if permissions != 0xFFFF_FFFFu32 as i32 {
            return Err((StatusCode::FORBIDDEN, "需要管理员权限".to_string()));
        }
    } else {
        return Err((StatusCode::UNAUTHORIZED, "用户不存在".to_string()));
    }

    // 获取当前用户信息
    let current_user: Option<(String,)> = sqlx::query_as("SELECT user_path FROM users WHERE id = ?")
        .bind(user_id)
        .fetch_optional(&pool)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // 如果用户路径发生变化，返回功能开发中的提示
    if let Some(new_user_path) = &payload.user_path {
        if let Some((current_path,)) = current_user {
            if new_user_path != &current_path {
                return Err((StatusCode::NOT_IMPLEMENTED, "用户路径设置功能正在开发中".to_string()));
            }
        }
    }

    // 直接使用具体的更新语句，避免类型转换问题
    let mut updated = false;

    if let Some(new_username) = &payload.username {
        sqlx::query("UPDATE users SET username = ? WHERE id = ?")
            .bind(new_username)
            .bind(user_id)
            .execute(&pool)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        updated = true;
    }

    if let Some(new_password) = &payload.password {
        let hashed_password = hash(new_password.as_bytes(), DEFAULT_COST).unwrap();
        sqlx::query("UPDATE users SET password = ? WHERE id = ?")
            .bind(&hashed_password)
            .bind(user_id)
            .execute(&pool)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        updated = true;
    }

    if let Some(new_permissions) = payload.permissions {
        sqlx::query("UPDATE users SET permissions = ? WHERE id = ?")
            .bind(new_permissions)
            .bind(user_id)
            .execute(&pool)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        updated = true;
    }

    if let Some(new_enabled) = payload.enabled {
        sqlx::query("UPDATE users SET enabled = ? WHERE id = ?")
            .bind(new_enabled)
            .bind(user_id)
            .execute(&pool)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        updated = true;
    }

    if !updated {
        return Err((StatusCode::BAD_REQUEST, "没有要更新的字段".to_string()));
    }

    let updated_user: UserResponse = sqlx::query_as(
        "SELECT id, username, permissions, enabled, user_path, created_at FROM users WHERE id = ?"
    )
    .bind(user_id)
    .fetch_one(&pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(updated_user))
}

#[axum::debug_handler]
async fn delete_user_admin(
    headers: HeaderMap,
    Extension(pool): Extension<SqlitePool>,
    axum::extract::Path(user_id): axum::extract::Path<i64>,
) -> Result<Json<()>, (StatusCode, String)> {
    let username = headers.get("x-username").and_then(|v| v.to_str().ok());
    if username.is_none() {
        return Err((StatusCode::UNAUTHORIZED, "未登录".to_string()));
    }
    let username = username.unwrap();

    // 检查是否为管理员
    let user: Option<(i64, i32)> = sqlx::query_as("SELECT id, permissions FROM users WHERE username = ?")
        .bind(username)
        .fetch_optional(&pool)
        .await
        .unwrap();

    if let Some((_id, permissions)) = user {
        if permissions != 0xFFFF_FFFFu32 as i32 {
            return Err((StatusCode::FORBIDDEN, "需要管理员权限".to_string()));
        }
    } else {
        return Err((StatusCode::UNAUTHORIZED, "用户不存在".to_string()));
    }

    // 检查是否尝试删除自己
    let current_user: Option<(i64,)> = sqlx::query_as("SELECT id FROM users WHERE username = ?")
        .bind(username)
        .fetch_optional(&pool)
        .await
        .unwrap();

    if let Some((current_user_id,)) = current_user {
        if current_user_id == user_id {
            return Err((StatusCode::BAD_REQUEST, "不能删除自己的账号".to_string()));
        }
    }

    // 删除用户
    sqlx::query("DELETE FROM users WHERE id = ?")
        .bind(user_id)
        .execute(&pool)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(()))
}

// 游客自动登录API
#[axum::debug_handler]
async fn guest_login(
    Extension(pool): Extension<SqlitePool>,
) -> Result<Json<UserResponse>, (StatusCode, String)> {
    let result = sqlx::query_as::<_, UserResponse>(
        r#"
        SELECT id, username, permissions, enabled, user_path, created_at FROM users WHERE username = 'guest'
        "#,
    )
    .fetch_optional(&pool)
    .await
    .map_err(|e| {
        error!("数据库查询失败: {}", e);
        (StatusCode::INTERNAL_SERVER_ERROR, "数据库错误".to_string())
    })?;

    match result {
        Some(user_data) => {
            // 检查游客账号是否启用
            if !user_data.enabled {
                return Err((StatusCode::UNAUTHORIZED, "游客账号已被禁用".to_string()));
            }
            Ok(Json(user_data))
        }
        None => {
            Err((StatusCode::UNAUTHORIZED, "游客账号不存在".to_string()))
        }
    }
}

#[axum::debug_handler]
async fn get_public_site_info(
    Extension(pool): Extension<SqlitePool>,
) -> Result<Json<PublicSiteInfo>, (StatusCode, String)> {
    let settings: Vec<SiteSetting> = sqlx::query_as::<_, SiteSetting>(
        "SELECT id, setting_key, setting_value, setting_type, description, created_at, updated_at FROM site_settings"
    )
    .fetch_all(&pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let mut site_info = PublicSiteInfo {
        site_title: "YaoList".to_string(),
        site_description: "现代化的文件管理系统".to_string(),
        theme_color: "#1976d2".to_string(),
        site_icon: "".to_string(),
        favicon: "https://api.ylist.org/logo/logo.svg".to_string(),
        background_url: "".to_string(),
        enable_glass_effect: false,
        allow_registration: true,
        items_per_page: 50,
        preview_text_types: "txt,htm,html,xml,java,properties,sql,js,md,json,conf,ini,vue,php,py,bat,gitignore,yml,go,sh,c,cpp,h,hpp,tsx,vtt,srt,ass,rs,lrc".to_string(),
        preview_audio_types: "mp3,flac,ogg,m4a,wav,opus,wma".to_string(),
        preview_video_types: "mp4,mkv,avi,mov,rmvb,webm,flv".to_string(),
        preview_image_types: "jpg,tiff,jpeg,png,gif,bmp,svg,ico,swf,webp".to_string(),
        preview_proxy_types: "m3u8".to_string(),
        preview_proxy_ignore_headers: "authorization,referer".to_string(),
        preview_external: "{}".to_string(),
        preview_iframe: "{\"doc,docx,xls,xlsx,ppt,pptx\":{\"Microsoft\":\"https://view.officeapps.live.com/op/view.aspx?src=$e_url\",\"Google\":\"https://docs.google.com/gview?url=$e_url&embedded=true\"},\"pdf\":{\"PDF.js\":\"https://alist-org.github.io/pdf.js/web/viewer.html?file=$e_url\"},\"epub\":{\"EPUB.js\":\"https://alist-org.github.io/static/epub.js/viewer.html?url=$e_url\"}}".to_string(),
        preview_audio_cover: "https://api.ylist.org/logo/logo.svg".to_string(),
        preview_auto_play_audio: false,
        preview_auto_play_video: false,
        preview_default_archive: false,
        preview_readme_render: true,
        preview_readme_filter_script: true,
        enable_top_message: false,
        top_message: "".to_string(),
        enable_bottom_message: false,
        bottom_message: "".to_string(),
    };

    // 将设置应用到站点信息
    for setting in settings {
        match setting.setting_key.as_str() {
            "site_title" => site_info.site_title = setting.setting_value,
            "site_description" => site_info.site_description = setting.setting_value,
            "theme_color" => site_info.theme_color = setting.setting_value,
            "site_icon" => site_info.site_icon = setting.setting_value,
            "favicon" => site_info.favicon = setting.setting_value,
            "allow_registration" => site_info.allow_registration = setting.setting_value == "true",
            "items_per_page" => {
                if let Ok(value) = setting.setting_value.parse::<i32>() {
                    site_info.items_per_page = value;
                }
            },
            "preview_text_types" => site_info.preview_text_types = setting.setting_value,
            "preview_audio_types" => site_info.preview_audio_types = setting.setting_value,
            "preview_video_types" => site_info.preview_video_types = setting.setting_value,
            "preview_image_types" => site_info.preview_image_types = setting.setting_value,
            "preview_proxy_types" => site_info.preview_proxy_types = setting.setting_value,
            "preview_proxy_ignore_headers" => site_info.preview_proxy_ignore_headers = setting.setting_value,
            "preview_external" => site_info.preview_external = setting.setting_value,
            "preview_iframe" => site_info.preview_iframe = setting.setting_value,
            "preview_audio_cover" => site_info.preview_audio_cover = setting.setting_value,
            "preview_auto_play_audio" => site_info.preview_auto_play_audio = setting.setting_value == "true",
            "preview_auto_play_video" => site_info.preview_auto_play_video = setting.setting_value == "true",
            "preview_default_archive" => site_info.preview_default_archive = setting.setting_value == "true",
            "preview_readme_render" => site_info.preview_readme_render = setting.setting_value == "true",
            "preview_readme_filter_script" => site_info.preview_readme_filter_script = setting.setting_value == "true",
            "enable_top_message" => site_info.enable_top_message = setting.setting_value == "true",
            "top_message" => site_info.top_message = setting.setting_value,
            "enable_bottom_message" => site_info.enable_bottom_message = setting.setting_value == "true",
            "bottom_message" => site_info.bottom_message = setting.setting_value,
            "background_url" => site_info.background_url = setting.setting_value,
            "enable_glass_effect" => site_info.enable_glass_effect = setting.setting_value == "true",
            _ => {}
        }
    }

    Ok(Json(site_info))
}

// =============== 文件复制/移动相关 ===============
#[derive(Debug, Deserialize)]
struct TransferParams {
    src_path: String,
    dst_path: String,
    action: String, // "copy" 或 "move"
}

// 递归复制文件或文件夹（BoxFuture 解决 async recursion）
fn copy_recursively<'a>(
    src_driver: &'a dyn drivers::Driver,
    src_path: &'a str,
    dst_driver: &'a dyn drivers::Driver,
    dst_path: &'a str,
) -> BoxFuture<'a, anyhow::Result<()>> {
    Box::pin(async move {
        let info = src_driver.get_file_info(src_path).await?;

        if !info.is_dir {
            // 确保目标父目录存在
            if let Some(parent) = std::path::Path::new(dst_path).parent() {
                let parent_str = parent.to_string_lossy();
                if !parent_str.is_empty() {
                    create_folder_structure(dst_driver, &parent_str).await.ok();
                }
            }

            // 下载源文件到内存
            let mut file = src_driver.download(src_path).await?;
            use tokio::io::AsyncReadExt;
            let mut buf = Vec::new();
            file.read_to_end(&mut buf).await?;

            let filename = std::path::Path::new(dst_path)
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("copy.dat");
            let parent_dir = std::path::Path::new(dst_path)
                .parent()
                .map(|p| p.to_string_lossy())
                .unwrap_or("/".into());
            let parent_dir = if parent_dir.is_empty() { "/" } else { parent_dir.as_ref() };

            dst_driver.upload_file(parent_dir, filename, &buf).await?;
        } else {
            // 创建目标目录
            dst_driver
                .create_folder(
                    if dst_path == "/" {
                        "/"
                    } else {
                        std::path::Path::new(dst_path)
                            .parent()
                            .and_then(|p| p.to_str())
                            .unwrap_or("/")
                    },
                    std::path::Path::new(dst_path)
                        .file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or(src_path),
                )
                .await
                .ok();

            // 遍历子节点
            let children = src_driver.list(src_path).await?;
            for child in children {
                let child_src = format!("{}/{}", src_path.trim_end_matches('/'), child.name);
                let child_dst = format!("{}/{}", dst_path.trim_end_matches('/'), child.name);
                copy_recursively(src_driver, &child_src, dst_driver, &child_dst).await?;
            }
        }

        Ok(())
    })
}

#[axum::debug_handler]
async fn transfer_file(
    headers: HeaderMap,
    Extension(pool): Extension<SqlitePool>,
    Json(payload): Json<TransferParams>,
) -> Result<Json<()>, (StatusCode, String)> {
    let username = headers.get("x-username").and_then(|v| v.to_str().ok())
        .ok_or((StatusCode::UNAUTHORIZED, "未登录".to_string()))?;

    // 权限检查
    let user: Option<(i64, i32)> = sqlx::query_as("SELECT id, permissions FROM users WHERE username = ?")
        .bind(username)
        .fetch_optional(&pool)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("数据库错误: {}", e)))?;

    let permissions = user.map(|(_, p)| p).unwrap_or(0);
    match payload.action.as_str() {
        "copy" if permissions & PERM_COPY == 0 => {
            return Err((StatusCode::FORBIDDEN, "无复制权限".to_string()));
        },
        "move" if permissions & PERM_MOVE == 0 => {
            return Err((StatusCode::FORBIDDEN, "无移动权限".to_string()));
        },
        _ => {}
    }

    if payload.src_path == payload.dst_path {
        return Err((StatusCode::BAD_REQUEST, "源路径与目标路径相同".to_string()));
    }

    // 获取源和目标存储
    let src_storage = find_storage_for_path(&payload.src_path).await
        .ok_or((StatusCode::NOT_FOUND, "未找到源存储".to_string()))?;
    let dst_storage = find_storage_for_path(&payload.dst_path).await
        .ok_or((StatusCode::NOT_FOUND, "未找到目标存储".to_string()))?;

    let src_driver = create_driver_from_storage(&src_storage)
        .ok_or((StatusCode::INTERNAL_SERVER_ERROR, "无法创建源存储驱动".to_string()))?;
    let dst_driver = create_driver_from_storage(&dst_storage)
        .ok_or((StatusCode::INTERNAL_SERVER_ERROR, "无法创建目标存储驱动".to_string()))?;

    // 获取相对路径
    let src_rel = get_relative_path(&payload.src_path, &src_storage.mount_path);
    let dst_rel = get_relative_path(&payload.dst_path, &dst_storage.mount_path);

    // 获取源文件信息
    let src_info = src_driver.get_file_info(&src_rel).await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("获取源文件信息失败: {}", e)))?;

    if !src_info.is_dir {
        // 确保目标父目录存在
        if let Some(parent) = std::path::Path::new(&dst_rel).parent() {
            let parent_str = parent.to_string_lossy();
            if !parent_str.is_empty() {
                create_folder_structure(&*dst_driver, &parent_str).await
                    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("创建目标目录失败: {}", e)))?;
            }
        }

        // 获取文件名和父目录
        let (filename, parent_dir) = get_path_components(&dst_rel);

        // 尝试使用流式传输
        if let Some((stream, _)) = src_driver.stream_download(&src_rel).await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("流式下载失败: {}", e)))? {
            
            // 创建临时文件
            let temp_path = format!("temp_{}", uuid::Uuid::new_v4());
            let temp_file = tokio::fs::File::create(&temp_path).await
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("创建临时文件失败: {}", e)))?;
            
            // 写入临时文件
            let mut temp_file_writer = tokio::io::BufWriter::new(temp_file);
            use futures::StreamExt;
            use tokio::io::AsyncWriteExt;
            
            let mut stream = stream;
            while let Some(chunk) = stream.next().await {
                match chunk {
                    Ok(bytes) => {
                        temp_file_writer.write_all(&bytes).await
                            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("写入临时文件失败: {}", e)))?;
                    },
                    Err(e) => {
                        return Err((StatusCode::INTERNAL_SERVER_ERROR, format!("流式传输错误: {}", e)));
                    }
                }
            }
            
            temp_file_writer.flush().await
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("刷新临时文件失败: {}", e)))?;
            
            // 重新打开文件用于读取
            let mut file = tokio::fs::File::open(&temp_path).await
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("打开临时文件失败: {}", e)))?;
            
            // 读取文件内容
            use tokio::io::AsyncReadExt;
            let mut buf = Vec::new();
            file.read_to_end(&mut buf).await
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("读取临时文件失败: {}", e)))?;
            
            // 删除临时文件
            tokio::fs::remove_file(&temp_path).await.ok();
            
            // 上传到目标存储
            dst_driver.upload_file(parent_dir.as_str(), filename.as_str(), &buf).await
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("上传到目标存储失败: {}", e)))?;
        } else {
            // 如果不支持流式下载，使用普通下载
            let mut file = src_driver.download(&src_rel).await
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("下载源文件失败: {}", e)))?;
            
            use tokio::io::AsyncReadExt;
            let mut buf = Vec::new();
            file.read_to_end(&mut buf).await
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("读取源文件失败: {}", e)))?;
            
            // 上传到目标存储
            dst_driver.upload_file(parent_dir.as_str(), filename.as_str(), &buf).await
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("上传到目标存储失败: {}", e)))?;
        }
    } else {
        // 如果是目录，递归复制
        copy_recursively(&*src_driver, &src_rel, &*dst_driver, &dst_rel).await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    }

    if payload.action == "move" {
        src_driver.delete(&src_rel).await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("移动后删除源文件失败: {}", e)))?;
    }

    Ok(Json(()))
}

// 辅助函数：获取相对路径
fn get_relative_path(path: &str, mount_path: &str) -> String {
    if mount_path == "/" {
        path.trim_start_matches('/').to_string()
    } else {
        path.strip_prefix(mount_path)
            .unwrap_or("")
            .trim_start_matches('/')
            .to_string()
    }
}

// 辅助函数：获取路径组件
fn get_path_components(path: &str) -> (String, String) {
    let path = std::path::Path::new(path);
    let filename = path.file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("copy.dat")
        .to_string();
    
    let parent_dir = path.parent()
        .map(|p| p.to_string_lossy())
        .unwrap_or("/".into());
    
    let parent_dir = if parent_dir.is_empty() { "/" } else { parent_dir.as_ref() };
    
    (filename, parent_dir.to_string())
}
