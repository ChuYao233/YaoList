//! 蓝奏云工具函数

use anyhow::{anyhow, Result};
use regex::Regex;
use reqwest::{Client, header::{HeaderMap, HeaderValue, COOKIE, USER_AGENT, REFERER}};
use std::collections::HashMap;

/// 蓝奏云API基础URL
pub const BASE_URL: &str = "https://pc.woozooo.com";
pub const UPLOAD_URL: &str = "https://up.woozooo.com";
pub const SHARE_URL: &str = "https://pan.lanzoui.com";
pub const LOGIN_URL: &str = "https://up.woozooo.com/mlogin.php";

/// 计算 acw_sc__v2 值（蓝奏云反爬虫）
pub fn calc_acw_sc_v2(arg1: &str) -> String {
    let key = "3000176000856006061501533003690027800375";
    hex_xor(unsbox(arg1), key)
}

fn unsbox(arg: &str) -> String {
    let v2: Vec<i32> = vec![
        15, 35, 29, 24, 33, 16, 1, 38, 10, 9, 19, 31, 40, 27, 22, 23, 25, 13, 6, 11,
        39, 18, 20, 8, 14, 21, 32, 26, 2, 30, 7, 4, 17, 5, 3, 28, 34, 37, 12, 36,
    ];
    
    let mut v3: Vec<char> = vec![' '; arg.len()];
    let chars: Vec<char> = arg.chars().collect();
    
    for (i, &idx) in v2.iter().enumerate() {
        if (idx as usize) <= chars.len() && i < chars.len() {
            v3[(idx - 1) as usize] = chars[i];
        }
    }
    
    v3.into_iter().collect()
}

fn hex_xor(a: String, b: &str) -> String {
    let a_chars: Vec<char> = a.chars().collect();
    let b_chars: Vec<char> = b.chars().collect();
    let mut result = String::new();
    
    let mut i = 0;
    while i < a_chars.len() && i < b_chars.len() {
        let a_val = u8::from_str_radix(&a[i..i+2].to_string(), 16).unwrap_or(0);
        let b_val = u8::from_str_radix(&b[i..i+2].to_string(), 16).unwrap_or(0);
        let xor_val = a_val ^ b_val;
        result.push_str(&format!("{:02x}", xor_val));
        i += 2;
    }
    
    result
}

