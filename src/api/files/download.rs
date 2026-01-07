use std::sync::Arc;
use std::io::Write;
use axum::{
    extract::{State, Path, Query, ConnectInfo},
    http::{StatusCode, header, HeaderMap, Method},
    response::Response,
    body::Body,
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use tower_cookies::Cookies;
use tokio_util::io::ReaderStream;
use chrono::{Utc, Duration};

use crate::state::AppState;
use crate::api::file_resolver::{select_driver_for_download, select_driver_for_download_with_ip};
use yaolist_backend::utils::fix_and_clean_path;
use yaolist_backend::download::{ThrottledStream, TrafficCountingStream};

use super::{
    get_user_context, join_user_path, get_user_id, generate_token,
    DownloadToken, DOWNLOAD_TOKENS,
};
use crate::api::stats;

/// 生成短一点的签名用于直链
fn generate_sign() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let bytes: [u8; 16] = rng.gen();
    hex::encode(bytes)
}

#[derive(Debug, Deserialize)]
pub struct FsDownloadReq {
    pub path: String,
    pub expire_minutes: Option<i64>,
}

/// POST /api/fs/get_download_url - 获取临时下载链接
/// Get temporary download URL
pub async fn fs_get_download_url(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    cookies: Cookies,
    Json(req): Json<FsDownloadReq>,
) -> Result<Json<Value>, StatusCode> {
    let req_path = fix_and_clean_path(&req.path);
    
    // 获取用户上下文（权限+根路径）
    let user_ctx = get_user_context(&state, &cookies).await;
    let perms = &user_ctx.permissions;
    
    // 权限验证
    if !perms.read_files && !perms.is_admin {
        return Ok(Json(json!({
            "code": 403,
            "message": "没有下载文件的权限"
        })));
    }
    
    // 将用户请求路径与用户根路径结合（防止路径穿越）
    let path = match join_user_path(&user_ctx.root_path, &req_path) {
        Ok(p) => p,
        Err(e) => {
            return Ok(Json(json!({
                "code": 403,
                "message": e
            })));
        }
    };
    
    // Use configured expiry or request param or default / 使用配置的有效期或请求参数或默认值
    let configured_expiry = state.download_settings.get_link_expiry_minutes() as i64;
    let expire_minutes = req.expire_minutes.unwrap_or(configured_expiry);
    tracing::debug!("fs_get_download_url: expiry={}min", expire_minutes);
    
    // 使用file_resolver的负载均衡选择驱动（302优先+轮询）
    if let Some(selected) = select_driver_for_download(&state, &path).await {
        let token = generate_token();
        let expires_at = Utc::now() + Duration::minutes(expire_minutes);
        
        // 获取文件大小
        let file_size = if let Some(driver) = state.storage_manager.get_driver(&selected.driver_id).await {
            let parent_path = selected.internal_path.rsplitn(2, '/').nth(1).unwrap_or("/");
            let filename = selected.internal_path.split('/').last().unwrap_or("");
            driver.list(parent_path).await
                .ok()
                .and_then(|entries| {
                    entries.iter()
                        .find(|e| e.name == filename && !e.is_dir)
                        .map(|e| e.size)
                })
        } else {
            None
        };
        
        // 获取用户ID用于流量统计
        let user_id = get_user_id(&state, &cookies).await;
        
        let download_token = DownloadToken {
            path: selected.internal_path,
            driver_id: selected.driver_id,
            expires_at,
            can_direct_link: selected.can_direct_link,
            file_size,
            user_id,
        };
        
        // 存储令牌
        {
            let mut tokens = DOWNLOAD_TOKENS.write().await;
            tokens.insert(token.clone(), download_token);
        }
        
        // Build download URL with configured domain if set / 如果配置了下载域名则使用配置的域名
        // Get scheme from X-Forwarded-Proto header (reverse proxy support) / 从反代请求头获取协议
        let scheme = headers.get("x-forwarded-proto")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        let download_path = format!("/download/{}", token);
        let configured_domain = state.download_settings.get_download_domain();
        tracing::debug!("fs_get_download_url: configured_domain={}, scheme={}", configured_domain, scheme);
        let download_url = state.download_settings.build_download_url(&download_path, scheme);
        tracing::debug!("fs_get_download_url: download_url={}", download_url);
        
        // 注意：流量统计移到实际下载时进行
        // - 302重定向：统计整个文件大小
        // - 本地中转：统计实际传输流量
        
        return Ok(Json(json!({
            "code": 200,
            "message": "success",
            "data": {
                "url": download_url,
                "expires_at": expires_at.to_rfc3339()
            }
        })));
    }
    
    Ok(Json(json!({
        "code": 404,
        "message": "文件不存在"
    })))
}

