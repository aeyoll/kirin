use serde::Deserialize;
use std::net::SocketAddr;
use std::path::PathBuf;

#[derive(Debug, Clone, Deserialize)]
pub struct AppConfig {
    pub server: ServerSection,
    pub limits: LimitsSection,
    #[serde(default)]
    pub upload_auth: UploadAuthSection,
    #[serde(default)]
    pub availabilities: AvailabilitiesSection,
    #[serde(default)]
    pub features: FeaturesSection,
    #[serde(default)]
    pub admin: AdminSection,
    #[serde(default)]
    pub ui: UiSection,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServerSection {
    pub bind: String,
    pub public_base_url: String,
    pub data_dir: PathBuf,
    #[serde(default = "default_max_body_mb")]
    pub max_body_mb: u64,
    #[serde(default = "default_chunk")]
    pub max_upload_chunk_bytes: u64,
}

fn default_max_body_mb() -> u64 {
    512
}

fn default_chunk() -> u64 {
    5_000_000
}

#[derive(Debug, Clone, Deserialize)]
pub struct LimitsSection {
    #[serde(default)]
    pub max_upload_bytes: u64,
    #[serde(default = "default_link_len")]
    pub link_id_length: u8,
    #[serde(default = "default_max_async_upload_sessions")]
    pub max_async_upload_sessions: usize,
    #[serde(default = "default_async_upload_session_ttl_secs")]
    pub async_upload_session_ttl_secs: u64,
}

fn default_link_len() -> u8 {
    8
}

fn default_max_async_upload_sessions() -> usize {
    512
}

fn default_async_upload_session_ttl_secs() -> u64 {
    3600
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct UploadAuthSection {
    #[serde(default)]
    pub passwords: Vec<String>,
    #[serde(default)]
    pub allowed_ips: Vec<String>,
    #[serde(default)]
    pub allowed_ips_without_password: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AvailabilitiesSection {
    #[serde(default = "truthy")]
    pub minute: bool,
    #[serde(default = "truthy")]
    pub hour: bool,
    #[serde(default = "truthy")]
    pub day: bool,
    #[serde(default = "truthy")]
    pub week: bool,
    #[serde(default = "truthy")]
    pub fortnight: bool,
    #[serde(default = "truthy")]
    pub month: bool,
    #[serde(default)]
    pub quarter: bool,
    #[serde(default)]
    pub year: bool,
    #[serde(default)]
    pub none: bool,
    #[serde(default = "default_avail")]
    pub default: String,
}

fn truthy() -> bool {
    true
}

fn default_avail() -> String {
    "month".into()
}

impl Default for AvailabilitiesSection {
    fn default() -> Self {
        Self {
            minute: true,
            hour: true,
            day: true,
            week: true,
            fortnight: true,
            month: true,
            quarter: false,
            year: false,
            none: false,
            default: "month".into(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct FeaturesSection {
    #[serde(default = "truthy")]
    pub one_time_download: bool,
    #[serde(default)]
    pub one_time_download_preselected: bool,
    #[serde(default = "truthy")]
    pub preview: bool,
    #[serde(default = "default_dpw")]
    pub download_password_requirement: String,
}

fn default_dpw() -> String {
    "optional".into()
}

impl Default for FeaturesSection {
    fn default() -> Self {
        Self {
            one_time_download: true,
            one_time_download_preselected: false,
            preview: true,
            download_password_requirement: "optional".into(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct AdminSection {
    #[serde(default)]
    pub password_sha256_hex: String,
    #[serde(default)]
    pub session_signing_key_hex: String,
    #[serde(default)]
    pub allowed_admin_ips: Vec<String>,
}

impl Default for AdminSection {
    fn default() -> Self {
        Self {
            password_sha256_hex: String::new(),
            session_signing_key_hex: String::new(),
            allowed_admin_ips: vec![],
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct UiSection {
    #[serde(default = "default_title")]
    pub title: String,
    #[serde(default)]
    pub organisation: String,
    #[serde(default = "default_ui_locale")]
    pub default_locale: String,
}

fn default_title() -> String {
    "Kirin".into()
}

fn default_ui_locale() -> String {
    "en".into()
}

impl Default for UiSection {
    fn default() -> Self {
        Self {
            title: default_title(),
            organisation: String::new(),
            default_locale: default_ui_locale(),
        }
    }
}

impl AppConfig {
    pub fn load_path(path: impl AsRef<std::path::Path>) -> anyhow::Result<Self> {
        let raw = std::fs::read_to_string(path.as_ref())?;
        let c: AppConfig = toml::from_str(&raw)?;
        c.validate()?;
        Ok(c)
    }

    fn validate(&self) -> anyhow::Result<()> {
        let admin_pw = &self.admin.password_sha256_hex;
        if !admin_pw.is_empty() {
            if admin_pw.len() > 512 {
                anyhow::bail!("admin.password_sha256_hex too long");
            }
            if admin_pw.starts_with("$argon2") {
                argon2::password_hash::PasswordHash::new(admin_pw).map_err(|_| {
                    anyhow::anyhow!("admin.password_sha256_hex: invalid Argon2 PHC")
                })?;
            } else if admin_pw.len() == 64 && admin_pw.chars().all(|c| c.is_ascii_hexdigit()) {
                let bytes = hex::decode(admin_pw.to_ascii_lowercase())
                    .map_err(|_| anyhow::anyhow!("admin.password_sha256_hex: invalid hex"))?;
                if bytes.len() != 32 {
                    anyhow::bail!("admin.password_sha256_hex: legacy form must be 64 hex chars");
                }
            } else {
                anyhow::bail!(
                    "admin.password_sha256_hex must be 64 hex chars (legacy SHA-256) or Argon2 PHC ($argon2...)"
                );
            }
        }
        let key_bytes = hex::decode(self.admin.session_signing_key_hex.to_ascii_lowercase())
            .map_err(|_| {
                anyhow::anyhow!("admin.session_signing_key_hex must be 64 valid hex characters")
            })?;
        if key_bytes.len() != 32 {
            anyhow::bail!(
                "admin.session_signing_key_hex must decode to 32 bytes (64 hex chars); use `openssl rand -hex 32`"
            );
        }
        if self.ui.default_locale != "en"
            && self.ui.default_locale != "fr"
            && self.ui.default_locale != "ja"
        {
            anyhow::bail!("ui.default_locale must be \"en\", \"fr\", or \"ja\"");
        }
        if self.limits.max_async_upload_sessions == 0 {
            anyhow::bail!("limits.max_async_upload_sessions must be at least 1");
        }
        Ok(())
    }

    pub fn socket_addr(&self) -> anyhow::Result<SocketAddr> {
        Ok(self.server.bind.parse()?)
    }

    pub fn public_base_url_normalized(&self) -> String {
        let mut u = self.server.public_base_url.trim().to_string();
        if !u.ends_with('/') {
            u.push('/');
        }
        u
    }

    pub fn session_cookie_secure(&self) -> bool {
        self.server
            .public_base_url
            .trim_start_matches(|c: char| c.is_ascii_whitespace())
            .to_ascii_lowercase()
            .starts_with("https://")
    }

    pub fn availability_enabled(&self, key: &str) -> bool {
        match key {
            "minute" => self.availabilities.minute,
            "hour" => self.availabilities.hour,
            "day" => self.availabilities.day,
            "week" => self.availabilities.week,
            "fortnight" => self.availabilities.fortnight,
            "month" => self.availabilities.month,
            "quarter" => self.availabilities.quarter,
            "year" => self.availabilities.year,
            "none" => self.availabilities.none,
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_invalid_default_locale() {
        let raw = r#"
[server]
bind = "127.0.0.1:8080"
public_base_url = "http://127.0.0.1/"
data_dir = "/tmp"

[limits]

[upload_auth]

[availabilities]

[features]

[admin]
session_signing_key_hex = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"

[ui]
title = "x"
organisation = ""
default_locale = "de"
"#;
        let cfg = toml::from_str::<AppConfig>(raw).unwrap();
        assert!(cfg.validate().is_err());
    }
}
