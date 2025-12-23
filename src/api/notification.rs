use axum::{
    extract::State,
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;
use chrono::Utc;
use uuid::Uuid;
use lettre::{
    message::header::ContentType,
    transport::smtp::authentication::Credentials,
    AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor,
};
use hmac::{Hmac, Mac};
use sha1::Sha1;
use std::collections::BTreeMap;
use tower_cookies::Cookies;

use crate::state::AppState;
use crate::auth::SESSION_COOKIE_NAME;

/// 验证管理员权限
async fn require_admin(state: &AppState, cookies: &Cookies) -> Result<(), (StatusCode, Json<Value>)> {
    let session_id = cookies.get(SESSION_COOKIE_NAME)
        .map(|c| c.value().to_string())
        .ok_or_else(|| (StatusCode::UNAUTHORIZED, Json(json!({"error": "未登录"}))))?;
    
    let is_admin: Option<bool> = sqlx::query_scalar(
        "SELECT u.is_admin FROM users u 
         JOIN sessions s ON u.id = s.user_id 
         WHERE s.id = ? AND s.expires_at > datetime('now') AND u.enabled = 1"
    )
    .bind(&session_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "服务器错误"}))))?;
    
    if !is_admin.unwrap_or(false) {
        return Err((StatusCode::FORBIDDEN, Json(json!({"error": "需要管理员权限"}))));
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationSettings {
    pub email_enabled: bool,
    pub email_host: String,
    pub email_port: i32,
    pub email_username: String,
    pub email_password: String,
    pub email_from_email: String,
    pub email_from_name: String,
    pub email_use_tls: bool,
    pub sms_enabled: bool,
    pub sms_access_key_id: String,
    pub sms_access_key_secret: String,
    pub sms_sign_name: String,
    pub sms_template_code: String,
}

impl Default for NotificationSettings {
    fn default() -> Self {
        Self {
            email_enabled: false,
            email_host: String::new(),
            email_port: 465,
            email_username: String::new(),
            email_password: String::new(),
            email_from_email: String::new(),
            email_from_name: "YaoList".to_string(),
            email_use_tls: true,
            sms_enabled: false,
            sms_access_key_id: String::new(),
            sms_access_key_secret: String::new(),
            sms_sign_name: String::new(),
            sms_template_code: String::new(),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct TestRequest {
    pub target: String,
}

/// 从数据库加载通知设置
pub async fn load_notification_settings(state: &AppState) -> NotificationSettings {
    let mut settings = NotificationSettings::default();
    
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT key, value FROM site_settings WHERE key LIKE 'notification_%'"
    )
    .fetch_all(&state.db)
    .await
    .unwrap_or_default();

    for (key, value) in rows {
        match key.as_str() {
            "notification_email_enabled" => settings.email_enabled = value == "true",
            "notification_email_host" => settings.email_host = value,
            "notification_email_port" => settings.email_port = value.parse().unwrap_or(465),
            "notification_email_username" => settings.email_username = value,
            "notification_email_password" => settings.email_password = value,
            "notification_email_from_email" => settings.email_from_email = value,
            "notification_email_from_name" => settings.email_from_name = value,
            "notification_email_use_tls" => settings.email_use_tls = value == "true",
            "notification_sms_enabled" => settings.sms_enabled = value == "true",
            "notification_sms_access_key_id" => settings.sms_access_key_id = value,
            "notification_sms_access_key_secret" => settings.sms_access_key_secret = value,
            "notification_sms_sign_name" => settings.sms_sign_name = value,
            "notification_sms_template_code" => settings.sms_template_code = value,
            _ => {}
        }
    }
    
    settings
}

/// 保存单个设置项
async fn save_setting(state: &AppState, key: &str, value: &str) -> Result<(), sqlx::Error> {
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "INSERT OR REPLACE INTO site_settings (key, value, updated_at) VALUES (?, ?, ?)"
    )
    .bind(key)
    .bind(value)
    .bind(&now)
    .execute(&state.db)
    .await?;
    Ok(())
}

/// GET /api/notifications/settings - 获取通知设置
pub async fn get_notification_settings(
    State(state): State<Arc<AppState>>,
    cookies: Cookies,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&state, &cookies).await?;
    let settings = load_notification_settings(&state).await;
    
    Ok(Json(json!({
        "code": 200,
        "data": settings
    })))
}

/// POST /api/notifications/settings - 保存通知设置
pub async fn save_notification_settings(
    State(state): State<Arc<AppState>>,
    cookies: Cookies,
    Json(req): Json<NotificationSettings>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&state, &cookies).await?;
    save_setting(&state, "notification_email_enabled", &req.email_enabled.to_string()).await.ok();
    save_setting(&state, "notification_email_host", &req.email_host).await.ok();
    save_setting(&state, "notification_email_port", &req.email_port.to_string()).await.ok();
    save_setting(&state, "notification_email_username", &req.email_username).await.ok();
    save_setting(&state, "notification_email_password", &req.email_password).await.ok();
    save_setting(&state, "notification_email_from_email", &req.email_from_email).await.ok();
    save_setting(&state, "notification_email_from_name", &req.email_from_name).await.ok();
    save_setting(&state, "notification_email_use_tls", &req.email_use_tls.to_string()).await.ok();
    save_setting(&state, "notification_sms_enabled", &req.sms_enabled.to_string()).await.ok();
    save_setting(&state, "notification_sms_access_key_id", &req.sms_access_key_id).await.ok();
    save_setting(&state, "notification_sms_access_key_secret", &req.sms_access_key_secret).await.ok();
    save_setting(&state, "notification_sms_sign_name", &req.sms_sign_name).await.ok();
    save_setting(&state, "notification_sms_template_code", &req.sms_template_code).await.ok();
    
    Ok(Json(json!({
        "code": 200,
        "message": "保存成功"
    })))
}

