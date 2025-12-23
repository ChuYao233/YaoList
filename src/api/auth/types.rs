use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
    pub captcha_id: Option<String>,
    pub captcha_code: Option<String>,
    pub totp_code: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct CaptchaResponse {
    pub captcha_id: String,
    pub captcha_image: String,
}


#[derive(Debug, Deserialize)]
pub struct RegisterRequest {
    pub username: String,
    pub password: String,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub verification_code: String,
    pub verification_type: String, // "email" or "sms"
}

#[derive(Debug, Deserialize)]
pub struct CheckUniqueRequest {
    pub username: Option<String>,
    pub email: Option<String>,
    pub phone: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ForgotPasswordRequest {
    pub target: String,           // 邮箱或手机号
    pub target_type: String,      // "email" 或 "sms"
    pub captcha_id: String,
    pub captcha_code: String,
}

#[derive(Debug, Deserialize)]
pub struct ResetPasswordRequest {
    pub target: String,
    pub target_type: String,
    pub verification_code: String,
    pub new_password: String,
}

#[derive(Debug, Deserialize)]
pub struct ChangePasswordRequest {
    pub current_password: String,
    pub new_password: String,
}

#[derive(Debug, Deserialize)]
pub struct UpdateEmailRequest {
    pub email: String,
    pub verification_code: String,
}

#[derive(Debug, Deserialize)]
pub struct UpdatePhoneRequest {
    pub phone: String,
    pub verification_code: String,
}

#[derive(Debug, Deserialize)]
pub struct Enable2FARequest {
    pub totp_code: String,
}

#[derive(Debug, Deserialize)]
pub struct Verify2FARequest {
    pub totp_code: String,
}
