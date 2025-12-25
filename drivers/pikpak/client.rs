//! PikPak HTTP client / PikPak HTTP客户端

use anyhow::{anyhow, Result};
use reqwest::{Client, Method, redirect};
use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use super::types::*;
use super::util::*;

/// Platform type / 平台类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Android,
    Web,
    Pc,
}

impl Default for Platform {
    fn default() -> Self {
        Platform::Web
    }
}

impl Platform {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "android" => Platform::Android,
            "pc" => Platform::Pc,
            _ => Platform::Web,
        }
    }

    pub fn to_str(&self) -> &'static str {
        match self {
            Platform::Android => "android",
            Platform::Web => "web",
            Platform::Pc => "pc",
        }
    }

    pub fn client_id(&self) -> &'static str {
        match self {
            Platform::Android => platform::android::CLIENT_ID,
            Platform::Web => platform::web::CLIENT_ID,
            Platform::Pc => platform::pc::CLIENT_ID,
        }
    }

    pub fn client_secret(&self) -> &'static str {
        match self {
            Platform::Android => platform::android::CLIENT_SECRET,
            Platform::Web => platform::web::CLIENT_SECRET,
            Platform::Pc => platform::pc::CLIENT_SECRET,
        }
    }

    pub fn client_version(&self) -> &'static str {
        match self {
            Platform::Android => platform::android::CLIENT_VERSION,
            Platform::Web => platform::web::CLIENT_VERSION,
            Platform::Pc => platform::pc::CLIENT_VERSION,
        }
    }

    pub fn package_name(&self) -> &'static str {
        match self {
            Platform::Android => platform::android::PACKAGE_NAME,
            Platform::Web => platform::web::PACKAGE_NAME,
            Platform::Pc => platform::pc::PACKAGE_NAME,
        }
    }

    pub fn sdk_version(&self) -> &'static str {
        match self {
            Platform::Android => platform::android::SDK_VERSION,
            Platform::Web => platform::web::SDK_VERSION,
            Platform::Pc => platform::pc::SDK_VERSION,
        }
    }

    pub fn algorithms(&self) -> &'static [&'static str] {
        match self {
            Platform::Android => platform::android::ALGORITHMS,
            Platform::Web => platform::web::ALGORITHMS,
            Platform::Pc => platform::pc::ALGORITHMS,
        }
    }
}

/// PikPak HTTP client / PikPak HTTP客户端
pub struct PikPakClient {
    pub client: Client,
    pub no_redirect_client: Client,
    pub platform: Platform,
    pub token_info: Arc<RwLock<TokenInfo>>,
}

impl PikPakClient {
    pub fn new(platform: Platform) -> Self {
        Self {
            client: Client::builder()
                .cookie_store(true)
                .redirect(redirect::Policy::limited(10))
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .unwrap(),
            no_redirect_client: Client::builder()
                .cookie_store(true)
                .redirect(redirect::Policy::none())
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .unwrap(),
            platform,
            token_info: Arc::new(RwLock::new(TokenInfo::default())),
        }
    }

    /// Initialize with existing token info / 使用现有令牌信息初始化
    pub fn init_token(&self, device_id: &str, refresh_token: &str, captcha_token: &str) {
        let mut info = self.token_info.write().unwrap();
        info.device_id = if device_id.is_empty() {
            uuid::Uuid::new_v4().to_string().replace("-", "")
        } else {
            device_id.to_string()
        };
        info.refresh_token = refresh_token.to_string();
        info.captcha_token = captcha_token.to_string();
    }

    /// Get user agent based on platform / 根据平台获取用户代理
    pub fn get_user_agent(&self) -> String {
        let info = self.token_info.read().unwrap();
        match self.platform {
            Platform::Android => build_android_user_agent(
                &info.device_id,
                self.platform.client_id(),
                self.platform.package_name(),
                self.platform.sdk_version(),
                self.platform.client_version(),
                self.platform.package_name(),
                &info.user_id,
            ),
            Platform::Web => build_web_user_agent(),
            Platform::Pc => build_pc_user_agent(),
        }
    }

    /// Get captcha sign / 获取验证码签名
    pub fn get_captcha_sign(&self) -> (i64, String) {
        let info = self.token_info.read().unwrap();
        let timestamp = chrono::Utc::now().timestamp_millis();
        let sign = generate_captcha_sign(
            self.platform.client_id(),
            self.platform.client_version(),
            self.platform.package_name(),
            &info.device_id,
            timestamp,
            self.platform.algorithms(),
        );
        (timestamp, sign)
    }

