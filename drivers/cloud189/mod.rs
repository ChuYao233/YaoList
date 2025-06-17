use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use reqwest::{Client, header::HeaderMap, header::HeaderValue};
use serde::{Deserialize, Serialize};
use chrono::Utc;
use rsa::{RsaPublicKey, Pkcs1v15Encrypt};
use rsa::pkcs8::DecodePublicKey;
use hmac::{Hmac, Mac};
use sha1::Sha1;
use base64::Engine as _;
use uuid::Uuid;
use md5;
use digest::Digest;
use std::sync::Arc;
use tokio::sync::Mutex;
use futures::StreamExt;
use async_stream;
use regex::Regex;
use rand::Rng;

use crate::drivers::{Driver, FileInfo, DriverFactory, DriverInfo};

type HmacSha1 = Hmac<Sha1>;

const ACCOUNT_TYPE: &str = "02";
const APP_ID: &str = "8025431004";
const CLIENT_TYPE: &str = "10020";
const VERSION: &str = "6.2";
const WEB_URL: &str = "https://cloud.189.cn";
const AUTH_URL: &str = "https://open.e.189.cn";
const API_URL: &str = "https://api.cloud.189.cn";
const UPLOAD_URL: &str = "https://upload.cloud.189.cn";
const RETURN_URL: &str = "https://m.cloud.189.cn/zhuanti/2020/loginErrorPc/index.html";
const PC: &str = "TELEPC";
const CHANNEL_ID: &str = "web_cloud.189.cn";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Cloud189Config {
    pub username: String,
    pub password: String,
    pub validate_code: Option<String>,
    pub root_folder_id: String,
    pub order_by: String,
    pub order_direction: String,
    pub cloud_type: String, // personal or family
    pub family_id: Option<String>,
    pub upload_method: String, // stream, rapid, old
    pub upload_thread: u32,
    pub family_transfer: bool,
    pub rapid_upload: bool,
    pub no_use_ocr: bool,
}

