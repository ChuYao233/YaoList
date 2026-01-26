//! 139云盘工具函数 / 139Yun utility functions

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use chrono::{TimeZone, Utc};
use sha1::{Sha1, Digest as Sha1Digest};
use sha2::{Sha256, Digest as Sha256Digest};
use aes::Aes128;
use aes::cipher::{BlockDecrypt, BlockEncrypt, KeyInit, generic_array::GenericArray};
use rand::Rng;

/// AES密钥(第一层) / AES key (layer 1)
pub const KEY_HEX_1: &str = "73634235495062495331515373756c734e7253306c673d3d";
/// AES密钥(第二层) / AES key (layer 2)  
pub const KEY_HEX_2: &str = "7150714477323633586746674c337538";

/// URL编码(类似JavaScript的encodeURIComponent) / URL encode like JavaScript's encodeURIComponent
pub fn encode_uri_component(s: &str) -> String {
    let encoded = urlencoding::encode(s).to_string();
    encoded
        .replace('+', "%20")
        .replace("%21", "!")
        .replace("%27", "'")
        .replace("%28", "(")
        .replace("%29", ")")
        .replace("%2A", "*")
}

/// 计算签名 / Calculate signature
pub fn calc_sign(body: &str, ts: &str, rand_str: &str) -> String {
    let body = encode_uri_component(body);
    let mut chars: Vec<char> = body.chars().collect();
    chars.sort();
    let sorted_body: String = chars.into_iter().collect();
    let body_base64 = BASE64.encode(sorted_body.as_bytes());
    
    let md5_body = md5_hex(&body_base64);
    let md5_ts_rand = md5_hex(&format!("{}:{}", ts, rand_str));
    let combined = format!("{}{}", md5_body, md5_ts_rand);
    md5_hex(&combined).to_uppercase()
}

/// MD5哈希(十六进制) / MD5 hash (hex)
pub fn md5_hex(data: &str) -> String {
    format!("{:x}", md5::compute(data.as_bytes()))
}

