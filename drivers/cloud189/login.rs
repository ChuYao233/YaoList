//! Cloud189 login logic / 天翼云盘登录逻辑
use anyhow::{anyhow, Result};
use reqwest::Client;
use chrono::Utc;

use super::types::*;
use super::utils::*;

/// Login manager / 登录管理器
pub struct LoginManager {
    client: Client,
}

impl LoginManager {
    pub fn new(client: Client) -> Self {
        Self { client }
    }

    /// Initialize base login parameters - copied from initBaseParams / 初始化基础登录参数
    pub async fn init_base_params(&self) -> Result<BaseLoginParam> {
        let ts = Utc::now().timestamp_millis();
        let text = self.client
            .get(format!("{}/api/portal/unifyLoginForPC.action", WEB_URL))
            .query(&[
                ("appId", APP_ID),
                ("clientType", CLIENT_TYPE),
                ("returnURL", RETURN_URL),
                ("timeStamp", &ts.to_string()),
            ])
            .header("Referer", WEB_URL)
            .send()
            .await?
            .text()
            .await?;

        let captcha_token = regex::Regex::new(r"'captchaToken' value='(.+?)'")
            .ok()
            .and_then(|r| r.captures(&text))
            .and_then(|c| c.get(1))
            .map(|m| m.as_str().to_string())
            .unwrap_or_default();

        let lt = regex::Regex::new(r#"lt = "(.+?)""#)
            .ok()
            .and_then(|r| r.captures(&text))
            .and_then(|c| c.get(1))
            .map(|m| m.as_str().to_string())
            .unwrap_or_default();

        let param_id = regex::Regex::new(r#"paramId = "(.+?)""#)
            .ok()
            .and_then(|r| r.captures(&text))
            .and_then(|c| c.get(1))
            .map(|m| m.as_str().to_string())
            .unwrap_or_default();

        let req_id = regex::Regex::new(r#"reqId = "(.+?)""#)
            .ok()
            .and_then(|r| r.captures(&text))
            .and_then(|c| c.get(1))
            .map(|m| m.as_str().to_string())
            .unwrap_or_default();

        if lt.is_empty() || req_id.is_empty() {
            return Err(anyhow!("Failed to get login parameters / 获取登录参数失败"));
        }

        Ok(BaseLoginParam {
            captcha_token,
            lt,
            param_id,
            req_id,
        })
    }

    /// Get encryption configuration / 获取加密配置
    pub async fn get_encrypt_conf(&self) -> Result<EncryptConfData> {
        let text = self.client
            .post(format!("{}/api/logbox/config/encryptConf.do", AUTH_URL))
            .header("Content-Type", "application/x-www-form-urlencoded")
            .form(&[("appId", APP_ID)])
            .send()
            .await?
            .text()
            .await?;

        let resp: EncryptConfResp = serde_json::from_str(&text)
            .map_err(|e| anyhow!("Failed to parse encryption config / 解析加密配置失败: {} - {}", e, text))?;

        resp.data.ok_or_else(|| anyhow!("Failed to get encryption config / 获取加密配置失败"))
    }

    /// Password login - copied from loginByPassword / 密码登录
    pub async fn login_by_password(&self, username: &str, password: &str) -> Result<AppSessionResp> {
        // Step 1: Get base parameters / 步骤1: 获取基础参数
        let base_param = self.init_base_params().await?;

        // Step 2: Get RSA encryption configuration / 步骤2: 获取RSA加密配置
        let encrypt_conf = self.get_encrypt_conf().await?;

        // Step 3: RSA encrypt username and password / 步骤3: RSA加密用户名和密码
        let rsa_username = format!("{}{}", encrypt_conf.pre, rsa_encrypt(&encrypt_conf.pub_key, username)?);
        let rsa_password = format!("{}{}", encrypt_conf.pre, rsa_encrypt(&encrypt_conf.pub_key, password)?);

        // Step 4: Execute login / 步骤4: 执行登录
        let login_text = self.client
            .post(format!("{}/api/logbox/oauth2/loginSubmit.do", AUTH_URL))
            .header("Referer", AUTH_URL)
            .header("REQID", &base_param.req_id)
            .header("lt", &base_param.lt)
            .form(&[
                ("appKey", APP_ID),
                ("accountType", ACCOUNT_TYPE),
                ("userName", rsa_username.as_str()),
                ("password", rsa_password.as_str()),
                ("validateCode", ""),
                ("captchaToken", base_param.captcha_token.as_str()),
                ("returnUrl", RETURN_URL),
                ("dynamicCheck", "FALSE"),
                ("clientType", CLIENT_TYPE),
                ("cb_SaveName", "1"),
                ("isOauth2", "false"),
                ("state", ""),
                ("paramId", base_param.param_id.as_str()),
            ])
            .send()
            .await?
            .text()
            .await?;

        let login_resp: LoginResp = serde_json::from_str(&login_text)
            .map_err(|e| anyhow!("Failed to parse login response / 解析登录响应失败: {} - {}", e, login_text))?;

        if login_resp.to_url.is_empty() {
            return Err(anyhow!("Login failed / 登录失败: {}", login_resp.msg));
        }

        // Step 5: Get Session / 步骤5: 获取Session
        self.get_session_for_pc(Some(&login_resp.to_url)).await
    }

    /// Get Session - copied from getSessionForPC / 获取Session
    pub async fn get_session_for_pc(&self, redirect_url: Option<&str>) -> Result<AppSessionResp> {
        let mut query = client_suffix();
        if let Some(url) = redirect_url {
            query.push(("redirectURL".to_string(), url.to_string()));
        }

        let text = self.client
            .post(format!("{}/getSessionForPC.action", API_URL))
            .query(&query)
            .header("Accept", "application/json;charset=UTF-8")
            .send()
            .await?
            .text()
            .await?;

        parse_session_response(&text)
    }

    /// Refresh Token - copied from refreshToken / 刷新Token
    pub async fn refresh_token(&self, refresh_token: &str) -> Result<AppSessionResp> {
        let text = self.client
            .post(format!("{}/api/oauth2/refreshToken.do", AUTH_URL))
            .header("Content-Type", "application/x-www-form-urlencoded")
            .form(&[
                ("clientId", APP_ID),
                ("refreshToken", refresh_token),
                ("grantType", "refresh_token"),
                ("format", "json"),
            ])
            .send()
            .await?
            .text()
            .await?;

        // Check errors / 检查错误
        if let Ok(err) = serde_json::from_str::<RespErr>(&text) {
            if err.has_error() {
                return Err(anyhow!("Failed to refresh token / 刷新Token失败: {}", err.error_message()));
            }
        }

        let new_token: AppSessionResp = serde_json::from_str(&text)
            .map_err(|e| anyhow!("Failed to parse refresh token response / 解析刷新Token响应失败: {} - {}", e, text))?;

        // Refresh session / 刷新session
        self.refresh_session(&new_token).await
    }

    /// Refresh Session - copied from refreshSession / 刷新Session
    pub async fn refresh_session(&self, token: &AppSessionResp) -> Result<AppSessionResp> {
        let mut query = client_suffix();
        query.push(("appId".to_string(), APP_ID.to_string()));
        query.push(("accessToken".to_string(), token.access_token.clone()));

        let text = self.client
            .get(format!("{}/getSessionForPC.action", API_URL))
            .query(&query)
            .header("X-Request-ID", uuid::Uuid::new_v4().to_string())
            .send()
            .await?
            .text()
            .await?;

        let mut session = parse_session_response(&text)?;
        // Keep refresh_token / 保留refresh_token
        session.refresh_token = token.refresh_token.clone();
        Ok(session)
    }
}

/// Base login parameters / 基础登录参数
pub struct BaseLoginParam {
    pub captcha_token: String,
    pub lt: String,
    pub param_id: String,
    pub req_id: String,
}