#[derive(Debug, Deserialize)]
pub struct DownloadQuery {
    pub sign: Option<String>,
}

/// 解析 Range 请求头，返回 (start, end)
fn parse_range_header(range_header: Option<&str>, file_size: u64) -> Option<(u64, u64)> {
    let range_str = range_header?;
    if !range_str.starts_with("bytes=") {
        return None;
    }
    
    let range_spec = &range_str[6..];
    let parts: Vec<&str> = range_spec.split('-').collect();
    if parts.len() != 2 {
        return None;
    }
    
    let start: u64 = if parts[0].is_empty() {
        // 后缀范围: bytes=-500 表示最后500字节
        let suffix_len: u64 = parts[1].parse().ok()?;
        file_size.saturating_sub(suffix_len)
    } else {
        parts[0].parse().ok()?
    };
    
    let end: u64 = if parts[1].is_empty() {
        file_size - 1
    } else {
        parts[1].parse().ok()?
    };
    
    if start > end || start >= file_size {
        return None;
    }
    
    Some((start, end.min(file_size - 1)))
}

/// GET /download/:token - 下载文件（支持 Range 请求和302重定向）
/// Download file (supports Range requests and 302 redirect)
pub async fn fs_download(
    State(state): State<Arc<AppState>>,
    Path(token): Path<String>,
    Query(_query): Query<DownloadQuery>,
    method: Method,
    headers: HeaderMap,
) -> Result<Response, StatusCode> {
    // Validate download domain / 验证下载域名
    // 支持反代环境：优先使用 X-Forwarded-Host，其次使用 HOST
    let request_host = headers.get("X-Forwarded-Host")
        .or_else(|| headers.get(axum::http::header::HOST))
        .and_then(|h| h.to_str().ok());
    
    if let Some(host) = request_host {
        if !state.download_settings.validate_domain(host) {
            tracing::warn!("Download access from invalid domain: {} (X-Forwarded-Host or HOST)", host);
            return Err(StatusCode::FORBIDDEN);
        }
    }
    
    // 清理过期令牌
    {
        let mut tokens = DOWNLOAD_TOKENS.write().await;
        let now = Utc::now();
        tokens.retain(|_, v| v.expires_at > now);
    }
    
    // 查找令牌 / Find token
    let download_token = {
        let tokens = DOWNLOAD_TOKENS.read().await;
        if let Some(t) = tokens.get(&token) {
            if t.expires_at <= Utc::now() {
                drop(tokens);
                let mut tokens = DOWNLOAD_TOKENS.write().await;
                tokens.remove(&token);
                return Err(StatusCode::NOT_FOUND);
            }
            t.clone()
        } else {
            return Err(StatusCode::NOT_FOUND);
        }
    };
    
    // 获取驱动
    let driver = state.storage_manager.get_driver(&download_token.driver_id).await
        .ok_or(StatusCode::NOT_FOUND)?;
    
    // 如果驱动支持直链，尝试获取直链并302重定向（所有请求包括Range都走302）
    if download_token.can_direct_link {
        if let Ok(Some(direct_url)) = driver.get_direct_link(&download_token.path).await {
            tracing::debug!("fs_download: 302重定向到直链 url={}", direct_url);
            
            // 302重定向时统计整个文件大小的流量
            if let Some(ref user_id) = download_token.user_id {
                stats::record_download(&state.db, user_id, download_token.file_size).await;
            }
            
            // 获取请求的Origin头，用于CORS
            // 如果请求有Origin头，使用它；否则使用*（但不能与credentials一起使用）
            let mut response_builder = Response::builder()
                .status(StatusCode::FOUND)
                .header(header::LOCATION, direct_url)
                .header("Referrer-Policy", "no-referrer")
                .header(header::CACHE_CONTROL, "max-age=0, no-cache, no-store, must-revalidate");
            
            // 添加CORS头，允许跨域访问
            if let Some(origin) = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) {
                // 如果有Origin头，使用它并允许credentials
                response_builder = response_builder
                    .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, origin)
                    .header(header::ACCESS_CONTROL_ALLOW_CREDENTIALS, "true");
            } else {
                // 如果没有Origin头，使用*（但不能与credentials一起使用）
                response_builder = response_builder
                    .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*");
            }
            
            return Ok(response_builder
                .header(header::ACCESS_CONTROL_ALLOW_METHODS, "GET, HEAD, OPTIONS")
                .header(header::ACCESS_CONTROL_EXPOSE_HEADERS, "Content-Length, Content-Range, Accept-Ranges")
                .body(Body::empty())
                .unwrap());
        }
    }
    
    // 不支持直链或获取失败，使用流式代理
    // 使用缓存的文件大小（避免每次请求都调用list）
    let file_size = download_token.file_size;
    
    tracing::debug!("fs_download proxy: using cached file_size={:?}", file_size);
    
    // 获取文件名和Content-Type
    let filename = download_token.path.split('/').last().unwrap_or("download");
    let filename_encoded = urlencoding::encode(filename);
    let content_type = mime_guess::from_path(&download_token.path)
        .first_or_octet_stream()
        .to_string();
    
    // 解析 Range 请求头
    let range_header = headers.get(header::RANGE)
        .and_then(|v| v.to_str().ok());
    
    // 如果有文件大小且有 Range 请求，尝试处理部分内容
    if let (Some(size), Some(range_str)) = (file_size, range_header) {
        tracing::debug!("fs_download: Range request: {} for file size {}", range_str, size);
        if let Some((start, end)) = parse_range_header(Some(range_str), size) {
            let content_length = end - start + 1;
            tracing::debug!("fs_download: Parsed range: start={}, end={}, content_length={}", start, end, content_length);
            
            // 使用带 Range 的流式接口
            let reader = driver.open_reader(&download_token.path, Some(start..(end + 1))).await
                .map_err(|e| {
                    tracing::error!("fs_download: open_reader failed: {}", e);
                    StatusCode::NOT_FOUND
                })?;
            
            let stream = ReaderStream::new(reader);
            // 包装流量统计（本地中转统计实际传输流量）
            let stream = TrafficCountingStream::new(stream, download_token.user_id.clone(), state.db.clone());
            // Apply global bandwidth limiting (shared across all downloads) / 应用全局带宽限制（所有下载共享）
            let max_speed = state.download_settings.get_max_speed();
            let body = if max_speed > 0 {
                let limiter = state.download_settings.get_limiter();
                Body::from_stream(ThrottledStream::new(stream, limiter))
            } else {
                Body::from_stream(stream)
            };
            
            return Ok(Response::builder()
                .status(StatusCode::PARTIAL_CONTENT)
                .header(header::CONTENT_TYPE, &content_type)
                .header(header::ACCEPT_RANGES, "bytes")
                .header(header::CONTENT_RANGE, format!("bytes {}-{}/{}", start, end, size))
                .header(header::CONTENT_LENGTH, content_length)
                .body(body)
                .unwrap());
        }
    }
    
    // 完整文件下载 / Full file download
    let reader = driver.open_reader(&download_token.path, None).await
        .map_err(|_| StatusCode::NOT_FOUND)?;
    
    let stream = ReaderStream::new(reader);
    // 包装流量统计（本地中转统计实际传输流量）
    let stream = TrafficCountingStream::new(stream, download_token.user_id.clone(), state.db.clone());
    // Apply global bandwidth limiting (shared across all downloads) / 应用全局带宽限制（所有下载共享）
    let max_speed = state.download_settings.get_max_speed();
    tracing::info!("fs_download: max_speed={} bytes/s ({}MB/s)", max_speed, max_speed / 1024 / 1024);
    let body = if max_speed > 0 {
        let limiter = state.download_settings.get_limiter();
        tracing::info!("fs_download: applying bandwidth limit, limiter rate={}", limiter.get_rate());
        Body::from_stream(ThrottledStream::new(stream, limiter))
    } else {
        tracing::info!("fs_download: no bandwidth limit");
        Body::from_stream(stream)
    };
    
    let mut response = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type)
        .header(header::ACCEPT_RANGES, "bytes")
        .header(header::CONTENT_DISPOSITION, format!("attachment; filename=\"{}\"; filename*=UTF-8''{}", filename, filename_encoded));
    
    // 如果获取到文件大小，添加Content-Length header
    if let Some(size) = file_size {
        response = response.header(header::CONTENT_LENGTH, size);
    }
    
    // HEAD请求：只返回headers，不返回body
    if method == Method::HEAD {
        return Ok(response.body(Body::empty()).unwrap());
    }
    
    Ok(response.body(body).unwrap())
}

