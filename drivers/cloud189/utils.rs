//! Cloud189 utility functions / 天翼云盘工具函数

use anyhow::{anyhow, Result};
use chrono::Utc;
use hmac::{Hmac, Mac};
use sha1::Sha1;
use rsa::{RsaPublicKey, Pkcs1v15Encrypt};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use rsa::pkcs8::DecodePublicKey;
use rand::SeedableRng;
use std::collections::HashMap;

use super::types::*;

// API constants / API常量
pub const WEB_URL: &str = "https://cloud.189.cn";
pub const AUTH_URL: &str = "https://open.e.189.cn";
pub const API_URL: &str = "https://api.cloud.189.cn";
pub const UPLOAD_URL: &str = "https://upload.cloud.189.cn";
pub const RETURN_URL: &str = "https://m.cloud.189.cn/zhuanti/2020/loginErrorPc/index.html";

pub const APP_ID: &str = "8025431004";
pub const CLIENT_TYPE: &str = "10020";
pub const ACCOUNT_TYPE: &str = "02";
pub const VERSION: &str = "6.2";
pub const PC: &str = "TELEPC";
pub const CHANNEL_ID: &str = "web_cloud.189.cn";

/// Client suffix parameters - copied from clientSuffix / 客户端后缀参数
pub fn client_suffix() -> Vec<(String, String)> {
    let rand_val = format!("{}_{}", 
        rand::random::<u32>() % 100000,
        rand::random::<u64>() % 10000000000
    );
    vec![
        ("clientType".to_string(), PC.to_string()),
        ("version".to_string(), VERSION.to_string()),
        ("channelId".to_string(), CHANNEL_ID.to_string()),
        ("rand".to_string(), rand_val),
    ]
}

/// Get HTTP date string - copied from getHttpDateStr / 获取HTTP日期字符串
pub fn get_http_date_str() -> String {
    Utc::now().format("%a, %d %b %Y %H:%M:%S GMT").to_string()
}

/// Timestamp in milliseconds / 时间戳毫秒
pub fn timestamp() -> i64 {
    Utc::now().timestamp_millis()
}

/// HMAC-SHA1 signature - copied from signatureOfHmac / HMAC-SHA1签名
pub fn signature_of_hmac(
    session_secret: &str,
    session_key: &str,
    operate: &str,
    full_url: &str,
    date_of_gmt: &str,
    param: &str,
) -> String {
    // Extract URL path - copied from regex `://[^/]+((/[^/\s?#]+)*)` / 提取URL路径
    let url_path = if let Some(pos) = full_url.find("://") {
        let rest = &full_url[pos + 3..];
        if let Some(slash_pos) = rest.find('/') {
            let path_part = &rest[slash_pos..];
            // Remove query parameters / 去掉查询参数
            if let Some(q_pos) = path_part.find('?') {
                &path_part[..q_pos]
            } else {
                path_part
            }
        } else {
            "/"
        }
    } else {
        "/"
    };

    let data = if param.is_empty() {
        format!(
            "SessionKey={}&Operate={}&RequestURI={}&Date={}",
            session_key, operate, url_path, date_of_gmt
        )
    } else {
        format!(
            "SessionKey={}&Operate={}&RequestURI={}&Date={}&params={}",
            session_key, operate, url_path, date_of_gmt, param
        )
    };

    type HmacSha1 = Hmac<Sha1>;
    let mut mac = HmacSha1::new_from_slice(session_secret.as_bytes()).unwrap();
    mac.update(data.as_bytes());
    hex::encode(mac.finalize().into_bytes()).to_uppercase()
}

/// RSA encryption - copied from RsaEncrypt / RSA加密
pub fn rsa_encrypt(public_key: &str, orig_data: &str) -> Result<String> {
    // Cloud189 returns pure base64 DER format public key / 天翼云返回的是纯base64的DER格式公钥
    let pub_key_der = BASE64.decode(public_key)
        .map_err(|e| anyhow!("Failed to decode public key base64 / 解码公钥base64失败: {}", e))?;
    
    let rsa_key = RsaPublicKey::from_public_key_der(&pub_key_der)
        .map_err(|e| anyhow!("Failed to parse RSA public key / 解析RSA公钥失败: {}", e))?;

    let mut rng = rand::rngs::StdRng::from_entropy();
    let encrypted = rsa_key.encrypt(&mut rng, Pkcs1v15Encrypt, orig_data.as_bytes())
        .map_err(|e| anyhow!("RSA encryption failed / RSA加密失败: {}", e))?;

    Ok(hex::encode(&encrypted).to_uppercase())
}