/// 发送SMTP邮件
pub async fn send_smtp_email(
    settings: &NotificationSettings,
    to_email: &str,
    subject: &str,
    body: &str,
) -> Result<(), String> {
    let email = Message::builder()
        .from(format!("{} <{}>", settings.email_from_name, settings.email_from_email)
            .parse()
            .map_err(|e| format!("发件人地址格式错误: {}", e))?)
        .to(to_email.parse().map_err(|e| format!("收件人地址格式错误: {}", e))?)
        .subject(subject)
        .header(ContentType::TEXT_HTML)
        .body(body.to_string())
        .map_err(|e| format!("构建邮件失败: {}", e))?;

    let creds = Credentials::new(settings.email_username.clone(), settings.email_password.clone());

    let mailer: AsyncSmtpTransport<Tokio1Executor> = if settings.email_use_tls {
        AsyncSmtpTransport::<Tokio1Executor>::relay(&settings.email_host)
            .map_err(|e| format!("SMTP连接失败: {}", e))?
            .port(settings.email_port as u16)
            .credentials(creds)
            .build()
    } else {
        AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&settings.email_host)
            .port(settings.email_port as u16)
            .credentials(creds)
            .build()
    };

    mailer.send(email).await
        .map_err(|e| format!("发送邮件失败: {}", e))?;

    Ok(())
}

