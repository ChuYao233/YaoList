use rand::Rng;
use sha2::{Sha256, Digest};
use chrono::{DateTime, Utc, Duration};

pub const SESSION_COOKIE_NAME: &str = "yaolist_session";

#[derive(Debug, Clone)]
pub struct Session {
    pub id: String,
    pub user_id: String,
    pub expires_at: DateTime<Utc>,
}

pub fn generate_session_id() -> String {
    let random_bytes: [u8; 32] = rand::thread_rng().gen();
    let mut hasher = Sha256::new();
    hasher.update(&random_bytes);
    hasher.update(Utc::now().timestamp_millis().to_le_bytes());
    hex::encode(hasher.finalize())
}

pub fn create_session(user_id: &str) -> Session {
    Session {
        id: generate_session_id(),
        user_id: user_id.to_string(),
        expires_at: Utc::now() + Duration::days(7),
    }
}
