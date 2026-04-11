use crate::config::AppConfig;
use crate::models::AsyncUploadSession;
use crate::storage::DynStorage;
use crate::templates::TemplateEngine;
use hmac::{Hmac, Mac};
use minijinja::Environment;
use sha2::Sha256;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

type HmacSha256 = Hmac<Sha256>;

#[derive(Clone)]
pub struct AppState {
    pub cfg: Arc<AppConfig>,
    pub storage: DynStorage,
    pub jinja: Arc<TemplateEngine>,
    pub async_sessions: Arc<Mutex<HashMap<String, AsyncUploadSession>>>,
    signing_key: Arc<[u8]>,
}

impl AppState {
    pub fn new(
        cfg: Arc<AppConfig>,
        storage: DynStorage,
        jinja: Arc<TemplateEngine>,
        signing_key: Vec<u8>,
    ) -> Self {
        Self {
            cfg,
            storage,
            jinja,
            async_sessions: Arc::new(Mutex::new(HashMap::new())),
            signing_key: signing_key.into(),
        }
    }

    pub fn minijinja(&self) -> &Environment<'static> {
        self.jinja.env()
    }

    /// Build `exp_unix:payload` admin session token (payload constant `1` for simplicity).
    pub fn sign_admin_session(&self, exp_unix: i64) -> String {
        let payload = format!("{exp_unix}:1");
        let mut mac = HmacSha256::new_from_slice(&self.signing_key)
            .expect("HMAC key length"); // justified: key validated at startup
        mac.update(payload.as_bytes());
        let sig = hex::encode(mac.finalize().into_bytes());
        format!("{payload}:{sig}")
    }

    pub fn verify_admin_session(&self, token: &str) -> bool {
        let parts: Vec<&str> = token.split(':').collect();
        if parts.len() != 3 {
            return false;
        }
        let exp: i64 = parts[0].parse().unwrap_or(0);
        let now = chrono::Utc::now().timestamp();
        if now > exp {
            return false;
        }
        let payload = format!("{}:{}", parts[0], parts[1]);
        let sig = parts[2];
        let Ok(sig_bytes) = hex::decode(sig) else {
            return false;
        };
        let mut mac = match HmacSha256::new_from_slice(&self.signing_key) {
            Ok(m) => m,
            Err(_) => return false,
        };
        mac.update(payload.as_bytes());
        mac.verify_slice(&sig_bytes).is_ok()
    }
}