/// 发送阿里云融合认证短信（号码认证服务）
pub async fn send_aliyun_sms(
    settings: &NotificationSettings,
    phone_number: &str,
    _template_param: &str,
) -> Result<String, String> {
    let timestamp = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let nonce = Uuid::new_v4().to_string();

    // 使用融合认证短信接口参数
    // TemplateParam使用##code##占位符，让系统自动生成验证码
    let template_param_json = r###"{"code":"##code##","min":"10"}"###;

    let mut params = BTreeMap::new();
    params.insert("AccessKeyId", settings.sms_access_key_id.as_str());
    params.insert("Action", "SendSmsVerifyCode");
    params.insert("Format", "JSON");
    params.insert("PhoneNumber", phone_number);
    params.insert("SignName", settings.sms_sign_name.as_str());
    params.insert("SignatureMethod", "HMAC-SHA1");
    params.insert("SignatureNonce", &nonce);
    params.insert("SignatureVersion", "1.0");
    params.insert("TemplateCode", settings.sms_template_code.as_str());
    params.insert("TemplateParam", template_param_json);
    params.insert("Timestamp", &timestamp);
    params.insert("Version", "2017-05-25");
    params.insert("CodeLength", "6");
    params.insert("ValidTime", "600");
    params.insert("CodeType", "1");
    params.insert("ReturnVerifyCode", "true");

    let query_string: String = params
        .iter()
        .map(|(k, v)| format!("{}={}", url_encode(k), url_encode(v)))
        .collect::<Vec<_>>()
        .join("&");

    let string_to_sign = format!(
        "GET&{}&{}",
        url_encode("/"),
        url_encode(&query_string)
    );

    let key = format!("{}&", settings.sms_access_key_secret);
    type HmacSha1 = Hmac<Sha1>;
    let mut mac = HmacSha1::new_from_slice(key.as_bytes())
        .map_err(|e| format!("HMAC初始化失败: {}", e))?;
    mac.update(string_to_sign.as_bytes());
    let signature = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, mac.finalize().into_bytes());

    let url = format!(
        "https://dypnsapi.aliyuncs.com/?{}&Signature={}",
        query_string,
        url_encode(&signature)
    );

    let client = reqwest::Client::new();
    let response = client.get(&url)
        .send()
        .await
        .map_err(|e| format!("请求失败: {}", e))?;

    let result: Value = response.json().await
        .map_err(|e| format!("解析响应失败: {}", e))?;

    tracing::debug!("阿里云短信API响应: {:?}", result);

    if result.get("Code").and_then(|v| v.as_str()) == Some("OK") {
        // 从Model中获取验证码
        let verify_code = result
            .get("Model")
            .and_then(|m| m.get("VerifyCode"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        Ok(verify_code)
    } else {
        Err(format!("短信发送失败: {}", result.get("Message").and_then(|v| v.as_str()).unwrap_or("未知错误")))
    }
}

fn url_encode(s: &str) -> String {
    urlencoding::encode(s)
        .replace('+', "%20")
        .replace('*', "%2A")
        .replace("%7E", "~")
}

/// POST /api/notifications/test/email - 测试邮件发送
pub async fn test_email(
    State(state): State<Arc<AppState>>,
    cookies: Cookies,
    Json(req): Json<TestRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&state, &cookies).await?;
    let settings = load_notification_settings(&state).await;
    
    if !settings.email_enabled {
        return Ok(Json(json!({
            "code": 400,
            "message": "邮箱通知未启用"
        })));
    }

    let subject = "YaoList 测试邮件";
    let body = r#"
        <div style="font-family: Arial, sans-serif; max-width: 600px; margin: 0 auto; padding: 20px;">
            <h2 style="color: #333;">测试邮件</h2>
            <p>这是一封来自 YaoList 的测试邮件。</p>
            <p>如果您收到此邮件，说明SMTP配置正确。</p>
        </div>
    "#;

    match send_smtp_email(&settings, &req.target, subject, body).await {
        Ok(_) => Ok(Json(json!({
            "code": 200,
            "message": "测试邮件发送成功"
        }))),
        Err(e) => Ok(Json(json!({
            "code": 500,
            "message": format!("发送失败: {}", e)
        })))
    }
}

/// POST /api/notifications/test/sms - 测试短信发送
pub async fn test_sms(
    State(state): State<Arc<AppState>>,
    cookies: Cookies,
    Json(req): Json<TestRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&state, &cookies).await?;
    let settings = load_notification_settings(&state).await;
    
    if !settings.sms_enabled {
        return Ok(Json(json!({
            "code": 400,
            "message": "短信通知未启用"
        })));
    }

    match send_aliyun_sms(&settings, &req.target, "").await {
        Ok(code) => Ok(Json(json!({
            "code": 200,
            "message": format!("测试短信发送成功，验证码: {}", code)
        }))),
        Err(e) => Ok(Json(json!({
            "code": 500,
            "message": format!("发送失败: {}", e)
        })))
    }
}

