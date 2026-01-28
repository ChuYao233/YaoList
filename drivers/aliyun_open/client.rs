//! 阿里云盘 Open HTTP 客户端和认证

use anyhow::{anyhow, Result};
use reqwest::Client;
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

use super::types::*;

/// 阿里云盘 Open 客户端
pub struct AliyunOpenClient {
    pub client: Client,
    pub access_token: Arc<RwLock<String>>,
    pub refresh_token: Arc<RwLock<String>>,
    pub client_id: String,
    pub client_secret: String,
}

impl AliyunOpenClient {
    pub fn new(
        client_id: String,
        client_secret: String,
        refresh_token: String,
    ) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(300))
            .build()
            .unwrap();

        Self {
            client,
            access_token: Arc::new(RwLock::new(String::new())),
            refresh_token: Arc::new(RwLock::new(refresh_token)),
            client_id,
            client_secret,
        }
    }

    /// 获取当前 access_token
    pub async fn get_access_token(&self) -> String {
        self.access_token.read().await.clone()
    }

    /// 获取当前 refresh_token
    pub async fn get_refresh_token(&self) -> String {
        self.refresh_token.read().await.clone()
    }

    /// 设置 tokens
    pub async fn set_tokens(&self, access_token: String, refresh_token: String) {
        *self.access_token.write().await = access_token;
        *self.refresh_token.write().await = refresh_token;
    }

    /// 刷新 token（仅使用官方 API）
    pub async fn refresh_token(&self) -> Result<(String, String)> {
        if self.client_id.is_empty() || self.client_secret.is_empty() {
            return Err(anyhow!("客户端 ID 或密钥为空"));
        }

        let refresh_token = self.get_refresh_token().await;
        let url = format!("{}/oauth/access_token", API_URL);

        let body = RefreshTokenRequest {
            client_id: self.client_id.clone(),
            client_secret: self.client_secret.clone(),
            grant_type: "refresh_token".to_string(),
            refresh_token,
        };

        let resp = self.client
            .post(&url)
            .json(&body)
            .send()
            .await?;

        let text = resp.text().await?;
        
        // 检查错误
        if let Ok(err) = serde_json::from_str::<ErrResp>(&text) {
            if !err.code.is_empty() {
                return Err(anyhow!("刷新 token 失败: {}", err.message));
            }
        }

        let json: Value = serde_json::from_str(&text)?;
        let new_refresh_token = json["refresh_token"]
            .as_str()
            .ok_or_else(|| anyhow!("响应中缺少 refresh_token"))?
            .to_string();
        let new_access_token = json["access_token"]
            .as_str()
            .ok_or_else(|| anyhow!("响应中缺少 access_token"))?
            .to_string();

        Ok((new_refresh_token, new_access_token))
    }

    /// 带认证的 API 请求
    pub async fn request<T>(&self, method: reqwest::Method, uri: &str, body: Option<Value>) -> Result<T>
    where
        T: for<'de> serde::Deserialize<'de>,
    {
        let mut retry_count = 0;
        const MAX_RETRIES: u32 = 3;

        loop {
            let access_token = self.get_access_token().await;
            let url = format!("{}{}", API_URL, uri);

            let mut req = self.client
                .request(method.clone(), &url)
                .header("Authorization", format!("Bearer {}", access_token))
                .header("Content-Type", "application/json");

            if let Some(ref b) = body {
                req = req.json(b);
            }

            let resp = req.send().await?;
            let text = resp.text().await?;

            // 检查错误
            if let Ok(err) = serde_json::from_str::<ErrResp>(&text) {
                if !err.code.is_empty() {
                    // Token 相关错误，尝试刷新
                    if (err.code == "AccessTokenInvalid" || err.code == "AccessTokenExpired" || err.code == "I400JD") 
                        && retry_count < MAX_RETRIES {
                        match self.refresh_token().await {
                            Ok((new_refresh, new_access)) => {
                                self.set_tokens(new_access, new_refresh).await;
                                retry_count += 1;
                                continue;
                            }
                            Err(e) => return Err(anyhow!("刷新 token 失败: {}", e)),
                        }
                    }
                    return Err(anyhow!("API 错误: {} - {}", err.code, err.message));
                }
            }

            // 解析成功响应
            return serde_json::from_str(&text)
                .map_err(|e| anyhow!("解析响应失败: {} - {}", e, &text[..text.len().min(200)]));
        }
    }

    /// GET 请求
    pub async fn get<T>(&self, uri: &str) -> Result<T>
    where
        T: for<'de> serde::Deserialize<'de>,
    {
        self.request(reqwest::Method::GET, uri, None).await
    }

    /// POST 请求
    pub async fn post<T>(&self, uri: &str, body: Value) -> Result<T>
    where
        T: for<'de> serde::Deserialize<'de>,
    {
        self.request(reqwest::Method::POST, uri, Some(body)).await
    }
}