/// SHA1哈希(十六进制) / SHA1 hash (hex)
pub fn sha1_hex(data: &str) -> String {
    let mut hasher = Sha1::new();
    hasher.update(data.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// SHA256哈希(十六进制) / SHA256 hash (hex)
pub fn sha256_hex(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    format!("{:x}", hasher.finalize())
}

/// 解析时间字符串(格式: 20060102150405) / Parse time string (format: 20060102150405)
pub fn parse_time(t: &str) -> chrono::DateTime<Utc> {
    if t.len() >= 14 {
        let year: i32 = t[0..4].parse().unwrap_or(1970);
        let month: u32 = t[4..6].parse().unwrap_or(1);
        let day: u32 = t[6..8].parse().unwrap_or(1);
        let hour: u32 = t[8..10].parse().unwrap_or(0);
        let min: u32 = t[10..12].parse().unwrap_or(0);
        let sec: u32 = t[12..14].parse().unwrap_or(0);
        
        Utc.with_ymd_and_hms(year, month, day, hour, min, sec)
            .single()
            .unwrap_or_else(|| Utc::now())
    } else {
        Utc::now()
    }
}

/// 解析个人版时间字符串(格式: 2006-01-02T15:04:05.999-07:00) / Parse personal time string
pub fn parse_personal_time(t: &str) -> chrono::DateTime<Utc> {
    chrono::DateTime::parse_from_rfc3339(t)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now())
}

/// 生成随机字符串 / Generate random string
pub fn random_string(len: usize) -> String {
    const CHARSET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let mut rng = rand::thread_rng();
    (0..len)
        .map(|_| {
            let idx = rng.gen_range(0..CHARSET.len());
            CHARSET[idx] as char
        })
        .collect()
}

/// 获取当前时间字符串(格式: 2006-01-02 15:04:05) / Get current time string
pub fn get_timestamp() -> String {
    Utc::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

/// Unicode转义 / Unicode escape
pub fn unicode_escape(s: &str) -> String {
    let mut result = String::new();
    for c in s.chars() {
        if c.is_ascii() {
            result.push(c);
        } else {
            for u in c.encode_utf16(&mut [0; 2]) {
                result.push_str(&format!("\\u{:04x}", u));
            }
        }
    }
    result
}

/// PKCS7填充 / PKCS7 padding
pub fn pkcs7_pad(data: &[u8], block_size: usize) -> Vec<u8> {
    let padding = block_size - (data.len() % block_size);
    let mut padded = data.to_vec();
    padded.extend(vec![padding as u8; padding]);
    padded
}

/// PKCS7去填充 / PKCS7 unpad
pub fn pkcs7_unpad(data: &[u8]) -> Option<Vec<u8>> {
    if data.is_empty() {
        return None;
    }
    let padding = data[data.len() - 1] as usize;
    if padding > data.len() || padding > 16 {
        return None;
    }
    Some(data[..data.len() - padding].to_vec())
}

/// AES ECB解密 / AES ECB decrypt
pub fn aes_ecb_decrypt(ciphertext: &[u8], key: &[u8]) -> Option<Vec<u8>> {
    if key.len() != 16 || ciphertext.len() % 16 != 0 {
        return None;
    }
    
    let cipher = Aes128::new(GenericArray::from_slice(key));
    let mut decrypted = Vec::with_capacity(ciphertext.len());
    
    for chunk in ciphertext.chunks(16) {
        let mut block = GenericArray::clone_from_slice(chunk);
        cipher.decrypt_block(&mut block);
        decrypted.extend_from_slice(&block);
    }
    
    pkcs7_unpad(&decrypted)
}

/// AES CBC加密 / AES CBC encrypt
pub fn aes_cbc_encrypt(plaintext: &[u8], key: &[u8], iv: &[u8]) -> Option<Vec<u8>> {
    if key.len() != 16 || iv.len() != 16 {
        return None;
    }
    
    let cipher = Aes128::new(GenericArray::from_slice(key));
    let padded = pkcs7_pad(plaintext, 16);
    let mut result = Vec::with_capacity(padded.len());
    let mut prev_block = iv.to_vec();
    
    for chunk in padded.chunks(16) {
        let mut block: Vec<u8> = chunk.iter()
            .zip(prev_block.iter())
            .map(|(a, b)| a ^ b)
            .collect();
        let mut block_arr = GenericArray::clone_from_slice(&block);
        cipher.encrypt_block(&mut block_arr);
        block = block_arr.to_vec();
        result.extend_from_slice(&block);
        prev_block = block;
    }
    
    Some(result)
}

/// AES CBC解密 / AES CBC decrypt
pub fn aes_cbc_decrypt(ciphertext: &[u8], key: &[u8], iv: &[u8]) -> Option<Vec<u8>> {
    if key.len() != 16 || iv.len() != 16 || ciphertext.len() % 16 != 0 {
        return None;
    }
    
    let cipher = Aes128::new(GenericArray::from_slice(key));
    let mut result = Vec::with_capacity(ciphertext.len());
    let mut prev_block = iv.to_vec();
    
    for chunk in ciphertext.chunks(16) {
        let mut block = GenericArray::clone_from_slice(chunk);
        cipher.decrypt_block(&mut block);
        let decrypted: Vec<u8> = block.iter()
            .zip(prev_block.iter())
            .map(|(a, b)| a ^ b)
            .collect();
        result.extend_from_slice(&decrypted);
        prev_block = chunk.to_vec();
    }
    
    pkcs7_unpad(&result)
}

/// 计算分片大小 / Calculate part size
/// 默认16MB，确保内存占用<50MB
pub fn get_part_size(_size: i64, custom_size: i64) -> i64 {
    if custom_size != 0 {
        return custom_size;
    }
    // 默认16MB分片，保持内存占用合理
    16 * 1024 * 1024
}

/// 修正路径 / Fix path
pub fn fix_path(path: &str) -> String {
    let path = path.trim();
    if path.is_empty() || path == "/" {
        return "/".to_string();
    }
    let path = if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{}", path)
    };
    path.trim_end_matches('/').to_string()
}

/// 获取父路径 / Get parent path
pub fn get_parent_path(path: &str) -> String {
    let path = fix_path(path);
    if path == "/" {
        return "/".to_string();
    }
    match path.rfind('/') {
        Some(0) => "/".to_string(),
        Some(pos) => path[..pos].to_string(),
        None => "/".to_string(),
    }
}

/// 获取文件名 / Get file name
pub fn get_file_name(path: &str) -> String {
    let path = fix_path(path);
    match path.rfind('/') {
        Some(pos) => path[pos + 1..].to_string(),
        None => path,
    }
}

/// 连接路径 / Join path
pub fn join_path(base: &str, name: &str) -> String {
    let base = fix_path(base);
    if base == "/" {
        format!("/{}", name)
    } else {
        format!("{}/{}", base, name)
    }
}

/// 解码Authorization获取账号 / Decode authorization to get account
pub fn decode_authorization(auth: &str) -> Option<(String, String, String)> {
    let decoded = BASE64.decode(auth).ok()?;
    let decoded_str = String::from_utf8(decoded).ok()?;
    let parts: Vec<&str> = decoded_str.split(':').collect();
    if parts.len() >= 3 {
        Some((
            parts[0].to_string(),
            parts[1].to_string(),
            parts[2].to_string(),
        ))
    } else {
        None
    }
}

/// 编码Authorization / Encode authorization
pub fn encode_authorization(prefix: &str, account: &str, token: &str) -> String {
    BASE64.encode(format!("{}:{}:{}", prefix, account, token).as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_md5_hex() {
        assert_eq!(md5_hex("test"), "098f6bcd4621d373cade4e832627b4f6");
    }

    #[test]
    fn test_sha1_hex() {
        let result = sha1_hex("fetion.com.cn:password");
        assert!(!result.is_empty());
    }

    #[test]
    fn test_random_string() {
        let s = random_string(16);
        assert_eq!(s.len(), 16);
    }
}