/// POST /api/notifications/send-code - 发送验证码（需要图形验证码）
pub async fn send_verification_code(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SendCodeRequest>,
) -> Result<Json<Value>, StatusCode> {
    // 验证图形验证码
    match (&req.captcha_id, &req.captcha_code) {
        (Some(id), Some(code)) if !id.is_empty() && !code.is_empty() => {
            if !state.login_security.verify_captcha(id, code) {
                return Ok(Json(json!({
                    "code": 400,
                    "message": "图形验证码错误"
                })));
            }
        }
        _ => {
            return Ok(Json(json!({
                "code": 400,
                "message": "请输入图形验证码"
            })));
        }
    }

    let settings = load_notification_settings(&state).await;
    let id = Uuid::new_v4().to_string();
    let now = Utc::now();
    let expires_at = (now + chrono::Duration::minutes(10)).to_rfc3339();
    let created_at = now.to_rfc3339();

    // 根据类型发送验证码
    let (code, send_error) = match req.send_type.as_str() {
        "email" => {
            if !settings.email_enabled {
                return Ok(Json(json!({
                    "code": 400,
                    "message": "邮箱通知未启用"
                })));
            }

            // 邮件验证码由本地生成
            let code = format!("{:06}", rand::random::<u32>() % 1000000);
            let subject = "YaoList 验证码";
            let body = format!(r#"
                <div style="font-family: Arial, sans-serif; max-width: 600px; margin: 0 auto; padding: 20px;">
                    <h2 style="color: #333;">验证码</h2>
                    <p>您的验证码是：</p>
                    <div style="background: #f5f5f5; padding: 20px; border-radius: 8px; text-align: center; margin: 20px 0;">
                        <span style="font-size: 32px; font-weight: bold; letter-spacing: 8px;">{}</span>
                    </div>
                    <p style="color: #999;">验证码有效期为10分钟，请勿泄露给他人。</p>
                </div>
            "#, code);

            match send_smtp_email(&settings, &req.target, subject, &body).await {
                Ok(_) => (code, None),
                Err(e) => (String::new(), Some(e))
            }
        }
        "sms" => {
            if !settings.sms_enabled {
                return Ok(Json(json!({
                    "code": 400,
                    "message": "短信通知未启用"
                })));
            }

            // 短信验证码由阿里云融合认证接口生成
            match send_aliyun_sms(&settings, &req.target, "").await {
                Ok(code) => (code, None),
                Err(e) => (String::new(), Some(e))
            }
        }
        _ => return Ok(Json(json!({
            "code": 400,
            "message": "不支持的验证码类型"
        })))
    };

    if let Some(e) = send_error {
        return Ok(Json(json!({
            "code": 500,
            "message": format!("发送失败: {}", e)
        })));
    }

    sqlx::query(
        "INSERT INTO verification_codes (id, user_id, target, code, type, expires_at, used, created_at) VALUES (?, NULL, ?, ?, ?, ?, 0, ?)"
    )
    .bind(&id)
    .bind(&req.target)
    .bind(&code)
    .bind(&req.send_type)
    .bind(&expires_at)
    .bind(&created_at)
    .execute(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(json!({
        "code": 200,
        "message": "验证码已发送"
    })))
}

#[derive(Debug, Deserialize)]
pub struct SendCodeRequest {
    pub target: String,
    #[serde(rename = "type")]
    pub send_type: String,
    pub captcha_id: Option<String>,
    pub captcha_code: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct VerifyCodeRequest {
    pub target: String,
    #[serde(rename = "type")]
    pub code_type: String,
    pub code: String,
}

/// POST /api/notifications/verify-code - 验证验证码
pub async fn verify_code(
    State(state): State<Arc<AppState>>,
    Json(req): Json<VerifyCodeRequest>,
) -> Result<Json<Value>, StatusCode> {
    let now = Utc::now().to_rfc3339();

    let result = sqlx::query_as::<_, (String, String, i32)>(
        "SELECT id, code, used FROM verification_codes WHERE target = ? AND type = ? AND expires_at > ? ORDER BY created_at DESC LIMIT 1"
    )
    .bind(&req.target)
    .bind(&req.code_type)
    .bind(&now)
    .fetch_optional(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    match result {
        Some((id, stored_code, used)) => {
            if used == 1 {
                return Ok(Json(json!({
                    "code": 400,
                    "message": "验证码已使用"
                })));
            }

            if stored_code != req.code {
                return Ok(Json(json!({
                    "code": 400,
                    "message": "验证码错误"
                })));
            }

            sqlx::query("UPDATE verification_codes SET used = 1 WHERE id = ?")
                .bind(&id)
                .execute(&state.db)
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

            Ok(Json(json!({
                "code": 200,
                "message": "验证成功"
            })))
        }
        None => Ok(Json(json!({
            "code": 400,
            "message": "验证码不存在或已过期"
        })))
    }
}
