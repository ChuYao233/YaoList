use axum::{
    extract::Query,
    http::StatusCode,
    response::{Html, IntoResponse},
    routing::get,
    Router,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use reqwest::Client;
use anyhow::{Result, anyhow};

#[derive(Debug, Deserialize)]
pub struct AuthCallback {
    pub code: Option<String>,
    pub error: Option<String>,
    pub error_description: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in: u64,
    pub token_type: String,
}

#[derive(Debug, Deserialize)]
pub struct TokenError {
    pub error: String,
    pub error_description: String,
}

#[derive(Clone)]
pub struct OneDriveOAuth {
    client: Client,
}

impl OneDriveOAuth {
    pub fn new() -> Self {
        Self {
            client: Client::new(),
        }
    }

    /// 生成 OAuth 授权 URL
    pub fn get_auth_url(
        &self,
        client_id: &str,
        redirect_uri: &str,
        region: &str,
    ) -> String {
        let oauth_host = match region {
            "cn" => "https://login.chinacloudapi.cn",
            "us" => "https://login.microsoftonline.us", 
            "de" => "https://login.microsoftonline.de",
            _ => "https://login.microsoftonline.com", // global
        };

        let scope = "https://graph.microsoft.com/Files.ReadWrite.All offline_access";
        
        format!(
            "{}/common/oauth2/v2.0/authorize?client_id={}&response_type=code&redirect_uri={}&scope={}&response_mode=query",
            oauth_host,
            urlencoding::encode(client_id),
            urlencoding::encode(redirect_uri),
            urlencoding::encode(scope)
        )
    }

    /// 使用授权码获取访问令牌
    pub async fn exchange_code_for_token(
        &self,
        code: &str,
        client_id: &str,
        client_secret: &str,
        redirect_uri: &str,
        region: &str,
    ) -> Result<TokenResponse> {
        let oauth_host = match region {
            "cn" => "https://login.chinacloudapi.cn",
            "us" => "https://login.microsoftonline.us",
            "de" => "https://login.microsoftonline.de", 
            _ => "https://login.microsoftonline.com", // global
        };

        let url = format!("{}/common/oauth2/v2.0/token", oauth_host);

        let mut params = HashMap::new();
        params.insert("grant_type", "authorization_code");
        params.insert("client_id", client_id);
        params.insert("client_secret", client_secret);
        params.insert("redirect_uri", redirect_uri);
        params.insert("code", code);

        let response = self.client
            .post(&url)
            .form(&params)
            .send()
            .await?;

        if response.status().is_success() {
            let token_resp: TokenResponse = response.json().await?;
            Ok(token_resp)
        } else {
            let error: TokenError = response.json().await?;
            Err(anyhow!("Token exchange failed: {}", error.error_description))
        }
    }
}

/// OAuth 回调处理器
#[axum::debug_handler]
pub async fn oauth_callback(
    Query(params): Query<AuthCallback>,
) -> impl IntoResponse {
    if let Some(error) = params.error {
        let error_desc = params.error_description.unwrap_or_else(|| "Unknown error".to_string());
        return Html(format!(
            r#"
            <!DOCTYPE html>
            <html>
            <head>
                <title>OneDrive 授权失败</title>
                <meta charset="utf-8">
                <style>
                    body {{ font-family: Arial, sans-serif; margin: 50px; }}
                    .error {{ color: red; }}
                    .container {{ max-width: 600px; margin: 0 auto; }}
                </style>
            </head>
            <body>
                <div class="container">
                    <h1>OneDrive 授权失败</h1>
                    <p class="error">错误: {}</p>
                    <p class="error">描述: {}</p>
                    <p>请返回应用程序重新尝试授权。</p>
                </div>
            </body>
            </html>
            "#,
            error, error_desc
        )).into_response();
    }

    if let Some(code) = params.code {
        Html(format!(
            r#"
            <!DOCTYPE html>
            <html>
            <head>
                <title>OneDrive 授权成功</title>
                <meta charset="utf-8">
                <style>
                    body {{ font-family: Arial, sans-serif; margin: 50px; }}
                    .success {{ color: green; }}
                    .code {{ 
                        background: #f5f5f5; 
                        padding: 10px; 
                        border-radius: 5px; 
                        font-family: monospace;
                        word-break: break-all;
                        margin: 10px 0;
                    }}
                    .container {{ max-width: 600px; margin: 0 auto; }}
                    .copy-btn {{
                        background: #007cba;
                        color: white;
                        border: none;
                        padding: 8px 16px;
                        border-radius: 4px;
                        cursor: pointer;
                        margin-left: 10px;
                    }}
                </style>
            </head>
            <body>
                <div class="container">
                    <h1 class="success">OneDrive 授权成功！</h1>
                    <p>请复制以下授权码到您的应用程序中：</p>
                    <div class="code" id="authCode">{}</div>
                    <button class="copy-btn" onclick="copyCode()">复制授权码</button>
                    <p><strong>注意：</strong>此授权码只能使用一次，请立即在应用程序中使用它来获取访问令牌。</p>
                </div>
                <script>
                    function copyCode() {{
                        const code = document.getElementById('authCode').textContent;
                        navigator.clipboard.writeText(code).then(function() {{
                            alert('授权码已复制到剪贴板！');
                        }}, function(err) {{
                            console.error('复制失败: ', err);
                            // 备用方法
                            const textArea = document.createElement('textarea');
                            textArea.value = code;
                            document.body.appendChild(textArea);
                            textArea.select();
                            document.execCommand('copy');
                            document.body.removeChild(textArea);
                            alert('授权码已复制到剪贴板！');
                        }});
                    }}
                </script>
            </body>
            </html>
            "#,
            code
        )).into_response()
    } else {
        Html(
            r#"
            <!DOCTYPE html>
            <html>
            <head>
                <title>OneDrive 授权错误</title>
                <meta charset="utf-8">
                <style>
                    body { font-family: Arial, sans-serif; margin: 50px; }
                    .error { color: red; }
                    .container { max-width: 600px; margin: 0 auto; }
                </style>
            </head>
            <body>
                <div class="container">
                    <h1>OneDrive 授权错误</h1>
                    <p class="error">未收到授权码，请重新尝试授权流程。</p>
                </div>
            </body>
            </html>
            "#
        ).into_response()
    }
}

/// 获取授权 URL 的处理器
#[axum::debug_handler]
async fn get_auth_url(
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let client_id = params.get("client_id").unwrap_or(&"".to_string()).clone();
    let redirect_uri = params.get("redirect_uri").unwrap_or(&"".to_string()).clone();
    let region = params.get("region").unwrap_or(&"global".to_string()).clone();

    if client_id.is_empty() || redirect_uri.is_empty() {
        return (StatusCode::BAD_REQUEST, "Missing client_id or redirect_uri").into_response();
    }

    let oauth = OneDriveOAuth::new();
    let auth_url = oauth.get_auth_url(&client_id, &redirect_uri, &region);
    
    Html(format!(
        r#"
        <!DOCTYPE html>
        <html>
        <head>
            <title>OneDrive 授权</title>
            <meta charset="utf-8">
            <style>
                body {{ font-family: Arial, sans-serif; margin: 50px; }}
                .container {{ max-width: 600px; margin: 0 auto; text-align: center; }}
                .auth-btn {{
                    background: #0078d4;
                    color: white;
                    padding: 12px 24px;
                    border: none;
                    border-radius: 4px;
                    font-size: 16px;
                    text-decoration: none;
                    display: inline-block;
                    margin: 20px 0;
                }}
                .auth-btn:hover {{
                    background: #106ebe;
                }}
            </style>
        </head>
        <body>
            <div class="container">
                <h1>OneDrive 授权</h1>
                <p>点击下面的按钮授权访问您的 OneDrive：</p>
                <a href="{}" class="auth-btn">授权 OneDrive 访问</a>
                <p><small>您将被重定向到 Microsoft 登录页面</small></p>
            </div>
        </body>
        </html>
        "#,
        auth_url
    )).into_response()
}

/// 创建 OAuth 路由
pub fn create_oauth_routes() -> Router {
    Router::new()
        .route("/onedrive/callback", get(oauth_callback))
        .route("/onedrive/auth", get(get_auth_url))
} 