/// 从HTML中提取JavaScript变量值
pub fn extract_js_var(html: &str, var_name: &str) -> Option<String> {
    let patterns = [
        format!(r#"var\s+{}\s*=\s*'([^']+)'"#, var_name),
        format!(r#"var\s+{}\s*=\s*"([^"]+)""#, var_name),
        format!(r#"{}\s*=\s*'([^']+)'"#, var_name),
        format!(r#"{}\s*=\s*"([^"]+)""#, var_name),
    ];
    
    for pattern in patterns {
        if let Ok(re) = Regex::new(&pattern) {
            if let Some(caps) = re.captures(html) {
                if let Some(m) = caps.get(1) {
                    return Some(m.as_str().to_string());
                }
            }
        }
    }
    
    None
}

/// 从HTML中提取JSON数据
pub fn extract_json_from_html(html: &str, func_name: &str) -> Option<String> {
    // 查找函数调用中的JSON参数
    let pattern = format!(r#"{}\s*\(\s*(\{{[^}}]+\}})\s*\)"#, func_name);
    if let Ok(re) = Regex::new(&pattern) {
        if let Some(caps) = re.captures(html) {
            if let Some(m) = caps.get(1) {
                return Some(m.as_str().to_string());
            }
        }
    }
    None
}

/// 移除HTML注释
pub fn remove_html_comments(html: &str) -> String {
    let re = Regex::new(r"<!--[\s\S]*?-->").unwrap();
    re.replace_all(html, "").to_string()
}

/// 从URL中提取文件ID
pub fn extract_file_id(url: &str) -> Option<String> {
    // 格式: https://xxx.lanzou?.com/xxxxx 或 https://xxx.lanzou?.com/xxxxx?pwd=xxxx
    let re = Regex::new(r"lanzou[a-z]\.com/([a-zA-Z0-9]+)").ok()?;
    re.captures(url).and_then(|caps| caps.get(1).map(|m| m.as_str().to_string()))
}

/// 从URL中提取密码
pub fn extract_password(url: &str) -> Option<String> {
    if let Some(pos) = url.find("pwd=") {
        let start = pos + 4;
        let end = url[start..].find('&').map(|p| start + p).unwrap_or(url.len());
        return Some(url[start..end].to_string());
    }
    None
}

/// 获取下载链接的过期时间
pub fn get_expiration_time(url: &str) -> Option<i64> {
    // 从URL参数中提取时间戳
    let re = Regex::new(r"[?&]t=(\d+)").ok()?;
    re.captures(url).and_then(|caps| {
        caps.get(1).and_then(|m| m.as_str().parse::<i64>().ok())
    })
}

/// 构建默认请求头
pub fn build_headers(user_agent: &str, cookie: Option<&str>) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(USER_AGENT, HeaderValue::from_str(user_agent).unwrap_or_else(|_| {
        HeaderValue::from_static("Mozilla/5.0")
    }));
    headers.insert("Accept", HeaderValue::from_static("application/json, text/javascript, */*"));
    headers.insert("Accept-Language", HeaderValue::from_static("zh-CN,zh;q=0.9"));
    
    if let Some(c) = cookie {
        if let Ok(v) = HeaderValue::from_str(c) {
            headers.insert(COOKIE, v);
        }
    }
    
    headers
}

/// 构建带Referer的请求头
pub fn build_headers_with_referer(user_agent: &str, cookie: Option<&str>, referer: &str) -> HeaderMap {
    let mut headers = build_headers(user_agent, cookie);
    if let Ok(v) = HeaderValue::from_str(referer) {
        headers.insert(REFERER, v);
    }
    headers
}

/// 解析分享页面获取文件信息
pub async fn parse_share_page(client: &Client, url: &str, password: Option<&str>, user_agent: &str) -> Result<SharePageInfo> {
    let headers = build_headers(user_agent, None);
    
    let response = client.get(url)
        .headers(headers.clone())
        .send()
        .await?;
    
    let html = response.text().await?;
    
    // 检查是否需要acw_sc__v2验证
    if html.contains("acw_sc__v2") {
        let arg1 = extract_js_var(&html, "arg1")
            .ok_or_else(|| anyhow!("无法提取arg1"))?;
        let acw_value = calc_acw_sc_v2(&arg1);
        
        // 带cookie重新请求
        let cookie = format!("acw_sc__v2={}", acw_value);
        let headers = build_headers(user_agent, Some(&cookie));
        
        let response = client.get(url)
            .headers(headers)
            .send()
            .await?;
        
        let html = response.text().await?;
        return parse_share_html(&html, password);
    }
    
    parse_share_html(&html, password)
}

/// 分享页面信息
#[derive(Debug)]
pub struct SharePageInfo {
    pub is_folder: bool,
    pub name: String,
    pub file_id: Option<String>,
    pub folder_id: Option<String>,
    pub sign: Option<String>,
    pub sign_t: Option<String>,
}

fn parse_share_html(html: &str, _password: Option<&str>) -> Result<SharePageInfo> {
    let html = remove_html_comments(html);
    
    // 检查是否是文件夹
    let is_folder = html.contains("文件夹") || html.contains("id=\"filemore\"");
    
    // 提取名称
    let name = extract_js_var(&html, "filename")
        .or_else(|| extract_title_from_html(&html))
        .unwrap_or_else(|| "未知".to_string());
    
    // 提取文件ID
    let file_id = extract_js_var(&html, "file_id")
        .or_else(|| extract_js_var(&html, "surl"));
    
    // 提取文件夹ID
    let folder_id = extract_js_var(&html, "folder_id");
    
    // 提取sign
    let sign = extract_js_var(&html, "sign");
    let sign_t = extract_js_var(&html, "websignkey");
    
    Ok(SharePageInfo {
        is_folder,
        name,
        file_id,
        folder_id,
        sign,
        sign_t,
    })
}

fn extract_title_from_html(html: &str) -> Option<String> {
    let re = Regex::new(r"<title>([^<]+)</title>").ok()?;
    re.captures(html).and_then(|caps| {
        caps.get(1).map(|m| {
            let title = m.as_str();
            // 移除"蓝奏云"等后缀
            title.split(" - ").next()
                .unwrap_or(title)
                .trim()
                .to_string()
        })
    })
}

/// 解析文件下载页面获取真实链接
pub async fn get_download_url(
    client: &Client,
    file_id: &str,
    sign: &str,
    sign_t: &str,
    password: Option<&str>,
    user_agent: &str,
) -> Result<String> {
    let url = format!("{}/ajaxm.php", BASE_URL);
    
    let mut params = HashMap::new();
    params.insert("action", "downprocess");
    params.insert("sign", sign);
    params.insert("signs", sign_t);
    params.insert("ves", "1");
    if let Some(pwd) = password {
        params.insert("p", pwd);
    }
    
    let headers = build_headers_with_referer(user_agent, None, &format!("{}/{}", BASE_URL, file_id));
    
    let response = client.post(&url)
        .headers(headers)
        .form(&params)
        .send()
        .await?;
    
    let json: serde_json::Value = response.json().await?;
    
    let zt = json.get("zt").and_then(|v| v.as_i64()).unwrap_or(0);
    if zt != 1 {
        let inf = json.get("inf").and_then(|v| v.as_str()).unwrap_or("未知错误");
        return Err(anyhow!("获取下载链接失败: {}", inf));
    }
    
    let dom = json.get("dom").and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("缺少dom字段"))?;
    let url_path = json.get("url").and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("缺少url字段"))?;
    
    Ok(format!("{}{}", dom, url_path))
}

/// 获取最终下载链接（跟随302重定向）
pub async fn get_final_download_url(client: &Client, url: &str, user_agent: &str) -> Result<String> {
    let headers = build_headers(user_agent, None);
    
    let response = client.get(url)
        .headers(headers)
        .send()
        .await?;
    
    // 获取最终URL（跟随重定向后的）
    Ok(response.url().to_string())
}

/// 从HTML中计算acw_sc__v2值
pub fn calc_acw_sc_v2_from_html(html: &str) -> Result<String> {
    let re = Regex::new(r"arg1='([0-9A-Z]+)'").unwrap();
    let arg1 = re.captures(html)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str())
        .ok_or_else(|| anyhow!("无法匹配arg1参数"))?;
    Ok(calc_acw_sc_v2(arg1))
}

/// 移除JS注释
pub fn remove_js_comments(html: &str) -> String {
    let mut result = String::with_capacity(html.len());
    let mut in_comment = false;
    let mut in_single_line = false;
    let chars: Vec<char> = html.chars().collect();
    let mut i = 0;
    
    while i < chars.len() {
        if in_single_line {
            if chars[i] == '\n' || chars[i] == '\r' {
                in_single_line = false;
                result.push(chars[i]);
            }
            i += 1;
            continue;
        }
        if in_comment {
            if chars[i] == '*' && i + 1 < chars.len() && chars[i + 1] == '/' {
                in_comment = false;
                i += 2;
                continue;
            }
            i += 1;
            continue;
        }
        if chars[i] == '/' && i + 1 < chars.len() {
            if chars[i + 1] == '*' {
                in_comment = true;
                i += 2;
                continue;
            } else if chars[i + 1] == '/' {
                in_single_line = true;
                i += 2;
                continue;
            }
        }
        result.push(chars[i]);
        i += 1;
    }
    
    result
}
