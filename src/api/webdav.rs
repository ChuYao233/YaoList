//! WebDAV API处理器
//! 
//! 将WebDAV请求转发给dav-server处理
//! 支持Windows WebDAV客户端

use axum::{
    body::Body,
    extract::State,
    http::{Request, Method, StatusCode},
    response::Response,
};
use std::sync::Arc;
use base64::Engine;

use crate::state::AppState;
use yaolist_backend::server::{WebDavFs, UserAuthenticator};

/// WebDAV请求处理器
/// 处理所有/dav/*路径的请求
pub async fn webdav_handler(
    State(state): State<Arc<AppState>>,
    req: Request<Body>,
) -> Response<Body> {
    let method = req.method().clone();
    
    // OPTIONS请求不需要认证（Windows WebDAV客户端需要）
    if method == Method::OPTIONS {
        return Response::builder()
            .status(StatusCode::OK)
            .header("Allow", "OPTIONS, GET, HEAD, PUT, DELETE, PROPFIND, PROPPATCH, MKCOL, COPY, MOVE, LOCK, UNLOCK")
            .header("DAV", "1, 2")
            .header("MS-Author-Via", "DAV")
            .body(Body::empty())
            .unwrap();
    }

    // Basic Auth认证
    let auth_header = req.headers().get("Authorization");
    tracing::debug!("WebDAV request: {:?}, Authorization: {:?}", method, auth_header);
    
    let user = if let Some(auth) = auth_header {
        if let Ok(auth_str) = auth.to_str() {
            if auth_str.starts_with("Basic ") {
                let encoded = &auth_str[6..];
                if let Ok(decoded) = base64::engine::general_purpose::STANDARD.decode(encoded) {
                    if let Ok(creds) = String::from_utf8(decoded) {
                        if let Some((username, password)) = creds.split_once(':') {
                            tracing::info!("WebDAV auth attempt: user={}", username);
                            let authenticator = UserAuthenticator::new(state.db.clone());
                            let result = authenticator.authenticate_webdav(username, password).await;
                            if result.is_none() {
                                tracing::warn!("WebDAV auth failed: user={}", username);
                            }
                            result
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        }
    } else {
        None
    };

    // 未认证返回401
    let user = match user {
        Some(u) => {
            tracing::info!("WebDAV auth success: {}", u.username);
            u
        },
        None => {
            tracing::info!("WebDAV returning 401 requiring authentication");
            return Response::builder()
                .status(StatusCode::UNAUTHORIZED)
                .header("WWW-Authenticate", "Basic realm=\"YaoList\"")
                .header("DAV", "1, 2")
                .header("MS-Author-Via", "DAV")
                .header("Content-Type", "text/plain; charset=utf-8")
                .body(Body::from("Authentication required"))
                .unwrap();
        }
    };

    // 创建带用户的文件系统
    let fs = WebDavFs::with_user(
        state.storage_manager.clone(),
        state.db.clone(),
        user,
    );
    
    // 创建WebDAV处理器
    let handler = dav_server::DavHandler::builder()
        .filesystem(Box::new(fs))
        .locksystem(dav_server::fakels::FakeLs::new())
        .strip_prefix("/dav")
        .build_handler();

    // 转换请求类型并处理
    let (mut parts, body) = req.into_parts();
    
    // Windows WebDAV客户端可能不发送Depth头部，默认添加Depth: 1
    if parts.method.as_str() == "PROPFIND" && !parts.headers.contains_key("depth") {
        parts.headers.insert("depth", "1".parse().unwrap());
    }
    
    let hyper_req = hyper::Request::from_parts(parts, body);
    
    // 处理请求
    let response = handler.handle(hyper_req).await;
    
    // 转换响应类型并添加Windows兼容头
    let (mut parts, body) = response.into_parts();
    parts.headers.insert("MS-Author-Via", "DAV".parse().unwrap());
    Response::from_parts(parts, Body::new(body))
}
