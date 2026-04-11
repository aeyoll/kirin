use crate::error::AppError;
use crate::expiry::expires_at_unix;
use crate::models::FileMeta;
use crate::multipart_util::{field_text_limited, MAX_MULTIPART_FIELDS};
use crate::password::hash_download_password;
use crate::routes::common::{challenge_upload, gen_delete_code, gen_link_id, valid_link_id};
use crate::routes::download::{render_file_unavailable, FileUnavailableKind};
use crate::routes::locale::{request_locale, tr_value};
use crate::state::AppState;
use axum::extract::{ConnectInfo, Multipart, Path, Query, State};
use axum::http::HeaderMap;
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum_extra::extract::cookie::CookieJar;
use blake3::Hasher;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;

#[derive(serde::Deserialize, Default)]
pub struct UploadCompleteQuery {
    #[serde(default)]
    pub v: Option<String>,
}

pub async fn upload_multipart(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    multipart: Multipart,
) -> Result<Response, AppError> {
    let ip = addr.ip().to_string();
    let res = process_multipart(&state, multipart, &ip).await?;
    let v = state.sign_upload_complete_view(&res.link_id, &res.delete_code);
    let target = format!("/upload/complete/{}?v={}", res.link_id, v);
    Ok(Redirect::to(&target).into_response())
}

pub async fn upload_complete_get(
    State(state): State<AppState>,
    Path(link_id): Path<String>,
    Query(q): Query<UploadCompleteQuery>,
    headers: HeaderMap,
    jar: CookieJar,
) -> Result<Response, AppError> {
    let cfg = &state.cfg;
    let loc = request_locale(cfg, &headers, &jar);
    if !valid_link_id(&link_id) {
        return render_file_unavailable(&state, FileUnavailableKind::Missing, loc).await;
    }
    let meta = match state.storage.read_meta(&link_id).await? {
        Some(m) => m,
        None => return render_file_unavailable(&state, FileUnavailableKind::Missing, loc).await,
    };
    let now = chrono::Utc::now().timestamp();
    if meta.is_expired(now) {
        let _ = state.storage.delete_link(&link_id).await;
        return render_file_unavailable(&state, FileUnavailableKind::Expired, loc).await;
    }
    let tr = tr_value(&state.i18n, loc);
    let base = cfg.public_base_url_normalized();
    let show_delete =
        q.v.as_deref()
            .is_some_and(|t| state.verify_upload_complete_view(&link_id, &meta.delete_code, t));
    let delete_link = if show_delete {
        format!("{}f/{}?d={}", base, meta.link_id, meta.delete_code)
    } else {
        String::new()
    };
    let ctx = minijinja::context! {
        original_name => meta.original_name,
        download_page => format!("{}f/{}", base, meta.link_id),
        direct_download => format!("{}f/{}?d=1", base, meta.link_id),
        show_delete_link => show_delete,
        delete_link => delete_link,
        locale => loc.as_str(),
        tr => tr,
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

    let mut field_count = 0usize;
    while let Some(mut field) = multipart
        .next_field()
        .await
        .map_err(|_| AppError::BadRequest("multipart".into()))?
    {
        field_count += 1;
        if field_count > MAX_MULTIPART_FIELDS {
            let _ = tokio::fs::remove_file(&tmp_file).await;
            return Err(AppError::BadRequest("too many multipart parts".into()));
        }
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
                f.write_all(&chunk).await.map_err(|_| AppError::Internal)?;
            }
            f.flush().await.map_err(|_| AppError::Internal)?;
            tmp_path = Some(tmp_file.clone());
        } else {
            let t = field_text_limited(&mut field).await?;
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

    let one_time = cfg.features.one_time_download
        && map
            .get("one_time_download")
            .map(|v| v == "1")
            .unwrap_or(false);
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
    if state
        .storage
        .read_meta(&link_id)
        .await
        .map_err(|_| AppError::Internal)?
        .is_some()
    {
        let _ = tokio::fs::remove_file(&tmp_path).await;
        return Err(AppError::Conflict);
    }

    let pw_hash = if key_plain.is_empty() {
        None
    } else {
        Some(hash_download_password(&key_plain).map_err(|_| AppError::Internal)?)
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
    })
}
