//! 用户统计服务
//! 统计用户请求数和流量

use sqlx::SqlitePool;

/// 增加用户请求数（不统计流量，用于预览等场景）
pub async fn record_request(pool: &SqlitePool, user_id: &str) {
    if let Err(e) = sqlx::query(
        "UPDATE users SET total_requests = total_requests + 1 WHERE id = ?"
    )
    .bind(user_id)
    .execute(pool)
    .await {
        tracing::warn!("请求统计更新失败: user_id={}, error={}", user_id, e);
    }
}

/// 增加用户流量（不增加请求数，用于本地中转实际传输场景）
pub async fn record_traffic(pool: &SqlitePool, user_id: &str, bytes: u64) {
    if bytes == 0 {
        return;
    }
    if let Err(e) = sqlx::query(
        "UPDATE users SET total_traffic = total_traffic + ? WHERE id = ?"
    )
    .bind(bytes as i64)
    .bind(user_id)
    .execute(pool)
    .await {
        tracing::warn!("流量统计更新失败: user_id={}, bytes={}, error={}", user_id, bytes, e);
    }
}

/// 增加用户请求数和流量（302重定向时使用，统计整个文件大小）
/// - user_id: 用户ID
/// - file_size: 文件大小（字节），用于统计流量
pub async fn record_download(pool: &SqlitePool, user_id: &str, file_size: Option<u64>) {
    let traffic = file_size.unwrap_or(0) as i64;
    
    if let Err(e) = sqlx::query(
        "UPDATE users SET total_requests = total_requests + 1, total_traffic = total_traffic + ? WHERE id = ?"
    )
    .bind(traffic)
    .bind(user_id)
    .execute(pool)
    .await {
        tracing::warn!("统计更新失败: user_id={}, error={}", user_id, e);
    }
}

/// 根据直链sign获取创建者ID并统计
pub async fn record_direct_link_download(pool: &SqlitePool, sign: &str, file_size: Option<u64>) {
    // 获取直链创建者ID
    let user_id: Option<Option<String>> = sqlx::query_scalar(
        "SELECT user_id FROM direct_links WHERE sign = ?"
    )
    .bind(sign)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    
    if let Some(Some(uid)) = user_id {
        record_download(pool, &uid, file_size).await;
    }
}

/// 根据分享short_id获取创建者ID并统计
pub async fn record_share_download(pool: &SqlitePool, short_id: &str, file_size: Option<u64>) {
    // 获取分享创建者ID
    let user_id: Option<Option<String>> = sqlx::query_scalar(
        "SELECT user_id FROM shares WHERE short_id = ?"
    )
    .bind(short_id)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    
    if let Some(Some(uid)) = user_id {
        record_download(pool, &uid, file_size).await;
    }
}
