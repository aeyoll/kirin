use crate::error::AppError;
use crate::expiry::expires_at_unix;
use crate::models::{AsyncUploadSession, FileMeta};
use crate::password::hash_download_password;
use crate::routes::common::{challenge_upload, gen_delete_code, gen_link_id, gen_rolling_code};
use crate::state::AppState;
use crate::storage::LocalFsStorage;
use axum::extract::{ConnectInfo, Multipart, State};
use serde::Deserialize;
use std::collections::HashMap;
use std::net::SocketAddr;
use tokio::fs::{File, OpenOptions};
use tokio::io::AsyncWriteExt;

#[derive(Deserialize)]
pub struct InitBody {
    pub filename: String,
    #[serde(default)]
    pub r#type: String,
    #[serde(default)]
    pub one_time_download: Option<String>,
    pub time: String,
    #[serde(default)]
    pub key: String,
    #[serde(default)]
    pub upload_password: String,
}

pub async fn async_init(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    axum::Form(init): axum::Form<InitBody>,
) -> Result<String, AppError> {
    let ip = addr.ip().to_string();
    let cfg = &state.cfg;
    let upw = if init.upload_password.is_empty() {
        None
    } else {
        Some(init.upload_password.as_str())
    };
    if !challenge_upload(
        &cfg.upload_auth.passwords,
        &cfg.upload_auth.allowed_ips,
        &cfg.upload_auth.allowed_ips_without_password,
        &ip,
        upw,
    ) {
        return Err(AppError::Forbidden);
    }
    if !cfg.availability_enabled(&init.time) {
        return Err(AppError::BadRequest("invalid time".into()));
    }
    let one_time = cfg.features.one_time_download && init.one_time_download.as_deref() == Some("1");
    if !cfg.features.one_time_download && init.one_time_download.is_some() {
        return Err(AppError::BadRequest("one time disabled".into()));
    }
    let now = chrono::Utc::now().timestamp();
    let expires = expires_at_unix(now, &init.time);
    let ref_token = gen_link_id(32);
    let code = gen_rolling_code(4);
    let async_root = state.storage.data_root().join("async");
    let shard = LocalFsStorage::shard_dir(&ref_token);
    let dir = async_root.join(&shard);
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|_| AppError::Internal)?;
    let temp_blob_path = dir.join(format!("{ref_token}.data"));
    File::create(&temp_blob_path)
        .await
        .map_err(|_| AppError::Internal)?;
    let sess = AsyncUploadSession {
        ref_token: ref_token.clone(),
        rolling_code: code.clone(),
        filename: init.filename.clone(),
        mime_type: if init.r#type.is_empty() {
            "application/octet-stream".into()
        } else {
            init.r#type.clone()
        },
        one_time,
        expires_at_unix: expires,
        uploader_ip: Some(ip),
        download_password_plain: if init.key.is_empty() {
            None
        } else {
            Some(init.key.clone())
        },
        temp_blob_path,
        accumulated: 0,
    };
    state
        .async_sessions
        .lock()
        .await
        .insert(ref_token.clone(), sess);
    Ok(format!("{ref_token}\n{code}\n"))
}

pub async fn async_push(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Result<String, AppError> {
    let cfg = &state.cfg;
    let mut fields: HashMap<String, String> = HashMap::new();
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|_| AppError::BadRequest("multipart".into()))?
    {
        let n = field.name().unwrap_or("").to_string();
        if n == "data" {
            let ref_token = fields
                .get("ref")
                .cloned()
                .ok_or_else(|| AppError::BadRequest("missing ref".into()))?;
            let code = fields
                .get("code")
                .cloned()
                .ok_or_else(|| AppError::BadRequest("missing code".into()))?;
            let mut guard = state.async_sessions.lock().await;
            let sess = guard
                .get_mut(&ref_token)
                .ok_or(AppError::BadRequest("bad ref".into()))?;
            if sess.rolling_code != code {
                return Err(AppError::Forbidden);
            }
            let mut f = OpenOptions::new()
                .append(true)
                .open(&sess.temp_blob_path)
                .await
                .map_err(|_| AppError::Internal)?;
            let mut field = field;
            while let Some(chunk) = field
                .chunk()
                .await
                .map_err(|_| AppError::BadRequest("chunk".into()))?
            {
                let new_total = sess.accumulated + chunk.len() as u64;
                if cfg.limits.max_upload_bytes > 0 && new_total > cfg.limits.max_upload_bytes {
                    let path = sess.temp_blob_path.clone();
                    let tok = sess.ref_token.clone();
                    drop(guard);
                    let _ = tokio::fs::remove_file(&path).await;
                    state.async_sessions.lock().await.remove(&tok);
                    return Err(AppError::PayloadTooLarge);
                }
                sess.accumulated = new_total;
                f.write_all(&chunk).await.map_err(|_| AppError::Internal)?;
            }
            let new_code = gen_rolling_code(4);
            sess.rolling_code = new_code.clone();
            return Ok(new_code);
        }
        if let Ok(t) = field.text().await {
            fields.insert(n, t);
        }
    }
    Err(AppError::BadRequest("missing data".into()))
}

#[derive(Deserialize)]
pub struct EndForm {
    #[serde(rename = "ref")]
    pub ref_token: String,
    pub code: String,
}

pub async fn async_end(
    State(state): State<AppState>,
    axum::Form(form): axum::Form<EndForm>,
) -> Result<String, AppError> {
    let cfg = &state.cfg;
    let mut guard = state.async_sessions.lock().await;
    let sess = guard
        .get(&form.ref_token)
        .ok_or(AppError::BadRequest("bad ref".into()))?;
    if sess.rolling_code != form.code {
        return Err(AppError::Forbidden);
    }
    let sess = guard.remove(&form.ref_token).unwrap();
    drop(guard);

    let tmp_path = sess.temp_blob_path.clone();
    let data = tokio::fs::read(&tmp_path)
        .await
        .map_err(|_| AppError::Internal)?;
    let hash = blake3::hash(&data);
    let now = chrono::Utc::now().timestamp();
    let link_len = cfg.limits.link_id_length.max(4).min(32) as usize;
    let mut link_id = gen_link_id(link_len);
    for _ in 0..32 {
        if state
            .storage
            .read_meta(&link_id)
            .await
            .map_err(|_| AppError::Internal)?
            .is_none()
        {
            break;
        }
        link_id = gen_link_id(link_len);
    }
    let delete_code = gen_delete_code(5);
    let mut mime = sess.mime_type.clone();
    if mime == "application/octet-stream" {
        if let Some(g) = mime_guess::from_path(&sess.filename).first() {
            mime = g.essence_str().to_string();
        }
    }
    let pw_hash = if let Some(ref p) = sess.download_password_plain {
        Some(hash_download_password(p).map_err(|_| AppError::Internal)?)
    } else {
        None
    };
    let meta = FileMeta {
        link_id: link_id.clone(),
        original_name: sess.filename.clone(),
        mime_type: mime,
        size: sess.accumulated,
        expires_at_unix: sess.expires_at_unix,
        one_time: sess.one_time,
        delete_code: delete_code.clone(),
        download_password_hash: pw_hash,
        uploaded_at_unix: now,
        uploader_ip: sess.uploader_ip.clone(),
        content_blake3: hash.to_hex().to_string(),
    };
    state
        .storage
        .finalize_upload(&link_id, &tmp_path, &meta)
        .await?;
    Ok(format!("{link_id}\n{delete_code}\n\n"))
}