    /// Login with username and password / 使用用户名密码登录
    pub async fn login(&self, username: &str, password: &str) -> Result<()> {
        if username.is_empty() || password.is_empty() {
            return Err(anyhow!("Username or password is empty / 用户名或密码为空"));
        }

        let captcha_token = {
            let info = self.token_info.read().unwrap();
            info.captcha_token.clone()
        };

        if captcha_token.is_empty() {
            self.refresh_captcha_token_in_login(
                &get_action("POST", api::LOGIN_URL),
                username,
            ).await?;
        }

        let body = {
            let info = self.token_info.read().unwrap();
            json!({
                "captcha_token": info.captcha_token,
                "client_id": self.platform.client_id(),
                "client_secret": self.platform.client_secret(),
                "username": username,
                "password": password,
            })
        };

        let resp: LoginResp = self.client
            .post(api::LOGIN_URL)
            .query(&[("client_id", self.platform.client_id())])
            .json(&body)
            .send()
            .await?
            .json()
            .await?;

        if resp.error.is_error() {
            return Err(anyhow!("Login failed / 登录失败: {}", resp.error.error_message()));
        }

        {
            let mut info = self.token_info.write().unwrap();
            info.access_token = resp.access_token;
            info.refresh_token = resp.refresh_token;
            info.user_id = resp.sub;
        }

        Ok(())
    }

    /// Refresh access token / 刷新访问令牌
    pub async fn refresh_token(&self, refresh_token: &str) -> Result<()> {
        let token = if refresh_token.is_empty() {
            let info = self.token_info.read().unwrap();
            info.refresh_token.clone()
        } else {
            refresh_token.to_string()
        };

        if token.is_empty() {
            return Err(anyhow!("No refresh token / 没有refresh_token"));
        }

        let body = json!({
            "client_id": self.platform.client_id(),
            "client_secret": self.platform.client_secret(),
            "grant_type": "refresh_token",
            "refresh_token": token,
        });

        let resp: LoginResp = self.client
            .post(api::TOKEN_URL)
            .query(&[("client_id", self.platform.client_id())])
            .json(&body)
            .send()
            .await?
            .json()
            .await?;

        if resp.error.is_error() {
            if resp.error.error_code == 4126 {
                return Err(anyhow!("Refresh token invalid / refresh_token无效"));
            }
            return Err(anyhow!("Refresh failed / 刷新失败: {}", resp.error.error_message()));
        }

        {
            let mut info = self.token_info.write().unwrap();
            info.access_token = resp.access_token;
            info.refresh_token = resp.refresh_token;
            info.user_id = resp.sub;
        }

        Ok(())
    }

    /// Refresh captcha token (after login) / 刷新验证码令牌(登录后)
    pub async fn refresh_captcha_token_at_login(&self, action: &str, user_id: &str) -> Result<()> {
        let (timestamp, captcha_sign) = self.get_captcha_sign();
        let mut metas = HashMap::new();
        metas.insert("client_version".to_string(), self.platform.client_version().to_string());
        metas.insert("package_name".to_string(), self.platform.package_name().to_string());
        metas.insert("user_id".to_string(), user_id.to_string());
        metas.insert("timestamp".to_string(), timestamp.to_string());
        metas.insert("captcha_sign".to_string(), captcha_sign);

        self.refresh_captcha_token(action, metas).await
    }

    /// Refresh captcha token (during login) / 刷新验证码令牌(登录时)
    pub async fn refresh_captcha_token_in_login(&self, action: &str, username: &str) -> Result<()> {
        let mut metas = HashMap::new();
        if is_email(username) {
            metas.insert("email".to_string(), username.to_string());
        } else if is_phone_number(username) {
            metas.insert("phone_number".to_string(), username.to_string());
        } else {
            metas.insert("username".to_string(), username.to_string());
        }

        self.refresh_captcha_token(action, metas).await
    }

    /// Refresh captcha token internal / 刷新验证码令牌内部实现
    async fn refresh_captcha_token(&self, action: &str, metas: HashMap<String, String>) -> Result<()> {
        let req = {
            let info = self.token_info.read().unwrap();
            CaptchaTokenRequest {
                action: action.to_string(),
                captcha_token: info.captcha_token.clone(),
                client_id: self.platform.client_id().to_string(),
                device_id: info.device_id.clone(),
                meta: metas,
                redirect_uri: "xlaccsdk01://xbase.cloud/callback?state=harbor".to_string(),
            }
        };

        let resp: CaptchaTokenResp = self.client
            .post(api::CAPTCHA_URL)
            .query(&[("client_id", self.platform.client_id())])
            .json(&req)
            .send()
            .await?
            .json()
            .await?;

        if !resp.url.is_empty() {
            return Err(anyhow!("Need captcha verification / 需要验证码验证: {}", resp.url));
        }

        {
            let mut info = self.token_info.write().unwrap();
            info.captcha_token = resp.captcha_token;
        }

        Ok(())
    }

