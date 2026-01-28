//! OAuth 回调处理 API
//! 用于处理 Google Drive 等需要 OAuth 授权的驱动

use axum::{
    extract::Query,
    response::Html,
    routing::{get, post},
    Json, Router,
};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// OAuth 回调参数
#[derive(Debug, Deserialize)]
pub struct OAuthCallbackParams {
    pub code: Option<String>,
    pub error: Option<String>,
}

/// Token 交换请求
#[derive(Debug, Deserialize)]
pub struct TokenExchangeRequest {
    pub code: String,
    pub client_id: String,
    pub client_secret: String,
    pub redirect_uri: String,
}

/// Token 交换响应
#[derive(Debug, Serialize)]
pub struct TokenExchangeResponse {
    pub refresh_token: Option<String>,
    pub access_token: Option<String>,
    pub error: Option<String>,
}

/// Google OAuth token 响应
#[derive(Debug, Deserialize)]
struct GoogleTokenResponse {
    access_token: Option<String>,
    refresh_token: Option<String>,
    #[allow(dead_code)]
    expires_in: Option<u64>,
    #[allow(dead_code)]
    token_type: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

/// OAuth 回调页面 - 显示结果并通过 postMessage 传递给父窗口
pub async fn google_oauth_callback(Query(params): Query<OAuthCallbackParams>) -> Html<String> {
    let html = if let Some(error) = params.error {
        format!(r#"<!DOCTYPE html>
<html>
<head>
    <meta charset="utf-8">
    <title>授权失败</title>
    <style>
        body {{ font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; display: flex; justify-content: center; align-items: center; height: 100vh; margin: 0; background: #f5f5f5; }}
        .container {{ background: white; padding: 40px; border-radius: 12px; box-shadow: 0 2px 10px rgba(0,0,0,0.1); text-align: center; }}
        .error {{ color: #e53935; }}
    </style>
</head>
<body>
    <div class="container">
        <h2 class="error">授权失败</h2>
        <p>{}</p>
        <p>请关闭此窗口并重试</p>
    </div>
    <script>
        if (window.opener) {{
            window.opener.postMessage({{ type: 'oauth_error', error: '{}' }}, '*');
        }}
    </script>
</body>
</html>"#, error, error)
    } else if let Some(code) = params.code {
        format!(r#"<!DOCTYPE html>
<html>
<head>
    <meta charset="utf-8">
    <title>授权成功</title>
    <style>
        body {{ font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; display: flex; justify-content: center; align-items: center; height: 100vh; margin: 0; background: #f5f5f5; }}
        .container {{ background: white; padding: 40px; border-radius: 12px; box-shadow: 0 2px 10px rgba(0,0,0,0.1); text-align: center; max-width: 500px; }}
        .success {{ color: #43a047; }}
    </style>
</head>
<body>
    <div class="container">
        <h2 class="success">授权成功</h2>
        <p>正在获取令牌...</p>
    </div>
    <script>
        if (window.opener) {{
            window.opener.postMessage({{ type: 'oauth_code', code: '{}' }}, '*');
            setTimeout(() => window.close(), 1000);
        }}
    </script>
</body>
</html>"#, code)
    } else {
        r#"<!DOCTYPE html>
<html>
<head>
    <meta charset="utf-8">
    <title>授权错误</title>
    <style>
        body { font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; display: flex; justify-content: center; align-items: center; height: 100vh; margin: 0; background: #f5f5f5; }
        .container { background: white; padding: 40px; border-radius: 12px; box-shadow: 0 2px 10px rgba(0,0,0,0.1); text-align: center; }
        .error { color: #e53935; }
    </style>
</head>
<body>
    <div class="container">
        <h2 class="error">授权错误</h2>
        <p>未收到授权码或错误信息</p>
    </div>
</body>
</html>"#.to_string()
    };
    Html(html)
}

/// 用授权码换取 refresh_token
pub async fn exchange_token(
    Json(req): Json<TokenExchangeRequest>,
) -> Json<TokenExchangeResponse> {
    let client = Client::new();
    
    let mut params = HashMap::new();
    params.insert("code", req.code.as_str());
    params.insert("client_id", req.client_id.as_str());
    params.insert("client_secret", req.client_secret.as_str());
    params.insert("redirect_uri", req.redirect_uri.as_str());
    params.insert("grant_type", "authorization_code");

    let result = client
        .post("https://oauth2.googleapis.com/token")
        .form(&params)
        .send()
        .await;

    match result {
        Ok(resp) => {
            match resp.json::<GoogleTokenResponse>().await {
                Ok(token_resp) => {
                    if let Some(error) = token_resp.error {
                        let error_msg = token_resp.error_description
                            .unwrap_or(error);
                        Json(TokenExchangeResponse {
                            refresh_token: None,
                            access_token: None,
                            error: Some(error_msg),
                        })
                    } else {
                        Json(TokenExchangeResponse {
                            refresh_token: token_resp.refresh_token,
                            access_token: token_resp.access_token,
                            error: None,
                        })
                    }
                }
                Err(e) => Json(TokenExchangeResponse {
                    refresh_token: None,
                    access_token: None,
                    error: Some(format!("解析响应失败: {}", e)),
                }),
            }
        }
        Err(e) => Json(TokenExchangeResponse {
            refresh_token: None,
            access_token: None,
            error: Some(format!("请求失败: {}", e)),
        }),
    }
}

/// 阿里云盘 OAuth token 响应
#[derive(Debug, Deserialize)]
struct AliyunTokenResponse {
    access_token: Option<String>,
    refresh_token: Option<String>,
    #[allow(dead_code)]
    expires_in: Option<u64>,
    #[allow(dead_code)]
    token_type: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

/// 阿里云盘 OAuth 回调页面
pub async fn aliyun_oauth_callback(Query(params): Query<OAuthCallbackParams>) -> Html<String> {
    let html = if let Some(error) = params.error {
        format!(r#"<!DOCTYPE html>
<html>
<head>
    <meta charset="utf-8">
    <title>阿里云盘授权失败</title>
    <style>
        body {{ font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; display: flex; justify-content: center; align-items: center; height: 100vh; margin: 0; background: #f5f5f5; }}
        .container {{ background: white; padding: 40px; border-radius: 12px; box-shadow: 0 2px 10px rgba(0,0,0,0.1); text-align: center; }}
        .error {{ color: #e53935; }}
    </style>
</head>
<body>
    <div class="container">
        <h2 class="error">阿里云盘授权失败</h2>
        <p>{}</p>
        <p>请关闭此窗口并重试</p>
    </div>
    <script>
        if (window.opener) {{
            window.opener.postMessage({{ type: 'oauth_error', error: '{}' }}, '*');
        }}
    </script>
</body>
</html>"#, error, error)
    } else if let Some(code) = params.code {
        format!(r#"<!DOCTYPE html>
<html>
<head>
    <meta charset="utf-8">
    <title>阿里云盘授权成功</title>
    <style>
        body {{ font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; display: flex; justify-content: center; align-items: center; height: 100vh; margin: 0; background: #f5f5f5; }}
        .container {{ background: white; padding: 40px; border-radius: 12px; box-shadow: 0 2px 10px rgba(0,0,0,0.1); text-align: center; max-width: 500px; }}
        .success {{ color: #43a047; }}
    </style>
</head>
<body>
    <div class="container">
        <h2 class="success">阿里云盘授权成功</h2>
        <p>正在获取令牌...</p>
    </div>
    <script>
        if (window.opener) {{
            window.opener.postMessage({{ type: 'oauth_code', code: '{}' }}, '*');
            setTimeout(() => window.close(), 1000);
        }}
    </script>
</body>
</html>"#, code)
    } else {
        r#"<!DOCTYPE html>
<html>
<head>
    <meta charset="utf-8">
    <title>阿里云盘授权错误</title>
    <style>
        body { font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; display: flex; justify-content: center; align-items: center; height: 100vh; margin: 0; background: #f5f5f5; }
        .container { background: white; padding: 40px; border-radius: 12px; box-shadow: 0 2px 10px rgba(0,0,0,0.1); text-align: center; }
        .error { color: #e53935; }
    </style>
</head>
<body>
    <div class="container">
        <h2 class="error">阿里云盘授权错误</h2>
        <p>未收到授权码或错误信息</p>
    </div>
</body>
</html>"#.to_string()
    };
    Html(html)
}

/// 阿里云盘用授权码换取 refresh_token
pub async fn aliyun_exchange_token(
    Json(req): Json<TokenExchangeRequest>,
) -> Json<TokenExchangeResponse> {
    let client = Client::new();
    
    let mut params = HashMap::new();
    params.insert("code", req.code.as_str());
    params.insert("client_id", req.client_id.as_str());
    params.insert("client_secret", req.client_secret.as_str());
    params.insert("redirect_uri", req.redirect_uri.as_str());
    params.insert("grant_type", "authorization_code");

    let result = client
        .post("https://openapi.alipan.com/oauth/access_token")
        .form(&params)
        .send()
        .await;

    match result {
        Ok(resp) => {
            match resp.json::<AliyunTokenResponse>().await {
                Ok(token_resp) => {
                    if let Some(error) = token_resp.error {
                        let error_msg = token_resp.error_description
                            .unwrap_or(error);
                        Json(TokenExchangeResponse {
                            refresh_token: None,
                            access_token: None,
                            error: Some(error_msg),
                        })
                    } else {
                        Json(TokenExchangeResponse {
                            refresh_token: token_resp.refresh_token,
                            access_token: token_resp.access_token,
                            error: None,
                        })
                    }
                }
                Err(e) => Json(TokenExchangeResponse {
                    refresh_token: None,
                    access_token: None,
                    error: Some(format!("解析响应失败: {}", e)),
                }),
            }
        }
        Err(e) => Json(TokenExchangeResponse {
            refresh_token: None,
            access_token: None,
            error: Some(format!("请求失败: {}", e)),
        }),
    }
}

/// 创建 OAuth 路由
pub fn oauth_routes() -> Router {
    Router::new()
        .route("/google/callback", get(google_oauth_callback))
        .route("/google/exchange", post(exchange_token))
        .route("/aliyun/callback", get(aliyun_oauth_callback))
        .route("/aliyun/exchange", post(aliyun_exchange_token))
}
