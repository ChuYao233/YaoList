//! 迅雷云盘 HTTP 客户端和认证

use anyhow::{anyhow, Result};
use md5;
use reqwest::Client;
use serde::Deserialize;
use sha1::{Sha1, Digest};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

use super::types::*;

/// 迅雷客户端
pub struct ThunderClient {
    pub client: Client,
    pub device_id: String,
    pub captcha_token: Arc<RwLock<String>>,
    pub credit_key: Arc<RwLock<String>>,
    pub token: Arc<RwLock<Option<TokenResp>>>,
    // 短信验证相关
    pub sms_token: Arc<RwLock<String>>,
    pub user_id: Arc<RwLock<String>>,
    pub mobile: Arc<RwLock<String>>,
}

impl ThunderClient {
    pub fn new(device_id: String, captcha_token: String, credit_key: String) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(300))
            .build()
            .unwrap();

        Self {
            client,
            device_id,
            captcha_token: Arc::new(RwLock::new(captcha_token)),
            credit_key: Arc::new(RwLock::new(credit_key)),
            token: Arc::new(RwLock::new(None)),
            sms_token: Arc::new(RwLock::new(String::new())),
            user_id: Arc::new(RwLock::new(String::new())),
            mobile: Arc::new(RwLock::new(String::new())),
        }
    }

    /// 解析 reviewurl 获取参数
    fn parse_review_url(&self, url: &str) -> std::collections::HashMap<String, String> {
        let mut params = std::collections::HashMap::new();
        if let Some(query) = url.split('?').nth(1) {
            for part in query.split('&') {
                if let Some((k, v)) = part.split_once('=') {
                    params.insert(
                        k.to_string(),
                        urlencoding::decode(v).unwrap_or_default().to_string()
                    );
                }
            }
        }
        params
    }

    /// 生成设备签名
    pub fn generate_device_sign(&self) -> String {
        let base = format!("{}{}{}{}", self.device_id, DEFAULT_PACKAGE_NAME, APPID, APP_KEY);
        let sha1_result = Sha1::digest(base.as_bytes());
        let sha1_hex = hex::encode(sha1_result);
        let md5_hex = format!("{:x}", md5::compute(sha1_hex.as_bytes()));
        format!("div101.{}{}", self.device_id, md5_hex)
    }

    /// 获取验证码签名
    pub fn get_captcha_sign(&self) -> (String, String) {
        let timestamp = chrono::Utc::now().timestamp_millis().to_string();
        let mut str = format!(
            "{}{}{}{}{}",
            DEFAULT_CLIENT_ID, DEFAULT_CLIENT_VERSION, DEFAULT_PACKAGE_NAME,
            self.device_id, timestamp
        );
        for alg in DEFAULT_ALGORITHMS {
            str = format!("{:x}", md5::compute(format!("{}{}", str, alg).as_bytes()));
        }
        (timestamp, format!("1.{}", str))
    }

    /// 刷新验证码 token
    pub async fn refresh_captcha_token(&self, action: &str, metas: HashMap<String, String>) -> Result<()> {
        let captcha_token = self.captcha_token.read().await.clone();
        let param = CaptchaTokenRequest {
            action: action.to_string(),
            captcha_token,
            client_id: DEFAULT_CLIENT_ID.to_string(),
            device_id: self.device_id.clone(),
            meta: metas,
            redirect_uri: "xlaccsdk01://xunlei.com/callback?state=harbor".to_string(),
        };

        let url = format!("{}/shield/captcha/init", XLUSER_API_URL);
        let resp = self.client
            .post(&url)
            .header("user-agent", DEFAULT_USER_AGENT)
            .header("accept", "application/json;charset=UTF-8")
            .header("x-device-id", &self.device_id)
            .header("x-client-id", DEFAULT_CLIENT_ID)
            .header("x-client-version", DEFAULT_CLIENT_VERSION)
            .json(&param)
            .send()
            .await?;

        let resp: CaptchaTokenResponse = resp.json().await?;

        if !resp.url.is_empty() {
            return Err(anyhow!("需要验证: {}", resp.url));
        }

        if resp.captcha_token.is_empty() {
            return Err(anyhow!("获取验证码token失败"));
        }

        *self.captcha_token.write().await = resp.captcha_token;
        Ok(())
    }

    /// Core 登录获取 sessionID
    pub async fn core_login(&self, username: &str, password: &str) -> Result<String> {
        let url = format!("{}/xluser.core.login/v3/login", XLUSER_API_BASE_URL);
        let credit_key = self.credit_key.read().await.clone();

        let req = CoreLoginRequest {
            protocol_version: "301".to_string(),
            sequence_no: "1000012".to_string(),
            platform_version: "10".to_string(),
            is_compressed: "0".to_string(),
            appid: APPID.to_string(),
            client_version: DEFAULT_CLIENT_VERSION.to_string(),
            peer_id: "00000000000000000000000000000000".to_string(),
            app_name: "ANDROID-com.xunlei.downloadprovider".to_string(),
            sdk_version: "512000".to_string(),
            devicesign: self.generate_device_sign(),
            network_type: "WIFI".to_string(),
            provider_name: "NONE".to_string(),
            device_model: "M2004J7AC".to_string(),
            device_name: "Xiaomi_M2004j7ac".to_string(),
            os_version: "12".to_string(),
            creditkey: credit_key,
            hl: "zh-CN".to_string(),
            user_name: username.to_string(),
            pass_word: password.to_string(),
            verify_key: String::new(),
            verify_code: String::new(),
            is_md5_pwd: "0".to_string(),
        };

        let resp = self.client
            .post(&url)
            .header("User-Agent", "android-ok-http-client/xl-acc-sdk/version-5.0.12.512000")
            .json(&req)
            .send()
            .await?;

        let text = resp.text().await?;
        let resp: CoreLoginResp = serde_json::from_str(&text)?;

        // 需要短信验证 - 自动发送验证码
        if resp.error == "review_panel" {
            // 解析 reviewurl 获取参数
            let params = self.parse_review_url(&resp.reviewurl);
            let user_id = params.get("userID").cloned().unwrap_or_default();
            let mobile = params.get("mobile").cloned().unwrap_or_default();
            
            // 保存信息用于后续验证
            *self.user_id.write().await = user_id.clone();
            *self.mobile.write().await = mobile.clone();
            *self.credit_key.write().await = resp.creditkey.clone();
            
            // 自动发送短信验证码
            match self.send_sms(&resp.creditkey, &user_id, &mobile).await {
                Ok(token) => {
                    *self.sms_token.write().await = token;
                    return Err(anyhow!("需要短信验证码，验证码已发送到 {}，请在配置中填写验证码后重新启用驱动", mobile));
                }
                Err(e) => {
                    return Err(anyhow!("发送短信验证码失败: {}", e));
                }
            }
        }

        // error 为空或 "0" 表示成功
        if !resp.error.is_empty() && resp.error != "0" && resp.error != "success" {
            return Err(anyhow!("登录失败: {} - {} (error={})", resp.error_code, resp.error_desc, resp.error));
        }

        // 保存 credit_key
        if !resp.creditkey.is_empty() {
            *self.credit_key.write().await = resp.creditkey;
        }

        if resp.session_id.is_empty() {
            return Err(anyhow!("登录失败：未获取到 sessionID"));
        }

        Ok(resp.session_id)
    }

    /// 用户密码登录（支持验证码）
    pub async fn login(&self, username: &str, password: &str) -> Result<()> {
        self.login_with_sms(username, password, None).await
    }

    /// 用户密码登录（带验证码）
    pub async fn login_with_sms(&self, username: &str, password: &str, sms_code: Option<&str>) -> Result<()> {
        // 如果提供了验证码，先验证
        if let Some(code) = sms_code {
            if !code.is_empty() {
                self.check_sms(code).await?;
            }
        }

        // 1. Core 登录获取 sessionID
        let session_id = self.core_login(username, password).await?;

        // 2. 刷新验证码 token
        let action = "POST:/v1/auth/signin/token";
        let mut metas = HashMap::new();
        if username.contains('@') {
            metas.insert("email".to_string(), username.to_string());
        } else if username.len() >= 11 && username.len() <= 18 {
            metas.insert("phone_number".to_string(), username.to_string());
        } else {
            metas.insert("username".to_string(), username.to_string());
        }
        self.refresh_captcha_token(action, metas).await?;

        // 3. 获取 token
        let url = format!("{}/auth/signin/token", XLUSER_API_URL);
        let captcha_token = self.captcha_token.read().await.clone();

        let req = SignInRequest {
            client_id: DEFAULT_CLIENT_ID.to_string(),
            client_secret: DEFAULT_CLIENT_SECRET.to_string(),
            provider: SIGN_PROVIDER.to_string(),
            signin_token: session_id,
        };

        let resp = self.client
            .post(&url)
            .header("user-agent", DEFAULT_USER_AGENT)
            .header("accept", "application/json;charset=UTF-8")
            .header("x-device-id", &self.device_id)
            .header("x-client-id", DEFAULT_CLIENT_ID)
            .header("x-client-version", DEFAULT_CLIENT_VERSION)
            .header("X-Captcha-Token", &captcha_token)
            .json(&req)
            .send()
            .await?;

        let token: TokenResp = resp.json().await?;

        if token.access_token.is_empty() {
            return Err(anyhow!("登录失败：未获取到 access_token"));
        }

        *self.token.write().await = Some(token);
        Ok(())
    }

    /// 发送短信验证码
    pub async fn send_sms(&self, creditkey: &str, user_id: &str, mobile: &str) -> Result<String> {
        let device_sign = self.generate_device_sign();
        let timestamp = chrono::Utc::now().timestamp_millis().to_string();
        
        let url = format!(
            "{}/xluser.core.login/v3/sendsms?deviceModel=NONE&deviceName=NONE&OSVersion=NONE&providerName=NONE&netWorkType=NONE&protocolVersion=301&devicesign={}&platformVersion=10&fromPlatformVersion=1&timestamp={}&sdkVersion=1.0&clientVersion={}&appid={}&event=login3&smsEvent=34&mainHost=xluser-ssl.xunlei.com,xluser2-ssl.xunlei.com,xluser3-ssl.xunlei.com&mobile={}&userID={}&creditkey={}&appName=ANDROID-com.xunlei.downloadprovider&trigger=0",
            XLUSER_API_BASE_URL, device_sign, timestamp, DEFAULT_CLIENT_VERSION, APPID, mobile, user_id, creditkey
        );

        let resp = self.client
            .get(&url)
            .header("user-agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
            .header("referer", "https://i.xunlei.com/")
            .send()
            .await?;

        let text = resp.text().await?;
        // 响应可能是 JSONP 格式，需要解析
        let json_str = if text.contains("({") {
            text.split("({").nth(1)
                .and_then(|s| s.rsplit("})").nth(1))
                .map(|s| format!("{{{}}}", s))
                .unwrap_or(text.clone())
        } else {
            text.clone()
        };

        let sms_resp: SmsResponse = serde_json::from_str(&json_str)
            .map_err(|e| anyhow!("解析短信响应失败: {} - {}", e, text))?;

        if sms_resp.error != "success" && sms_resp.error_code != "0" {
            return Err(anyhow!("发送短信失败: {} - {}", sms_resp.error_code, sms_resp.error_desc));
        }

        // 更新 creditkey
        if !sms_resp.creditkey.is_empty() {
            *self.credit_key.write().await = sms_resp.creditkey;
        }

        Ok(sms_resp.token)
    }

    /// 验证短信验证码
    pub async fn check_sms(&self, sms_code: &str) -> Result<String> {
        let device_sign = self.generate_device_sign();
        let timestamp = chrono::Utc::now().timestamp_millis().to_string();
        let creditkey = self.credit_key.read().await.clone();
        let user_id = self.user_id.read().await.clone();
        let mobile = self.mobile.read().await.clone();
        let sms_token = self.sms_token.read().await.clone();

        let form = [
            ("deviceModel", "NONE"),
            ("deviceName", "NONE"),
            ("OSVersion", "NONE"),
            ("providerName", "NONE"),
            ("netWorkType", "NONE"),
            ("protocolVersion", "301"),
            ("devicesign", &device_sign),
            ("platformVersion", "10"),
            ("fromPlatformVersion", "1"),
            ("timestamp", &timestamp),
            ("sdkVersion", "1.0"),
            ("clientVersion", DEFAULT_CLIENT_VERSION),
            ("appid", APPID),
            ("event", "login3"),
            ("smsEvent", "34"),
            ("mainHost", "xluser-ssl.xunlei.com,xluser2-ssl.xunlei.com,xluser3-ssl.xunlei.com"),
            ("mobile", &mobile),
            ("userID", &user_id),
            ("creditkey", &creditkey),
            ("appName", "ANDROID-com.xunlei.downloadprovider"),
            ("trigger", "0"),
            ("token", &sms_token),
            ("smsCode", sms_code),
        ];

        let url = format!("{}/xluser.core.login/v3/checksms", XLUSER_API_BASE_URL);
        let resp = self.client
            .post(&url)
            .header("user-agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
            .header("referer", "https://i.xunlei.com/")
            .header("origin", "https://i.xunlei.com")
            .form(&form)
            .send()
            .await?;

        let text = resp.text().await?;
        
        // checksms 成功时返回空响应，creditkey 在 header 中或需要重新登录
        if text.is_empty() || text == "{}" {
            // 验证成功，返回当前 creditkey
            return Ok(creditkey);
        }

        let check_resp: CheckSmsResponse = serde_json::from_str(&text)
            .map_err(|e| anyhow!("解析验证响应失败: {} - {}", e, text))?;

        if check_resp.error != "success" && check_resp.error_code != "0" && !check_resp.error.is_empty() {
            return Err(anyhow!("验证失败: {} - {}", check_resp.error_code, check_resp.error_desc));
        }

        // 返回新的 creditkey
        if !check_resp.creditkey.is_empty() {
            *self.credit_key.write().await = check_resp.creditkey.clone();
            Ok(check_resp.creditkey)
        } else {
            Ok(creditkey)
        }
    }

    /// 设置 refresh_token（从持久化配置恢复）
    pub async fn set_refresh_token(&self, refresh_token: &str) {
        let token = TokenResp {
            access_token: String::new(),
            refresh_token: refresh_token.to_string(),
            token_type: "Bearer".to_string(),
            expires_in: 0,
            user_id: String::new(),
        };
        *self.token.write().await = Some(token);
    }
    
    /// 获取当前 refresh_token
    pub async fn get_refresh_token(&self) -> Option<String> {
        let t = self.token.read().await;
        t.as_ref().map(|t| t.refresh_token.clone())
    }

    /// 刷新 token
    pub async fn refresh_token(&self) -> Result<()> {
        let refresh_token = {
            let t = self.token.read().await;
            t.as_ref().map(|t| t.refresh_token.clone()).unwrap_or_default()
        };

        if refresh_token.is_empty() {
            return Err(anyhow!("没有 refresh_token"));
        }

        let url = format!("{}/auth/token", XLUSER_API_URL);
        let body = serde_json::json!({
            "grant_type": "refresh_token",
            "refresh_token": refresh_token,
            "client_id": DEFAULT_CLIENT_ID,
            "client_secret": DEFAULT_CLIENT_SECRET,
        });

        let resp = self.client
            .post(&url)
            .header("user-agent", DEFAULT_USER_AGENT)
            .header("x-device-id", &self.device_id)
            .header("x-client-id", DEFAULT_CLIENT_ID)
            .header("x-client-version", DEFAULT_CLIENT_VERSION)
            .json(&body)
            .send()
            .await?;

        let token: TokenResp = resp.json().await?;

        if token.refresh_token.is_empty() {
            return Err(anyhow!("刷新 token 失败"));
        }

        *self.token.write().await = Some(token);
        Ok(())
    }

    /// 带认证的 API 请求
    pub async fn request<T: for<'de> Deserialize<'de>>(
        &self,
        url: &str,
        method: reqwest::Method,
        body: Option<serde_json::Value>,
    ) -> Result<T> {
        let (token_str, user_id) = {
            let t = self.token.read().await;
            match t.as_ref() {
                Some(t) => (t.token(), t.user_id.clone()),
                None => return Err(anyhow!("未登录")),
            }
        };
        let captcha_token = self.captcha_token.read().await.clone();

        let mut req = self.client
            .request(method.clone(), url)
            .header("user-agent", DEFAULT_USER_AGENT)
            .header("accept", "application/json;charset=UTF-8")
            .header("x-device-id", &self.device_id)
            .header("x-client-id", DEFAULT_CLIENT_ID)
            .header("x-client-version", DEFAULT_CLIENT_VERSION)
            .header("Authorization", &token_str)
            .header("X-Captcha-Token", &captcha_token);

        if let Some(b) = &body {
            req = req.json(b);
        }

        let resp = req.send().await?;
        let text = resp.text().await?;

        // 检查错误
        if let Ok(err) = serde_json::from_str::<ErrResp>(&text) {
            if err.is_error() {
                let code = err.error_code;
                // Token 过期
                if code == 4122 || code == 4121 || code == 10 || code == 16 {
                    self.refresh_token().await?;
                    return Box::pin(self.request(url, method, body)).await;
                }
                // 验证码 token 过期
                if code == 9 {
                    let (ts, sign) = self.get_captcha_sign();
                    let action = format!("{}:{}", method.as_str(), url.split("://").nth(1).and_then(|s| s.find('/').map(|i| &s[i..])).unwrap_or(""));
                    let mut metas = HashMap::new();
                    metas.insert("client_version".to_string(), DEFAULT_CLIENT_VERSION.to_string());
                    metas.insert("package_name".to_string(), DEFAULT_PACKAGE_NAME.to_string());
                    metas.insert("user_id".to_string(), user_id);
                    metas.insert("timestamp".to_string(), ts);
                    metas.insert("captcha_sign".to_string(), sign);
                    self.refresh_captcha_token(&action, metas).await?;
                    return Box::pin(self.request(url, method, body)).await;
                }
                return Err(anyhow!("API 错误: {}", err.message()));
            }
        }

        serde_json::from_str(&text).map_err(|e| anyhow!("解析响应失败: {} - {}", e, &text[..text.len().min(200)]))
    }

    /// GET 请求
    pub async fn get<T: for<'de> Deserialize<'de>>(&self, url: &str) -> Result<T> {
        self.request(url, reqwest::Method::GET, None).await
    }

    /// POST 请求
    pub async fn post<T: for<'de> Deserialize<'de>>(&self, url: &str, body: serde_json::Value) -> Result<T> {
        self.request(url, reqwest::Method::POST, Some(body)).await
    }
}