    /// Make authenticated request / 发送认证请求
    pub async fn request<T: DeserializeOwned>(
        &self,
        url: &str,
        method: Method,
        query: Option<Vec<(&str, String)>>,
        body: Option<Value>,
    ) -> Result<T> {
        // 在await之前提取所有需要的数据，然后释放锁
        let (device_id, captcha_token, access_token) = {
            let info = self.token_info.read().unwrap();
            (info.device_id.clone(), info.captcha_token.clone(), info.access_token.clone())
        };
        let user_agent = self.get_user_agent();
        
        let mut req = self.client
            .request(method.clone(), url)
            .header("User-Agent", &user_agent)
            .header("X-Device-ID", &device_id)
            .header("X-Captcha-Token", &captcha_token);

        if !access_token.is_empty() {
            req = req.header("Authorization", format!("Bearer {}", access_token));
        }

        if let Some(q) = query {
            req = req.query(&q);
        }

        if let Some(b) = body {
            req = req.json(&b);
        }

        let resp = req.send().await?;
        let status = resp.status();
        let text = resp.text().await?;

        if let Ok(err) = serde_json::from_str::<ErrResp>(&text) {
            if err.is_error() {
                match err.error_code {
                    4122 | 4121 | 16 => {
                        return Err(anyhow!("TOKEN_EXPIRED"));
                    }
                    9 => {
                        return Err(anyhow!("CAPTCHA_EXPIRED"));
                    }
                    10 => {
                        return Err(anyhow!("Rate limited / 操作频繁: {}", err.error_description));
                    }
                    _ => {
                        return Err(anyhow!("API error / API错误: {}", err.error_message()));
                    }
                }
            }
        }

        serde_json::from_str(&text)
            .map_err(|e| anyhow!("Parse response failed / 解析响应失败: {} - status: {} - body: {}", e, status, text))
    }

    /// Request with auto retry on token expiry / 带自动重试的请求
    pub async fn request_with_retry<T: DeserializeOwned>(
        &self,
        url: &str,
        method: Method,
        query: Option<Vec<(&str, String)>>,
        body: Option<Value>,
    ) -> Result<T> {
        match self.request::<T>(url, method.clone(), query.clone(), body.clone()).await {
            Ok(resp) => Ok(resp),
            Err(e) => {
                let err_str = e.to_string();
                if err_str.contains("TOKEN_EXPIRED") {
                    let refresh_token = {
                        let info = self.token_info.read().unwrap();
                        info.refresh_token.clone()
                    };
                    
                    self.refresh_token(&refresh_token).await?;
                    self.request(url, method, query, body).await
                } else if err_str.contains("CAPTCHA_EXPIRED") {
                    let user_id = {
                        let info = self.token_info.read().unwrap();
                        info.user_id.clone()
                    };
                    
                    self.refresh_captcha_token_at_login(&get_action(method.as_str(), url), &user_id).await?;
                    self.request(url, method, query, body).await
                } else {
                    Err(e)
                }
            }
        }
    }

    /// GET request / GET请求
    pub async fn get<T: DeserializeOwned>(&self, url: &str, query: Option<Vec<(&str, String)>>) -> Result<T> {
        self.request_with_retry(url, Method::GET, query, None).await
    }

    /// POST request / POST请求
    pub async fn post<T: DeserializeOwned>(&self, url: &str, body: Value) -> Result<T> {
        self.request_with_retry(url, Method::POST, None, Some(body)).await
    }

    /// PATCH request / PATCH请求
    pub async fn patch<T: DeserializeOwned>(&self, url: &str, body: Value) -> Result<T> {
        self.request_with_retry(url, Method::PATCH, None, Some(body)).await
    }

    /// DELETE request / DELETE请求
    pub async fn delete<T: DeserializeOwned>(&self, url: &str, query: Option<Vec<(&str, String)>>) -> Result<T> {
        self.request_with_retry(url, Method::DELETE, query, None).await
    }

    /// Get current token info / 获取当前令牌信息
    pub fn get_token_info(&self) -> TokenInfo {
        self.token_info.read().unwrap().clone()
    }

    /// Get download URL (302 redirect) / 获取下载链接(302重定向)
    pub async fn get_download_url(&self, file_id: &str, disable_media_link: bool) -> Result<String> {
        let query = if disable_media_link {
            vec![
                ("_magic", "2021".to_string()),
                ("usage", "FETCH".to_string()),
                ("thumbnail_size", "SIZE_LARGE".to_string()),
            ]
        } else {
            vec![
                ("_magic", "2021".to_string()),
                ("usage", "CACHE".to_string()),
                ("thumbnail_size", "SIZE_LARGE".to_string()),
            ]
        };

        let url = format!("{}/{}", api::FILES_URL, file_id);
        let resp: PikPakFile = self.get(&url, Some(query)).await?;

        if !disable_media_link && !resp.medias.is_empty() && !resp.medias[0].link.url.is_empty() {
            return Ok(resp.medias[0].link.url.clone());
        }

        Ok(resp.web_content_link)
    }
}
