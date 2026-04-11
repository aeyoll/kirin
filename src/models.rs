use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileMeta {
    pub link_id: String,
    pub original_name: String,
    pub mime_type: String,
    pub size: u64,
    #[serde(default)]
    pub expires_at_unix: Option<i64>,
    #[serde(default)]
    pub one_time: bool,
    pub delete_code: String,
    #[serde(default)]
    pub download_password_hash: Option<String>,
    pub uploaded_at_unix: i64,
    #[serde(default)]
    pub uploader_ip: Option<String>,
    pub content_blake3: String,
}

impl FileMeta {
    pub fn is_expired(&self, now: i64) -> bool {
        match self.expires_at_unix {
            Some(t) => now > t,
            None => false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct AsyncUploadSession {
    pub created_at_unix: i64,
    pub ref_token: String,
    pub rolling_code: String,
    pub filename: String,
    pub mime_type: String,
    pub one_time: bool,
    pub expires_at_unix: Option<i64>,
    pub uploader_ip: Option<String>,
    pub download_password_plain: Option<String>,
    pub temp_blob_path: std::path::PathBuf,
    pub accumulated: u64,
}
