use crate::error::AppError;
use crate::expiry::expires_at_unix;
use crate::models::FileMeta;
use crate::password::hash_download_password;
use crate::routes::common::{challenge_upload, gen_delete_code, gen_link_id};
use crate::state::AppState;
use axum::extract::{ConnectInfo, Multipart, State};
use axum::response::{Html, IntoResponse, Response};
use blake3::Hasher;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;

pub async fn upload_multipart(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    multipart: Multipart,
) -> Result<Response, AppError> {
    let ip = addr.ip().to_string();
    let res = process_multipart(&state, multipart, &ip).await?;
    let cfg = &state.cfg;
    let base = cfg.public_base_url_normalized();
    let ctx = minijinja::context! {
        original_name => res.original_name,
        download_page => format!("{}f/{}", base, res.link_id),
        direct_download => format!("{}f/{}?d=1", base, res.link_id),
        delete_link => format!("{}f/{}?d={}", base, res.link_id, res.delete_code),
    };
    let html = state
        .minijinja()
        .get_template("upload_result.html")
        .map_err(|_| AppError::Internal)?
        .render(ctx)
        .map_err(|_| AppError::Internal)?;
    Ok(Html(html).into_response())
}

pub struct UploadResult {
    pub link_id: String,
    pub delete_code: String,
    pub original_name: String,
}

pub async fn process_multipart(
    state: &AppState,
    mut multipart: Multipart,
    client_ip: &str,
) -> Result<UploadResult, AppError> {
    let cfg = &state.cfg;
    let mut map: HashMap<String, String> = HashMap::new();
    let mut tmp_path: Option<PathBuf> = None;
    let mut original_name = String::from("upload.bin");
    let mut mime_type = String::from("application/octet-stream");
    let mut size: u64 = 0;
    let mut hasher = Hasher::new();

    let tmp_dir = state.storage.data_root().join("tmp");
    tokio::fs::create_dir_all(&tmp_dir)
        .await
        .map_err(|_| AppError::Internal)?;
    let tmp_file = tmp_dir.join(format!("{}.part", uuid::Uuid::new_v4()));

    while let Some(mut field) = multipart
        .next_field()
        .await
        .map_err(|_| AppError::BadRequest("multipart".into()))?
    {
        let name = field.name().unwrap_or("").to_string();
        if name == "file" {
            original_name = field
                .file_name()
                .map(|s| s.to_string())
                .unwrap_or_else(|| "upload.bin".into());
            mime_type = field
                .content_type()
                .map(|s| s.to_string())
                .unwrap_or_else(|| "application/octet-stream".into());
            let mut f = File::create(&tmp_file)
                .await
                .map_err(|_| AppError::Internal)?;
            while let Some(chunk) = field
                .chunk()
                .await
                .map_err(|_| AppError::BadRequest("chunk read".into()))?
            {
                if cfg.limits.max_upload_bytes > 0
                    && size + chunk.len() as u64 > cfg.limits.max_upload_bytes
                {
                    let _ = tokio::fs::remove_file(&tmp_file).await;
                    return Err(AppError::PayloadTooLarge);
                }
                size += chunk.len() as u64;
                hasher.update(&chunk);
                f.write_all(&chunk)
                    .await
                    .map_err(|_| AppError::Internal)?;
            }
            f.flush().await.map_err(|_| AppError::Internal)?;
            tmp_path = Some(tmp_file.clone());
        } else if let Ok(t) = field.text().await {
            map.insert(name, t);
        }
    }

    let tmp_path = tmp_path.ok_or_else(|| AppError::BadRequest("missing file".into()))?;

    let upload_pw = map.get("upload_password").map(String::as_str);
    if !challenge_upload(
        &cfg.upload_auth.passwords,
        &cfg.upload_auth.allowed_ips,
        &cfg.upload_auth.allowed_ips_without_password,
        client_ip,
        upload_pw,
    ) {
        let _ = tokio::fs::remove_file(&tmp_path).await;
        return Err(AppError::Forbidden);
    }

    let time_key = map.get("time").map(String::as_str).unwrap_or("none");
    if !cfg.availability_enabled(time_key) {
        let _ = tokio::fs::remove_file(&tmp_path).await;
        return Err(AppError::BadRequest("invalid time".into()));
    }

    let one_time = cfg.features.one_time_download && map.get("one_time_download").map(|v| v == "1").unwrap_or(false);
    if !cfg.features.one_time_download && map.contains_key("one_time_download") {
        let _ = tokio::fs::remove_file(&tmp_path).await;
        return Err(AppError::BadRequest("one time disabled".into()));
    }

    let key_plain = map.get("key").cloned().unwrap_or_default();
    match cfg.features.download_password_requirement.as_str() {
        "required" if key_plain.is_empty() => {
            let _ = tokio::fs::remove_file(&tmp_path).await;
            return Err(AppError::BadRequest("download password required".into()));
        }
        _ => {}
    }

    if cfg.limits.max_upload_bytes > 0 && size > cfg.limits.max_upload_bytes {
        let _ = tokio::fs::remove_file(&tmp_path).await;
        return Err(AppError::PayloadTooLarge);
    }

    if mime_type == "application/octet-stream" || mime_type.is_empty() {
        if let Some(g) = mime_guess::from_path(&original_name).first() {
            mime_type = g.essence_str().to_string();
        }
    }

    let now = chrono::Utc::now().timestamp();
    let expires = expires_at_unix(now, time_key);
    let delete_code = gen_delete_code(5);
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

    let pw_hash = if key_plain.is_empty() {
        None
    } else {
        Some(
            hash_download_password(&key_plain).map_err(|_| AppError::Internal)?,
        )
    };

    let meta = FileMeta {
        link_id: link_id.clone(),
        original_name: original_name.clone(),
        mime_type,
        size,
        expires_at_unix: expires,
        one_time,
        delete_code: delete_code.clone(),
        download_password_hash: pw_hash,
        uploaded_at_unix: now,
        uploader_ip: Some(client_ip.to_string()),
        content_blake3: hasher.finalize().to_hex().to_string(),
    };

    state
        .storage
        .finalize_upload(&link_id, &tmp_path, &meta)
        .await?;

    Ok(UploadResult {
        link_id,
        delete_code,
        original_name,
    })
}