/// 将目录压缩为zip（使用流式写入，但最终返回完整数据）
fn create_zip_from_dir(dir_path: &std::path::Path) -> Result<Vec<u8>, std::io::Error> {
    use std::io::Cursor;
    
    let mut buffer = Vec::new();
    {
        let cursor = Cursor::new(&mut buffer);
        let mut zip = zip::ZipWriter::new(cursor);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        
        add_dir_to_zip(&mut zip, dir_path, "", &options)?;
        zip.finish()?;
    }
    Ok(buffer)
}

fn add_dir_to_zip<W: Write + std::io::Seek>(
    zip: &mut zip::ZipWriter<W>,
    dir_path: &std::path::Path,
    prefix: &str,
    options: &zip::write::SimpleFileOptions,
) -> Result<(), std::io::Error> {
    for entry in std::fs::read_dir(dir_path)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        
        let full_name = if prefix.is_empty() {
            name_str.to_string()
        } else {
            format!("{}/{}", prefix, name_str)
        };
        
        if path.is_dir() {
            zip.add_directory(&full_name, *options)?;
            add_dir_to_zip(zip, &path, &full_name, options)?;
        } else {
            zip.start_file(&full_name, *options)?;
            let mut file = std::fs::File::open(&path)?;
            std::io::copy(&mut file, zip)?;
        }
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
pub struct FsDirectLinkReq {
    pub path: String,
    pub expires_at: Option<String>,
    pub max_access_count: Option<i64>,
}

/// POST /api/fs/get_direct_link - 获取永久直链
/// Get permanent direct link
pub async fn fs_get_direct_link(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    cookies: Cookies,
    Json(req): Json<FsDirectLinkReq>,
) -> Result<Json<Value>, StatusCode> {
    let req_path = fix_and_clean_path(&req.path);
    
    // 获取用户上下文（权限+根路径）
    let user_ctx = get_user_context(&state, &cookies).await;
    let perms = &user_ctx.permissions;
    
    // 权限验证
    if !perms.allow_direct_link && !perms.is_admin {
        return Ok(Json(json!({
            "code": 403,
            "message": "没有创建直链的权限"
        })));
    }
    
    // 将用户请求路径与用户根路径结合（防止路径穿越）
    let path = match join_user_path(&user_ctx.root_path, &req_path) {
        Ok(p) => p,
        Err(e) => {
            return Ok(Json(json!({
                "code": 403,
                "message": e
            })));
        }
    };
    
    // 检查是否已有直链
    let existing: Option<(String,)> = sqlx::query_as(
        "SELECT sign FROM direct_links WHERE path = ?"
    )
    .bind(&path)
    .fetch_optional(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    
    let filename = path.split('/').last().unwrap_or("file");
    
    // Get scheme and host from headers for building URL
    // 从请求头获取协议和域名用于构建URL
    let scheme = headers.get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("http");
    let request_host = headers.get("x-forwarded-host")
        .or_else(|| headers.get(axum::http::header::HOST))
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    
    // 构建直链URL的辅助函数
    let build_dlink_url = |dlink_path: &str| -> String {
        let configured = state.download_settings.get_download_domain();
        if configured.is_empty() {
            // 未配置下载域名，使用请求的Host构建完整URL
            if request_host.is_empty() {
                dlink_path.to_string()
            } else {
                format!("{}://{}{}", scheme, request_host, dlink_path)
            }
        } else {
            state.download_settings.build_download_url(dlink_path, scheme)
        }
    };
    
    if let Some((sign,)) = existing {
        // Build direct link URL with configured domain / 使用配置的下载域名生成直链
        let dlink_path = format!("/dlink/{}/{}", sign, urlencoding::encode(filename));
        let dlink_url = build_dlink_url(&dlink_path);
        return Ok(Json(json!({
            "code": 200,
            "message": "success",
            "data": {
                "url": dlink_url
            }
        })));
    }
    
    // 生成新直链
    let sign = generate_sign();
    let now = Utc::now().to_rfc3339();
    
    // 获取用户ID（使用现有的get_user_id函数）
    let user_id = get_user_id(&state, &cookies).await;
    
    sqlx::query(
        "INSERT INTO direct_links (user_id, sign, path, filename, expires_at, max_access_count, enabled, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, 1, ?, ?)"
    )
    .bind(&user_id)
    .bind(&sign)
    .bind(&path)
    .bind(filename)
    .bind(&req.expires_at)
    .bind(&req.max_access_count)
    .bind(&now)
    .bind(&now)
    .execute(&state.db)
    .await
    .map_err(|e| {
        tracing::error!("Failed to create direct link: {:?}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    
    // Build direct link URL with configured domain / 使用配置的下载域名生成直链
    let dlink_path = format!("/dlink/{}/{}", sign, urlencoding::encode(filename));
    let dlink_url = build_dlink_url(&dlink_path);
    
    Ok(Json(json!({
        "code": 200,
        "message": "success",
        "data": {
            "url": dlink_url
        }
    })))
}

/// 创建错误响应
fn direct_link_error_response(status: StatusCode, code: &str, message: &str) -> Response {
    let body = serde_json::json!({
        "code": code,
        "message": message
    });
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json; charset=utf-8")
        .body(Body::from(body.to_string()))
        .unwrap()
}

/// 直链查询参数
#[derive(Debug, Deserialize)]
pub struct DlinkQuery {
    /// 如果为true，返回JSON格式的URL而不是302重定向
    pub url_only: Option<bool>,
}

/// GET /dlink/:sign/:filename - 访问直链 / Access direct link
pub async fn direct_link_download(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    ConnectInfo(addr): ConnectInfo<std::net::SocketAddr>,
    Path(sign): Path<String>,
    Query(query): Query<DlinkQuery>,
) -> Response {
    // Validate download domain / 验证下载域名
    // 支持反代环境：优先使用 X-Forwarded-Host，其次使用 HOST
    let request_host = headers.get("X-Forwarded-Host")
        .or_else(|| headers.get(axum::http::header::HOST))
        .and_then(|h| h.to_str().ok());
    
    if let Some(host) = request_host {
        if !state.download_settings.validate_domain(host) {
            tracing::warn!("Direct link access from invalid domain: {} (X-Forwarded-Host or HOST)", host);
            return direct_link_error_response(StatusCode::FORBIDDEN, "INVALID_DOMAIN", "请使用正确的下载域名访问");
        }
    }
    
    // 获取真实客户端IP（支持反代/CDN）
    let client_ip = yaolist_backend::geoip::extract_client_ip(&headers, Some(addr.ip()));
    // sign 可能包含 /filename 部分，需要提取
    let sign = sign.split('/').next().unwrap_or(&sign).to_string();
    let url_only = query.url_only.unwrap_or(false);
    
    // 查找直链（包含所有验证需要的字段）
    let link: Option<(i64, String, bool, Option<String>, Option<i64>, i64)> = match sqlx::query_as(
        "SELECT id, path, enabled, expires_at, max_access_count, access_count FROM direct_links WHERE sign = ?"
    )
    .bind(&sign)
    .fetch_optional(&state.db)
    .await {
        Ok(l) => l,
        Err(_) => return direct_link_error_response(StatusCode::INTERNAL_SERVER_ERROR, "SERVER_ERROR", "服务器内部错误"),
    };
    
    let (link_id, path, enabled, expires_at, max_access_count, access_count) = match link {
        Some(l) => l,
        None => return direct_link_error_response(StatusCode::NOT_FOUND, "NOT_FOUND", "直链不存在或已被删除"),
    };
    
    // 检查是否启用
    if !enabled {
        tracing::warn!("直链已禁用: {}", sign);
        return direct_link_error_response(StatusCode::FORBIDDEN, "DISABLED", "直链已被禁用");
    }
    
    // 检查是否过期
    if let Some(ref expires) = expires_at {
        // 尝试多种日期格式解析
        let is_expired = if let Ok(expires_time) = chrono::DateTime::parse_from_rfc3339(expires) {
            expires_time < chrono::Utc::now()
        } else if let Ok(expires_time) = chrono::NaiveDateTime::parse_from_str(expires, "%Y-%m-%dT%H:%M:%S") {
            expires_time < chrono::Utc::now().naive_utc()
        } else if let Ok(expires_time) = chrono::NaiveDateTime::parse_from_str(expires, "%Y-%m-%dT%H:%M") {
            expires_time < chrono::Utc::now().naive_utc()
        } else {
            tracing::warn!("无法解析过期时间: {}", expires);
            false
        };
        
        if is_expired {
            tracing::warn!("直链已过期: {} (expires_at={})", sign, expires);
            return direct_link_error_response(StatusCode::GONE, "EXPIRED", "直链已过期");
        }
    }
    
    // 检查访问次数
    if let Some(max_count) = max_access_count {
        if access_count >= max_count {
            tracing::warn!("直链访问次数已达上限: {}", sign);
            return direct_link_error_response(StatusCode::GONE, "EXHAUSTED", "直链访问次数已达上限");
        }
    }
    
    // 更新访问次数
    if let Err(_) = sqlx::query("UPDATE direct_links SET access_count = access_count + 1, updated_at = ? WHERE id = ?")
        .bind(chrono::Utc::now().to_rfc3339())
        .bind(link_id)
        .execute(&state.db)
        .await {
        return direct_link_error_response(StatusCode::INTERNAL_SERVER_ERROR, "SERVER_ERROR", "服务器内部错误");
    }
    
    // 使用file_resolver的多源聚合选择驱动（支持地区分流）
    let selected = match select_driver_for_download_with_ip(&state, &path, client_ip).await {
        Some(s) => s,
        None => return direct_link_error_response(StatusCode::SERVICE_UNAVAILABLE, "DRIVER_ERROR", "存储驱动故障"),
    };
    
    let actual_path = selected.internal_path;
    let driver = match state.storage_manager.get_driver(&selected.driver_id).await {
        Some(d) => d,
        None => return direct_link_error_response(StatusCode::SERVICE_UNAVAILABLE, "DRIVER_ERROR", "存储驱动故障"),
    };
    
    let filename = actual_path.split('/').last().unwrap_or("file");
    let filename_encoded = urlencoding::encode(filename);
    let content_type = mime_guess::from_path(&actual_path)
        .first_or_octet_stream()
        .to_string();
    
    // 获取文件大小（通过列出父目录找到文件）
    let parent_path = std::path::Path::new(&actual_path)
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| "/".to_string());
    let file_size: Option<u64> = match driver.list(&parent_path).await {
        Ok(entries) => entries.iter()
            .find(|e| e.name == filename)
            .map(|e| e.size),
        Err(_) => None,
    };
    
    // 获取直链创建者ID用于流量统计
    let link_user_id: Option<Option<String>> = sqlx::query_scalar(
        "SELECT user_id FROM direct_links WHERE sign = ?"
    )
    .bind(&sign)
    .fetch_optional(&state.db)
    .await
    .ok()
    .flatten();
    let link_user_id = link_user_id.flatten();
    
    // 如果驱动支持直链且启用302重定向，返回302（或JSON格式URL）
    tracing::debug!("dlink: can_direct_link={}, path={}, url_only={}", selected.can_direct_link, actual_path, url_only);
    if selected.can_direct_link {
        match driver.get_direct_link(&actual_path).await {
            Ok(Some(direct_url)) => {
                // 如果请求url_only，返回JSON格式的URL（供下载器使用）
                if url_only {
                    tracing::info!("dlink: 返回JSON格式URL url={}", direct_url);
                    let body = serde_json::json!({
                        "code": 200,
                        "url": direct_url
                    });
                    return Response::builder()
                        .status(StatusCode::OK)
                        .header(header::CONTENT_TYPE, "application/json; charset=utf-8")
                        .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
                        .body(Body::from(body.to_string()))
                        .unwrap();
                }
                
                // 302重定向时统计整个文件大小的流量
                if let Some(ref uid) = link_user_id {
                    stats::record_download(&state.db, uid, file_size).await;
                }
                
                tracing::info!("dlink: 302重定向到直链 url={}", direct_url);
                return Response::builder()
                    .status(StatusCode::FOUND)
                    .header(header::LOCATION, &direct_url)
                    .header("Referrer-Policy", "no-referrer")
                    .header(header::CACHE_CONTROL, "max-age=0, no-cache, no-store, must-revalidate")
                    .body(Body::empty())
                    .unwrap();
            }
            Ok(None) => {
                tracing::warn!("dlink: get_direct_link returned None, path={}", actual_path);
            }
            Err(e) => {
                tracing::warn!("dlink: get_direct_link failed: {}, path={}", e, actual_path);
            }
        }
    }
    
    // 解析Range请求头
    let range_header = headers.get(header::RANGE)
        .and_then(|v| v.to_str().ok());
    
    // 如果有文件大小且有Range请求，处理部分内容
    if let (Some(size), Some(range_str)) = (file_size, range_header) {
        if let Some((start, end)) = parse_range_header(Some(range_str), size) {
            let content_length = end - start + 1;
            
            // 使用带Range的流式接口
            let reader = match driver.open_reader(&actual_path, Some(start..(end + 1))).await {
                Ok(r) => r,
                Err(e) => {
                    tracing::error!("Direct link file read failed: {}", e);
                    return direct_link_error_response(StatusCode::SERVICE_UNAVAILABLE, "DRIVER_ERROR", "存储驱动故障");
                }
            };
            
            let stream = ReaderStream::new(reader);
            // 包装流量统计（本地中转统计实际传输流量）
            let stream = TrafficCountingStream::new(stream, link_user_id.clone(), state.db.clone());
            // Apply global bandwidth limiting / 应用全局带宽限制
            let max_speed = state.download_settings.get_max_speed();
            let body = if max_speed > 0 {
                let limiter = state.download_settings.get_limiter();
                Body::from_stream(ThrottledStream::new(stream, limiter))
            } else {
                Body::from_stream(stream)
            };
            
            return Response::builder()
                .status(StatusCode::PARTIAL_CONTENT)
                .header(header::CONTENT_TYPE, &content_type)
                .header(header::ACCEPT_RANGES, "bytes")
                .header(header::CONTENT_RANGE, format!("bytes {}-{}/{}", start, end, size))
                .header(header::CONTENT_LENGTH, content_length)
                .header(header::CONTENT_DISPOSITION, format!("inline; filename=\"{}\"; filename*=UTF-8''{}", filename, filename_encoded))
                .body(body)
                .unwrap();
        }
    }
    
    // 完整文件下载（使用流式接口，不读取到内存）
    let reader = match driver.open_reader(&actual_path, None).await {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("Direct link file read failed: {}", e);
            return direct_link_error_response(StatusCode::SERVICE_UNAVAILABLE, "DRIVER_ERROR", "存储驱动故障");
        }
    };
    
    let stream = ReaderStream::new(reader);
    // 包装流量统计（本地中转统计实际传输流量）
    let stream = TrafficCountingStream::new(stream, link_user_id.clone(), state.db.clone());
    // Apply global bandwidth limiting / 应用全局带宽限制
    let max_speed = state.download_settings.get_max_speed();
    let body = if max_speed > 0 {
        let limiter = state.download_settings.get_limiter();
        Body::from_stream(ThrottledStream::new(stream, limiter))
    } else {
        Body::from_stream(stream)
    };
    
    let mut response = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type)
        .header(header::ACCEPT_RANGES, "bytes")
        .header(header::CONTENT_DISPOSITION, format!("inline; filename=\"{}\"; filename*=UTF-8''{}", filename, filename_encoded));
    
    if let Some(size) = file_size {
        response = response.header(header::CONTENT_LENGTH, size);
    }
    
    response.body(body).unwrap()
}