/// AES ECB encryption - copied from AesECBEncrypt / AES ECB加密
pub fn aes_ecb_encrypt(data: &str, key: &str) -> String {
    use aes::cipher::{BlockEncrypt, KeyInit, generic_array::GenericArray};
    use aes::Aes128;

    let key_bytes = key.as_bytes();
    if key_bytes.len() < 16 {
        return String::new();
    }
    
    let cipher = Aes128::new(GenericArray::from_slice(&key_bytes[..16]));

    // PKCS7 padding / PKCS7填充
    let data_bytes = data.as_bytes();
    let padding_len = 16 - (data_bytes.len() % 16);
    let mut padded = data_bytes.to_vec();
    padded.extend(vec![padding_len as u8; padding_len]);

    let mut result = Vec::new();
    for chunk in padded.chunks(16) {
        let mut block = GenericArray::clone_from_slice(chunk);
        cipher.encrypt_block(&mut block);
        result.extend_from_slice(&block);
    }

    hex::encode(result).to_uppercase()
}

/// Sort order conversion - copied from toFamilyOrderBy / 排序转换
pub fn to_family_order_by(order_by: &str) -> &'static str {
    match order_by {
        "filename" => "1",
        "filesize" => "2",
        "lastOpTime" => "3",
        _ => "1",
    }
}

/// Sort direction conversion - copied from toDesc / 排序方向转换
pub fn to_desc(order_direction: &str) -> &'static str {
    match order_direction {
        "desc" => "true",
        _ => "false",
    }
}

/// Parse HTTP Header - copied from ParseHttpHeader / 解析HTTP Header
pub fn parse_http_header(str: &str) -> HashMap<String, String> {
    let mut header = HashMap::new();
    for value in str.split('&') {
        if let Some((k, v)) = value.split_once('=') {
            header.insert(k.to_string(), v.to_string());
        }
    }
    header
}

/// Calculate chunk size - copied from partSize / 计算分片大小
/// Chunk count limits: 10MIB 20MIB 999 chunks, 50MIB+ 1999 chunks / 对分片数量有限制
pub fn part_size(size: i64) -> i64 {
    const DEFAULT: i64 = 1024 * 1024 * 10; // 10MIB
    if size > DEFAULT * 2 * 999 {
        let slice_size = (size as f64 / 1999.0).ceil();
        let rate = (slice_size / DEFAULT as f64).ceil().max(5.0);
        return (rate * DEFAULT as f64) as i64;
    }
    if size > DEFAULT * 999 {
        return DEFAULT * 2; // 20MIB
    }
    DEFAULT
}

/// Convert bool to number - copied from BoolToNumber / bool转数字
pub fn bool_to_number(b: bool) -> i32 {
    if b { 1 } else { 0 }
}

/// Parse session response (supports JSON and XML) / 解析Session响应
pub fn parse_session_response(text: &str) -> Result<AppSessionResp> {
    // Try JSON first / 先尝试JSON
    if let Ok(info) = serde_json::from_str::<AppSessionResp>(text) {
        if !info.user_session.session_key.is_empty() {
            return Ok(info);
        }
    }
    // Then try XML / 再尝试XML
    if text.contains("<?xml") || text.contains("<userSession>") {
        let xml_info: AppSessionRespXml = quick_xml::de::from_str(text)
            .map_err(|e| anyhow!("Failed to parse XML Session / 解析XML Session失败: {} - {}", e, text))?;
        return Ok(xml_info.into());
    }
    Err(anyhow!("Cannot parse session response / 无法解析Session响应: {}", text))
}

/// Encryption parameters - copied from EncryptParams / 加密参数
pub fn encrypt_params(params: &[(&str, &str)], session_secret: &str) -> String {
    if params.is_empty() || session_secret.len() < 16 {
        return String::new();
    }

    // Sort parameters / 排序参数
    let mut sorted_params: Vec<_> = params.to_vec();
    sorted_params.sort_by(|a, b| a.0.cmp(b.0));

    let param_str: String = sorted_params
        .iter()
        .map(|(k, v)| format!("{}={}", k, v))
        .collect::<Vec<_>>()
        .join("&");

    aes_ecb_encrypt(&param_str, &session_secret[..16])
}

/// 排序参数并编码 - 照抄Params.Encode
pub fn encode_params(params: &[(&str, &str)]) -> String {
    if params.is_empty() {
        return String::new();
    }
    let mut sorted: Vec<_> = params.to_vec();
    sorted.sort_by(|a, b| a.0.cmp(b.0));
    sorted.iter()
        .map(|(k, v)| format!("{}={}", k, v))
        .collect::<Vec<_>>()
        .join("&")
}
