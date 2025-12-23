use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Share {
    pub id: i64,
    pub user_id: Option<String>,
    pub short_id: String,
    pub path: String,
    pub name: String,
    pub is_dir: bool,
    pub password: Option<String>,
    pub expires_at: Option<String>,
    pub max_access_count: Option<i64>,
    pub access_count: i64,
    pub enabled: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct ShareWithCreator {
    pub id: i64,
    pub user_id: Option<String>,
    pub short_id: String,
    pub path: String,
    pub name: String,
    pub is_dir: bool,
    pub password: Option<String>,
    pub expires_at: Option<String>,
    pub max_access_count: Option<i64>,
    pub access_count: i64,
    pub enabled: bool,
    pub created_at: String,
    pub updated_at: String,
    pub creator_name: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreateShareRequest {
    pub path: String,
    pub password: Option<String>,
    pub expires_at: Option<String>,
    pub max_access_count: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateShareRequest {
    pub password: Option<String>,
    pub expires_at: Option<String>,
    pub max_access_count: Option<i64>,
    pub enabled: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct ListSharesQuery {
    pub page: Option<i64>,
    pub per_page: Option<i64>,
    pub search: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ShareFileRequest {
    pub password: Option<String>,
    pub sub_path: Option<String>,
    pub page: Option<i64>,
    pub per_page: Option<i64>,
}

pub fn generate_short_id(length: usize) -> String {
    use rand::Rng;
    const CHARSET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZabcdefghjkmnpqrstuvwxyz23456789";
    let mut rng = rand::thread_rng();
    (0..length)
        .map(|_| {
            let idx = rng.gen_range(0..CHARSET.len());
            CHARSET[idx] as char
        })
        .collect()
}