impl Default for Cloud189Config {
    fn default() -> Self {
        Self {
            username: String::new(),
            password: String::new(),
            validate_code: None,
            root_folder_id: "-11".to_string(),
            order_by: "filename".to_string(),
            order_direction: "asc".to_string(),
            cloud_type: "personal".to_string(),
            family_id: None,
            upload_method: "stream".to_string(),
            upload_thread: 3,
            family_transfer: false,
            rapid_upload: false,
            no_use_ocr: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoginParam {
    pub rsa_username: String,
    pub rsa_password: String,
    pub j_rsa_key: String,
    pub lt: String,
    pub req_id: String,
    pub param_id: String,
    pub captcha_token: String,
}

#[derive(Debug, Deserialize)]
pub struct EncryptConfResp {
    pub result: i32,
    pub data: EncryptConfData,
}

#[derive(Debug, Deserialize)]
pub struct EncryptConfData {
    #[serde(rename = "upSmsOn")]
    pub up_sms_on: String,
    pub pre: String,
    #[serde(rename = "preDomain")]
    pub pre_domain: String,
    #[serde(rename = "pubKey")]
    pub pub_key: String,
}

#[derive(Debug, Deserialize)]
pub struct LoginResp {
    pub msg: String,
    pub result: i32,
    #[serde(rename = "toUrl")]
    pub to_url: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AppSessionResp {
    #[serde(rename = "res_code")]
    pub res_code: i32,
    #[serde(rename = "res_message")]
    pub res_message: String,
    #[serde(rename = "loginName")]
    pub login_name: String,
    #[serde(rename = "keepAlive")]
    pub keep_alive: i32,
    #[serde(rename = "sessionKey")]
    pub session_key: String,
    #[serde(rename = "sessionSecret")]
    pub session_secret: String,
    #[serde(rename = "familySessionKey")]
    pub family_session_key: Option<String>,
    #[serde(rename = "familySessionSecret")]
    pub family_session_secret: Option<String>,
    #[serde(rename = "accessToken")]
    pub access_token: String,
    #[serde(rename = "refreshToken")]
    pub refresh_token: String,
}

#[derive(Debug, Deserialize)]
pub struct Cloud189File {
    pub id: String,
    pub name: String,
    pub size: i64,
    pub md5: Option<String>,
    #[serde(rename = "lastOpTime")]
    pub last_op_time: String,
    #[serde(rename = "createDate")]
    pub create_date: String,
    pub icon: Option<Cloud189Icon>,
}

#[derive(Debug, Deserialize)]
pub struct Cloud189Icon {
    #[serde(rename = "smallUrl")]
    pub small_url: Option<String>,
    #[serde(rename = "largeUrl")]
    pub large_url: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct Cloud189Folder {
    pub id: String,
    #[serde(rename = "parentId")]
    pub parent_id: String,
    pub name: String,
    #[serde(rename = "lastOpTime")]
    pub last_op_time: String,
    #[serde(rename = "createDate")]
    pub create_date: String,
}

#[derive(Debug, Deserialize)]
pub struct Cloud189FilesResp {
    #[serde(rename = "fileListAO")]
    pub file_list_ao: FileListAO,
}

#[derive(Debug, Deserialize)]
pub struct FileListAO {
    pub count: i32,
    #[serde(rename = "fileList")]
    pub file_list: Vec<Cloud189File>,
    #[serde(rename = "folderList")]
    pub folder_list: Vec<Cloud189Folder>,
}

#[derive(Debug, Deserialize)]
pub struct DownloadUrlResp {
    #[serde(rename = "fileDownloadUrl")]
    pub file_download_url: String,
}

#[derive(Debug, Deserialize)]
pub struct RespErr {
    #[serde(rename = "res_code")]
    pub res_code: Option<serde_json::Value>,
    #[serde(rename = "res_message")]
    pub res_message: Option<String>,
    pub error: Option<String>,
    pub code: Option<String>,
    pub message: Option<String>,
    pub msg: Option<String>,
    #[serde(rename = "errorCode")]
    pub error_code: Option<String>,
    #[serde(rename = "errorMsg")]
    pub error_msg: Option<String>,
}

impl RespErr {
    pub fn has_error(&self) -> bool {
        if let Some(res_code) = &self.res_code {
            if let Some(code) = res_code.as_i64() {
                if code != 0 {
                    return true;
                }
            }
            if let Some(code_str) = res_code.as_str() {
                if !code_str.is_empty() && code_str != "0" {
                    return true;
                }
            }
        }
        
        self.error.is_some() ||
        self.code.is_some() ||
        self.error_code.is_some() ||
        (self.message.is_some() && !self.message.as_ref().unwrap().is_empty()) ||
        (self.msg.is_some() && !self.msg.as_ref().unwrap().is_empty()) ||
        (self.error_msg.is_some() && !self.error_msg.as_ref().unwrap().is_empty()) ||
        (self.res_message.is_some() && !self.res_message.as_ref().unwrap().is_empty())
    }
    
    pub fn error_message(&self) -> String {
        if let Some(res_code) = &self.res_code {
            if let Some(code) = res_code.as_i64() {
                if code != 0 {
                    return format!("res_code: {}, res_msg: {}", code, 
                        self.res_message.as_deref().unwrap_or(""));
                }
            }
            if let Some(code_str) = res_code.as_str() {
                if !code_str.is_empty() && code_str != "0" {
                    return format!("res_code: {}, res_msg: {}", code_str, 
                        self.res_message.as_deref().unwrap_or(""));
                }
            }
        }
        
        if let Some(code) = &self.code {
            if !code.is_empty() && code != "SUCCESS" {
                if let Some(msg) = &self.msg {
                    return format!("code: {}, msg: {}", code, msg);
                }
                if let Some(message) = &self.message {
                    return format!("code: {}, msg: {}", code, message);
                }
                return format!("code: {}", code);
            }
        }
        
        if let Some(error_code) = &self.error_code {
            return format!("err_code: {}, err_msg: {}", error_code, 
                self.error_msg.as_deref().unwrap_or(""));
        }
        
        if let Some(error) = &self.error {
            return format!("error: {}, message: {}", error, 
                self.message.as_deref().unwrap_or(""));
        }
        
        "Unknown error".to_string()
    }
}

pub struct Cloud189Driver {
    config: Cloud189Config,
    client: Client,
    identity: String,
    token_info: Arc<Mutex<Option<AppSessionResp>>>,
    login_param: Arc<Mutex<Option<LoginParam>>>,
}

impl Cloud189Driver {
    pub fn new(config: Cloud189Config) -> Result<Self> {
        // 创建客户端
        let client = Client::builder()
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
            .timeout(std::time::Duration::from_secs(30))
            .build()?;
            
        let identity = format!("{}{}", config.username, config.password);
        let identity = format!("{:x}", md5::Md5::digest(identity.as_bytes()));
        
        Ok(Self {
            config,
            client,
            identity,
            token_info: Arc::new(Mutex::new(None)),
            login_param: Arc::new(Mutex::new(None)),
        })
    }
    
    fn is_family(&self) -> bool {
        self.config.cloud_type == "family"
    }
    
    async fn is_login(&self) -> bool {
        self.token_info.lock().await.is_some()
    }
    
    async fn ensure_login(&self) -> Result<()> {
        if !self.is_login().await {
            self.login().await?;
        }
        Ok(())
    }
    
    // 刷新会话
    async fn refresh_session(&self) -> Result<()> {
        let token_info = {
            let guard = self.token_info.lock().await;
            guard.as_ref().ok_or_else(|| anyhow!("未登录"))?.clone()
        };
        
        let mut query_params = Self::client_suffix();
        query_params.insert("appId".to_string(), APP_ID.to_string());
        query_params.insert("accessToken".to_string(), token_info.access_token.clone());
        
        let response = self.client
            .get(&format!("{}/getSessionForPC.action", API_URL))
            .query(&query_params)
            .header("X-Request-ID", Uuid::new_v4().to_string())
            .send()
            .await?;
        
        let session_text = response.text().await?;
        
        // 检查错误
        if let Ok(error_resp) = serde_json::from_str::<RespErr>(&session_text) {
            if error_resp.has_error() {
                // 如果刷新失败，尝试重新登录
                if session_text.contains("UserInvalidOpenToken") {
                    println!("会话过期，重新登录...");
                    return self.login().await;
                }
                return Err(anyhow!("刷新会话失败: {}", error_resp.error_message()));
            }
        }
        
        let new_token_info: AppSessionResp = serde_json::from_str(&session_text)?;
        
        if new_token_info.res_code != 0 {
            return Err(anyhow!("刷新会话失败: {}", new_token_info.res_message));
        }
        
        *self.token_info.lock().await = Some(new_token_info);
        Ok(())
    }
    
    // 模拟alist的clientSuffix函数
    fn client_suffix() -> HashMap<String, String> {
        let mut params = HashMap::new();
        params.insert("clientType".to_string(), PC.to_string());
        params.insert("version".to_string(), VERSION.to_string());
        params.insert("channelId".to_string(), CHANNEL_ID.to_string());
        
        // 按照alist help.go中的clientSuffix实现
        let mut rng = rand::thread_rng();
        let rand1: u64 = rng.gen_range(0..100000);
        let rand2: u64 = rng.gen_range(0..10000000000);
        params.insert("rand".to_string(), format!("{}_{}", rand1, rand2));
        
        params
    }
    
    // 创建一个新的"干净"客户端，模拟alist的cookiejar.New()
    fn create_clean_client() -> Result<Client> {
        Client::builder()
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| anyhow!("Failed to create client: {}", e))
    }

    // 登录流程 - 完全重写以模拟alist
    async fn login(&self) -> Result<()> {
        println!("🔐 开始天翼云盘登录流程...");
        
        // 创建新的"干净"客户端，模拟alist的jar, _ := cookiejar.New(nil)
        let clean_client = Self::create_clean_client()?;
        
        // 步骤1: 初始化登录参数（如果还没有的话）
        if self.login_param.lock().await.is_none() {
            self.init_login_param_with_client(&clean_client).await?;
        }
        
        let login_param = {
            let guard = self.login_param.lock().await;
            guard.as_ref().ok_or_else(|| anyhow!("登录参数未初始化"))?.clone()
        };
        
        println!("🚀 使用干净客户端执行登录请求...");
        
        // 步骤2: 执行登录请求
        let login_response = clean_client
            .post(&format!("{}/api/logbox/oauth2/loginSubmit.do", AUTH_URL))
            .header("Content-Type", "application/json;charset=UTF-8")
            .header("REQID", &login_param.req_id)
            .header("lt", &login_param.lt)
            .form(&[
                ("appKey", APP_ID),
                ("accountType", ACCOUNT_TYPE),
                ("userName", &login_param.rsa_username),
                ("password", &login_param.rsa_password),
                ("validateCode", self.config.validate_code.as_deref().unwrap_or("")),
                ("captchaToken", &login_param.captcha_token),
                ("returnUrl", RETURN_URL),
                ("dynamicCheck", "FALSE"),
                ("clientType", CLIENT_TYPE),
                ("cb_SaveName", "1"),
                ("isOauth2", "false"),
                ("state", ""),
                ("paramId", &login_param.param_id),
            ])
            .send()
            .await?;
        
        let login_text = login_response.text().await?;
        println!("📋 登录响应: {}", login_text);
        
        let login_resp: LoginResp = serde_json::from_str(&login_text)
            .map_err(|e| anyhow!("解析登录响应失败: {}, 响应内容: {}", e, login_text))?;
        
        if login_resp.result != 0 {
            // 清理登录参数以便下次重试
            *self.login_param.lock().await = None;
            return Err(anyhow!("登录失败: {} (错误码: {})", login_resp.msg, login_resp.result));
        }
        
        let to_url = login_resp.to_url.ok_or_else(|| anyhow!("登录响应中缺少toUrl字段"))?;
        if to_url.is_empty() {
            return Err(anyhow!("登录失败: toUrl为空"));
        }
        
        println!("✅ 获取到重定向URL: {}", to_url);
        
        // 步骤3: 获取会话信息
        let mut query_params = Self::client_suffix();
        query_params.insert("redirectURL".to_string(), urlencoding::encode(&to_url).to_string());
        
        let session_response = clean_client
            .post(&format!("{}/getSessionForPC.action", API_URL))
            .query(&query_params)
            .send()
            .await?;
        
        let session_text = session_response.text().await?;
        println!("📡 会话响应: {}", session_text);
        
        // 检查错误
        if let Ok(error_resp) = serde_json::from_str::<RespErr>(&session_text) {
            if error_resp.has_error() {
                return Err(anyhow!("获取会话失败: {}", error_resp.error_message()));
            }
        }
        
        let token_info: AppSessionResp = serde_json::from_str(&session_text)
            .map_err(|e| anyhow!("解析会话响应失败: {}, 响应内容: {}", e, session_text))?;
        
        if token_info.res_code != 0 {
            return Err(anyhow!("获取会话失败: {}", token_info.res_message));
        }
        
        println!("🎉 登录成功！用户: {}", token_info.login_name);
        *self.token_info.lock().await = Some(token_info);
        
        // 清理登录参数
        *self.login_param.lock().await = None;
        
        Ok(())
    }
    
    // 使用指定客户端初始化登录参数
    async fn init_login_param_with_client(&self, client: &Client) -> Result<()> {
        println!("🔧 使用干净客户端初始化登录参数...");
        
        // 获取登录页面
        let response = client
            .get(&format!("{}/api/portal/unifyLoginForPC.action", WEB_URL))
            .query(&[
                ("appId", APP_ID),
                ("clientType", CLIENT_TYPE),
                ("returnURL", RETURN_URL),
                ("timeStamp", &Self::get_timestamp().to_string()),
            ])
            .send()
            .await?;
        
        let html = response.text().await?;
        println!("📄 获取登录页面成功，长度: {} 字符", html.len());
        
        // 提取参数
        let captcha_token = extract_regex(&html, r"'captchaToken' value='(.+?)'")?;
        let lt = extract_regex(&html, r#"lt = "(.+?)""#)?;
        let param_id = extract_regex(&html, r#"paramId = "(.+?)""#)?;
        let req_id = extract_regex(&html, r#"reqId = "(.+?)""#)?;
        
        println!("📝 提取登录参数成功:");
        println!("  captchaToken: {}", captcha_token);
        println!("  lt: {}", lt);
        println!("  paramId: {}", param_id);
        println!("  reqId: {}", req_id);
        
        // 获取RSA公钥配置
        let encrypt_conf: EncryptConfResp = client
            .post(&format!("{}/api/logbox/config/encryptConf.do", AUTH_URL))
            .header("Content-Type", "application/json;charset=UTF-8")
            .form(&[("appId", APP_ID)])
            .send()
            .await?
            .json()
            .await?;
        
        println!("🔑 获取RSA公钥配置成功:");
        println!("  result: {}", encrypt_conf.result);
        println!("  pre: {}", encrypt_conf.data.pre);
        println!("  pubKey长度: {}", encrypt_conf.data.pub_key.len());
        
        let pub_key = format!(
            "-----BEGIN PUBLIC KEY-----\n{}\n-----END PUBLIC KEY-----",
            encrypt_conf.data.pub_key
        );
        
        let rsa_username = format!("{}{}", 
            encrypt_conf.data.pre, 
            Self::rsa_encrypt(&pub_key, &self.config.username)?
        );
        let rsa_password = format!("{}{}", 
            encrypt_conf.data.pre, 
            Self::rsa_encrypt(&pub_key, &self.config.password)?
        );
        
        println!("🔐 RSA加密完成:");
        println!("  rsa_username长度: {}", rsa_username.len());
        println!("  rsa_password长度: {}", rsa_password.len());
        
        // 检查是否需要验证码
        let need_captcha = client
            .post(&format!("{}/api/logbox/oauth2/needcaptcha.do", AUTH_URL))
            .header("REQID", &req_id)
            .form(&[
                ("appKey", APP_ID),
                ("accountType", ACCOUNT_TYPE),
                ("userName", &rsa_username),
            ])
            .send()
            .await?
            .text()
            .await?;
        
        println!("🤖 验证码检查结果: {}", need_captcha);
        
        if need_captcha != "0" && self.config.validate_code.is_none() {
            return Err(anyhow!("需要验证码，请在配置中提供验证码。返回值: {}", need_captcha));
        }
        
        let login_param = LoginParam {
            rsa_username,
            rsa_password,
            j_rsa_key: pub_key,
            lt,
            req_id,
            param_id,
            captcha_token,
        };
        
        *self.login_param.lock().await = Some(login_param);
        
        Ok(())
    }
    
    // 保持向后兼容的初始化登录参数方法
    async fn init_login_param(&self) -> Result<()> {
        let clean_client = Self::create_clean_client()?;
        self.init_login_param_with_client(&clean_client).await
    }
    
    fn get_timestamp() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
    }
    
    fn get_http_date_str() -> String {
        let now = Utc::now();
        now.format("%a, %d %b %Y %H:%M:%S GMT").to_string()
    }
    
    fn rsa_encrypt(public_key: &str, data: &str) -> Result<String> {
        // 清理公钥格式，移除可能的换行符和空格
        let clean_key = public_key
            .replace("-----BEGIN PUBLIC KEY-----", "")
            .replace("-----END PUBLIC KEY-----", "")
            .replace("\n", "")
            .replace("\r", "")
            .replace(" ", "");
        
        // 按照标准PEM格式，每64个字符一行
        let mut formatted_key = String::new();
        for (i, c) in clean_key.chars().enumerate() {
            if i > 0 && i % 64 == 0 {
                formatted_key.push('\n');
            }
            formatted_key.push(c);
        }
        
        // 重新构建正确的PEM格式
        let pem_key = format!("-----BEGIN PUBLIC KEY-----\n{}\n-----END PUBLIC KEY-----", formatted_key);
        
        println!("使用RSA公钥加密，公钥长度: {}", clean_key.len());
        println!("格式化的PEM公钥:\n{}", pem_key);
        
        // 使用PKCS#8格式解析公钥（天翼云盘使用的是PKIX格式）
        let public_key = RsaPublicKey::from_public_key_pem(&pem_key)?;
        let mut rng = rand::thread_rng();
        let encrypted_data = public_key.encrypt(&mut rng, Pkcs1v15Encrypt, data.as_bytes())?;
        Ok(hex::encode(encrypted_data).to_uppercase())
    }
    
    fn aes_ecb_encrypt(data: &str, key: &str) -> Result<String> {
        use aes::cipher::{BlockEncrypt, KeyInit};
        use aes::Aes128;
        
        let key_bytes = key.as_bytes();
        if key_bytes.len() != 16 {
            return Err(anyhow!("AES key must be 16 bytes"));
        }
        
        let cipher = Aes128::new_from_slice(key_bytes)?;
        
        // PKCS7 padding
        let data_bytes = data.as_bytes();
        let block_size = 16;
        let padding = block_size - (data_bytes.len() % block_size);
        let mut padded_data = data_bytes.to_vec();
        padded_data.extend(vec![padding as u8; padding]);
        
        // ECB mode encryption
        let mut encrypted = vec![0u8; padded_data.len()];
        for (i, chunk) in padded_data.chunks(16).enumerate() {
            let mut block = [0u8; 16];
            block.copy_from_slice(chunk);
            let mut block = aes::Block::from(block);
            cipher.encrypt_block(&mut block);
            encrypted[i * 16..(i + 1) * 16].copy_from_slice(&block);
        }
        
        Ok(hex::encode(encrypted).to_uppercase())
    }
    
    fn signature_of_hmac(
        session_secret: &str,
        session_key: &str,
        method: &str,
        url: &str,
        date: &str,
        params: &str,
    ) -> Result<String> {
        // 按照alist help.go中的signatureOfHmac实现
        let re = Regex::new(r"://[^/]+((/[^/\s?#]+)*)")?;
        let url_path = if let Some(captures) = re.captures(url) {
            captures.get(1).map(|m| m.as_str()).unwrap_or("")
        } else {
            ""
        };
        
        let mut data = format!("SessionKey={}&Operate={}&RequestURI={}&Date={}", 
            session_key, method, url_path, date);
        
        if !params.is_empty() {
            data.push_str(&format!("&params={}", params));
        }
        
        let mut mac = <HmacSha1 as hmac::Mac>::new_from_slice(session_secret.as_bytes())
            .map_err(|_| anyhow!("Invalid key length"))?;
        mac.update(data.as_bytes());
        let result = mac.finalize();
        let signature = hex::encode(result.into_bytes()).to_uppercase();
        
        Ok(signature)
    }
    
    async fn signature_header(&self, url: &str, method: &str, params: &str, is_family: bool) -> Result<HeaderMap> {
        let date_of_gmt = Self::get_http_date_str();
        let token_info_guard = self.token_info.lock().await;
        let token_info = token_info_guard.as_ref().ok_or_else(|| anyhow!("Not logged in"))?;
        
        let session_key = if is_family {
            token_info.family_session_key.as_deref().unwrap_or(&token_info.session_key)
        } else {
            &token_info.session_key
        };
        
        let session_secret = if is_family {
            token_info.family_session_secret.as_deref().unwrap_or(&token_info.session_secret)
        } else {
            &token_info.session_secret
        };
        
        let signature = Self::signature_of_hmac(
            session_secret, session_key, method, url, &date_of_gmt, params
        )?;
        
        let mut headers = HeaderMap::new();
        headers.insert("Date", HeaderValue::from_str(&date_of_gmt)?);
        headers.insert("SessionKey", HeaderValue::from_str(session_key)?);
        headers.insert("X-Request-ID", HeaderValue::from_str(&Uuid::new_v4().to_string())?);
        headers.insert("Signature", HeaderValue::from_str(&signature)?);
        
        Ok(headers)
    }
    
    async fn encrypt_params(&self, params: &HashMap<String, String>, is_family: bool) -> Result<String> {
        let token_info_guard = self.token_info.lock().await;
        let token_info = token_info_guard.as_ref().ok_or_else(|| anyhow!("Not logged in"))?;
        
        let session_secret = if is_family {
            token_info.family_session_secret.as_deref().unwrap_or(&token_info.session_secret)
        } else {
            &token_info.session_secret
        };
        
        if !params.is_empty() {
            let params_str = serde_urlencoded::to_string(params)?;
            let key = &session_secret[..16];
            Self::aes_ecb_encrypt(&params_str, key)
        } else {
            Ok(String::new())
        }
    }
    
    async fn get_files(&self, folder_id: &str, is_family: bool) -> Result<Vec<FileInfo>> {
        let mut files = Vec::new();
        let mut page_num = 1;
        
        loop {
            let resp = self.get_files_with_page(folder_id, is_family, page_num, 1000).await?;
            
            if resp.file_list_ao.count == 0 {
                break;
            }
            
            // 添加文件夹
            for folder in resp.file_list_ao.folder_list {
                files.push(FileInfo {
                    name: folder.name,
                    path: folder.id.clone(),
                    size: 0,
                    is_dir: true,
                    modified: folder.last_op_time,
                });
            }
            
            // 添加文件
            for file in resp.file_list_ao.file_list {
                files.push(FileInfo {
                    name: file.name,
                    path: file.id.clone(),
                    size: file.size as u64,
                    is_dir: false,
                    modified: file.last_op_time,
                });
            }
            
            page_num += 1;
        }
        
        Ok(files)
    }
    
    async fn get_files_with_page(
        &self,
        folder_id: &str,
        is_family: bool,
        page_num: i32,
        page_size: i32,
    ) -> Result<Cloud189FilesResp> {
        let url = if is_family {
            format!("{}/family/file/listFiles.action", API_URL)
        } else {
            format!("{}/listFiles.action", API_URL)
        };
        
        let mut params = HashMap::new();
        params.insert("folderId".to_string(), folder_id.to_string());
        params.insert("fileType".to_string(), "0".to_string());
        params.insert("mediaAttr".to_string(), "0".to_string());
        params.insert("iconOption".to_string(), "5".to_string());
        params.insert("pageNum".to_string(), page_num.to_string());
        params.insert("pageSize".to_string(), page_size.to_string());
        
        if is_family {
            if let Some(family_id) = &self.config.family_id {
                params.insert("familyId".to_string(), family_id.clone());
            }
            params.insert("orderBy".to_string(), self.config.order_by.clone());
            params.insert("descending".to_string(), 
                if self.config.order_direction == "desc" { "true" } else { "false" }.to_string());
        } else {
            params.insert("recursive".to_string(), "0".to_string());
            params.insert("orderBy".to_string(), self.config.order_by.clone());
            params.insert("descending".to_string(), 
                if self.config.order_direction == "desc" { "true" } else { "false" }.to_string());
        }
        
        self.request(&url, "GET", Some(params), None, is_family).await
    }
    
    async fn request<T: for<'de> serde::Deserialize<'de>>(
        &self,
        url: &str,
        method: &str,
        params: Option<HashMap<String, String>>,
        body: Option<serde_json::Value>,
        is_family: bool,
    ) -> Result<T> {
        // 最多重试3次
        for attempt in 1..=3 {
            match self.try_request(url, method, params.clone(), body.clone(), is_family).await {
                Ok(result) => return Ok(result),
                Err(e) => {
                    println!("请求尝试 {}/3 失败: {}", attempt, e);
                    
                    // 如果是认证错误，尝试刷新会话
                    if e.to_string().contains("UserInvalidOpenToken") || 
                       e.to_string().contains("InvalidSessionKey") ||
                       e.to_string().contains("-20000") {
                        println!("检测到认证错误，尝试刷新会话...");
                        if let Err(refresh_err) = self.refresh_session().await {
                            println!("刷新会话失败: {}", refresh_err);
                        }
                    }
                    
                    if attempt < 3 {
                        println!("等待1秒后重试...");
                        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                    } else {
                        return Err(e);
                    }
                }
            }
        }
        
        unreachable!()
    }
    
    async fn try_request<T: for<'de> serde::Deserialize<'de>>(
        &self,
        url: &str,
        method: &str,
        params: Option<HashMap<String, String>>,
        body: Option<serde_json::Value>,
        is_family: bool,
    ) -> Result<T> {
        let params = params.unwrap_or_default();
        let encrypted_params = self.encrypt_params(&params, is_family).await?;
        let headers = self.signature_header(url, method, &encrypted_params, is_family).await?;
        
        println!("发送请求: {} {}", method, url);
        println!("加密参数: {}", encrypted_params);
        
        let mut request_builder = match method.to_uppercase().as_str() {
            "GET" => {
                let mut query_params = Self::client_suffix();
                if !encrypted_params.is_empty() {
                    query_params.insert("params".to_string(), encrypted_params);
                }
                self.client.get(url).query(&query_params)
            }
            "POST" => {
                let mut form_data = Self::client_suffix();
                if !encrypted_params.is_empty() {
                    form_data.insert("params".to_string(), encrypted_params);
                }
                if let Some(body_data) = body {
                    // 如果有body数据，合并到form_data中
                    if let serde_json::Value::Object(obj) = body_data {
                        for (key, value) in obj {
                            if let serde_json::Value::String(s) = value {
                                form_data.insert(key, s);
                            } else {
                                form_data.insert(key, value.to_string());
                            }
                        }
                    }
                }
                self.client.post(url).form(&form_data)
            }
            _ => return Err(anyhow!("不支持的HTTP方法: {}", method)),
        };
        
        request_builder = request_builder.headers(headers);
        
        let response = request_builder.send().await?;
        let status = response.status();
        let response_text = response.text().await?;
        
        println!("响应状态: {}", status);
        println!("响应内容: {}", response_text);
        
        if !status.is_success() {
            return Err(anyhow!("HTTP请求失败，状态码: {}, 响应: {}", status, response_text));
        }
        
        // 首先尝试解析为错误响应
        if let Ok(error_resp) = serde_json::from_str::<RespErr>(&response_text) {
            if error_resp.has_error() {
                return Err(anyhow!("API错误: {}", error_resp.error_message()));
            }
        }
        
        // 解析为目标类型
        serde_json::from_str(&response_text)
            .map_err(|e| anyhow!("解析响应失败: {}, 响应内容: {}", e, response_text))
    }
    
    async fn get_download_url(&self, path: &str) -> Result<Option<String>> {
        self.ensure_login().await?;
        
        // 解析路径获取文件ID
        let file_id = self.path_to_file_id(path).await?;
        println!("获取文件 '{}' 的下载URL，文件ID: {}", path, file_id);
        
        // 获取下载URL
        let download_url = self.get_file_download_url(&file_id, self.is_family()).await?;
        Ok(Some(download_url))
    }
    
    async fn path_to_file_id(&self, path: &str) -> Result<String> {
        if path == "/" || path.is_empty() {
            return Err(anyhow!("路径不能为根目录"));
        }
        
        let parts: Vec<&str> = path.trim_start_matches('/').split('/').filter(|s| !s.is_empty()).collect();
        if parts.is_empty() {
            return Err(anyhow!("无效的文件路径"));
        }
        
        let file_name = parts.last().unwrap();
        let parent_path = if parts.len() == 1 {
            "/"
        } else {
            &format!("/{}", parts[..parts.len()-1].join("/"))
        };
        
        // 获取父文件夹ID
        let parent_folder_id = self.path_to_folder_id(parent_path).await?;
        
        // 在父文件夹中查找文件
        self.find_file_by_name(&parent_folder_id, file_name).await
    }
    
    async fn find_file_by_name(&self, parent_folder_id: &str, file_name: &str) -> Result<String> {
        let files_resp = self.get_files_with_page(parent_folder_id, self.is_family(), 1, 100).await?;
        
        for file in &files_resp.file_list_ao.file_list {
            if file.name == file_name {
                return Ok(file.id.clone());
            }
        }
        
        Err(anyhow!("文件 '{}' 在文件夹 '{}' 中未找到", file_name, parent_folder_id))
    }
    
    async fn get_file_download_url(&self, file_id: &str, is_family: bool) -> Result<String> {
        let mut params = HashMap::new();
        params.insert("fileId".to_string(), file_id.to_string());
        params.insert("dt".to_string(), "3".to_string());
        
        let url = if is_family {
            format!("{}/family/file/getFileDownloadUrl", API_URL)
        } else {
            format!("{}/open/file/getFileDownloadUrl", API_URL)
        };
        
        let response: DownloadUrlResp = self.request(&url, "GET", Some(params), None, is_family).await?;
        Ok(response.file_download_url)
    }
    
    async fn path_to_folder_id(&self, path: &str) -> Result<String> {
        if path == "/" || path.is_empty() {
            return Ok(self.config.root_folder_id.clone());
        }
        
        let parts: Vec<&str> = path.trim_start_matches('/').split('/').filter(|s| !s.is_empty()).collect();
        let mut current_folder_id = self.config.root_folder_id.clone();
        
        for part in parts {
            current_folder_id = self.find_folder_by_name(&current_folder_id, part).await?;
        }
        
        Ok(current_folder_id)
    }
    
    async fn find_folder_by_name(&self, parent_folder_id: &str, folder_name: &str) -> Result<String> {
        let files_resp = self.get_files_with_page(parent_folder_id, self.is_family(), 1, 100).await?;
        
        for folder in &files_resp.file_list_ao.folder_list {
            if folder.name == folder_name {
                return Ok(folder.id.clone());
            }
        }
        
        Err(anyhow!("文件夹 '{}' 在父文件夹 '{}' 中未找到", folder_name, parent_folder_id))
    }
}

fn extract_regex(text: &str, pattern: &str) -> Result<String> {
    let re = Regex::new(pattern)?;
    let captures = re.captures(text)
        .ok_or_else(|| anyhow!("Pattern not found: {}", pattern))?;
    Ok(captures.get(1)
        .ok_or_else(|| anyhow!("Capture group not found"))?
        .as_str()
        .to_string())
}

#[async_trait]
impl Driver for Cloud189Driver {
    async fn list(&self, path: &str) -> Result<Vec<FileInfo>> {
        self.ensure_login().await?;
        
        // 解析路径获取文件夹ID
        let folder_id = self.path_to_folder_id(path).await?;
        println!("列出路径 '{}' 对应的文件夹ID: {}", path, folder_id);
        
        // 获取文件列表
        self.get_files(&folder_id, self.is_family()).await
    }
    
    async fn download(&self, path: &str) -> Result<tokio::fs::File> {
        // 天翼云盘不支持直接文件下载，需要通过stream_download
        Err(anyhow!("请使用stream_download方法下载文件"))
    }
    
    async fn get_download_url(&self, path: &str) -> Result<Option<String>> {
        self.ensure_login().await?;
        
        // 解析路径获取文件ID
        let file_id = self.path_to_file_id(path).await?;
        println!("获取文件 '{}' 的下载URL，文件ID: {}", path, file_id);
        
        // 获取下载URL
        let download_url = self.get_file_download_url(&file_id, self.is_family()).await?;
        Ok(Some(download_url))
    }
    
    async fn upload_file(&self, _parent_path: &str, _file_name: &str, _content: &[u8]) -> Result<()> {
        Err(anyhow!("天翼云盘上传功能暂未实现"))
    }
    
    async fn delete(&self, _path: &str) -> Result<()> {
        Err(anyhow!("天翼云盘删除功能暂未实现"))
    }
    
    async fn rename(&self, _path: &str, _new_name: &str) -> Result<()> {
        Err(anyhow!("天翼云盘重命名功能暂未实现"))
    }
    
    async fn create_folder(&self, _parent_path: &str, _folder_name: &str) -> Result<()> {
        Err(anyhow!("天翼云盘创建文件夹功能暂未实现"))
    }
    
    async fn get_file_info(&self, _path: &str) -> Result<FileInfo> {
        Err(anyhow!("天翼云盘获取文件信息功能暂未实现"))
    }
    
    async fn move_file(&self, _file_path: &str, _new_parent_path: &str) -> Result<()> {
        Err(anyhow!("天翼云盘不支持移动操作"))
    }
    
    async fn copy_file(&self, _file_path: &str, _new_parent_path: &str) -> Result<()> {
        Err(anyhow!("天翼云盘不支持复制操作"))
    }
    
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    
    async fn stream_download(&self, path: &str) -> Result<Option<(Box<dyn futures::Stream<Item = Result<axum::body::Bytes, std::io::Error>> + Send + Unpin>, String)>> {
        self.ensure_login().await?;
        
        // 获取下载URL
        let download_url = match self.get_download_url(path).await? {
            Some(url) => url,
            None => return Ok(None),
        };
        
        println!("开始流式下载: {}", download_url);
        
        let client = self.client.clone();
        let stream = async_stream::stream! {
            let response = match client.get(&download_url).send().await {
                Ok(resp) => resp,
                Err(e) => {
                    yield Err(std::io::Error::new(std::io::ErrorKind::Other, e));
                    return;
                }
            };
            
            let mut stream = response.bytes_stream();
            while let Some(chunk) = stream.next().await {
                match chunk {
                    Ok(bytes) => yield Ok(bytes),
                    Err(e) => {
                        yield Err(std::io::Error::new(std::io::ErrorKind::Other, e));
                        return;
                    }
                }
            }
        };
        
        Ok(Some((Box::new(Box::pin(stream)), "application/octet-stream".to_string())))
    }
    
    async fn stream_download_with_range(&self, path: &str, start: Option<u64>, end: Option<u64>) -> Result<Option<(Box<dyn futures::Stream<Item = Result<axum::body::Bytes, std::io::Error>> + Send + Unpin>, String, u64, Option<u64>)>> {
        self.ensure_login().await?;
        
        // 获取下载URL
        let download_url = match self.get_download_url(path).await? {
            Some(url) => url,
            None => return Ok(None),
        };
        
        // 先获取文件大小
        let head_response = self.client.head(&download_url).send().await?;
        let content_length = head_response
            .headers()
            .get("content-length")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0);
        
        let actual_start = start.unwrap_or(0);
        let actual_end = end.unwrap_or(content_length.saturating_sub(1));
        
        println!("范围下载: {} bytes={}-{}", download_url, actual_start, actual_end);
        
        let client = self.client.clone();
        let stream = async_stream::stream! {
            let response = match client.get(&download_url)
                .header("Range", format!("bytes={}-{}", actual_start, actual_end))
                .send()
                .await 
            {
                Ok(resp) => resp,
                Err(e) => {
                    yield Err(std::io::Error::new(std::io::ErrorKind::Other, e));
                    return;
                }
            };
            
            let mut stream = response.bytes_stream();
            while let Some(chunk) = stream.next().await {
                match chunk {
                    Ok(bytes) => yield Ok(bytes),
                    Err(e) => {
                        yield Err(std::io::Error::new(std::io::ErrorKind::Other, e));
                        return;
                    }
                }
            }
        };
        
        Ok(Some((Box::new(Box::pin(stream)), "application/octet-stream".to_string(), actual_start, Some(actual_end))))
    }
}

pub struct Cloud189DriverFactory;

#[async_trait]
impl DriverFactory for Cloud189DriverFactory {
    fn driver_type(&self) -> &'static str {
        "cloud189"
    }
    
    fn driver_info(&self) -> DriverInfo {
        DriverInfo {
            driver_type: "cloud189".to_string(),
            display_name: "天翼云盘".to_string(),
            description: "中国电信天翼云盘存储驱动".to_string(),
            config_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "username": {
                        "type": "string",
                        "title": "用户名",
                        "description": "天翼云盘用户名"
                    },
                    "password": {
                        "type": "string",
                        "title": "密码",
                        "description": "天翼云盘密码",
                        "format": "password"
                    },
                    "validate_code": {
                        "type": "string",
                        "title": "验证码",
                        "description": "登录验证码（如需要）"
                    },
                    "root_folder_id": {
                        "type": "string",
                        "title": "根文件夹ID",
                        "description": "根文件夹ID，个人云默认-11",
                        "default": "-11"
                    },
                    "order_by": {
                        "type": "string",
                        "title": "排序方式",
                        "enum": ["filename", "filesize", "lastOpTime"],
                        "default": "filename"
                    },
                    "order_direction": {
                        "type": "string",
                        "title": "排序方向",
                        "enum": ["asc", "desc"],
                        "default": "asc"
                    },
                    "cloud_type": {
                        "type": "string",
                        "title": "云盘类型",
                        "enum": ["personal", "family"],
                        "default": "personal"
                    },
                    "family_id": {
                        "type": "string",
                        "title": "家庭云ID",
                        "description": "家庭云ID（仅家庭云需要）"
                    },
                    "upload_method": {
                        "type": "string",
                        "title": "上传方式",
                        "enum": ["stream", "rapid", "old"],
                        "default": "stream"
                    },
                    "upload_thread": {
                        "type": "integer",
                        "title": "上传线程数",
                        "minimum": 1,
                        "maximum": 32,
                        "default": 3
                    },
                    "family_transfer": {
                        "type": "boolean",
                        "title": "家庭云转存",
                        "default": false
                    },
                    "rapid_upload": {
                        "type": "boolean",
                        "title": "秒传",
                        "default": false
                    },
                    "no_use_ocr": {
                        "type": "boolean",
                        "title": "不使用OCR",
                        "default": false
                    }
                },
                "required": ["username", "password"]
            }),
        }
    }
    
    fn create_driver(&self, config: serde_json::Value) -> Result<Box<dyn Driver>> {
        let config: Cloud189Config = serde_json::from_value(config)?;
        let driver = Cloud189Driver::new(config)?;
        Ok(Box::new(driver))
    }
    
    fn get_routes(&self) -> Option<axum::Router> {
        None
    }
}

