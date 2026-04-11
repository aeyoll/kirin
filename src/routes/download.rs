use crate::error::AppError;
use crate::models::FileMeta;
use crate::password::verify_download_password;
use crate::routes::common::{content_disposition, human_size, valid_link_id};
use crate::state::AppState;
use crate::storage::DynStorage;
use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::header;
use axum::response::{Html, IntoResponse, Response};
use bytes::Bytes;
use futures_util::Stream;
use serde::Deserialize;
use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use tokio::fs::File;
use tokio::io::{AsyncRead, ReadBuf};

#[derive(Deserialize, Default)]
pub struct DownloadQuery {
    #[serde(default)]
    pub d: Option<String>,
    #[serde(default)]
    pub p: Option<String>,
}

pub async fn download_get(
    State(state): State<AppState>,
    Path(link_id): Path<String>,
    Query(q): Query<DownloadQuery>,
) -> Result<Response, AppError> {
    if !valid_link_id(&link_id) {
        return Err(AppError::NotFound);
    }
    let meta = state
        .storage
        .read_meta(&link_id)
        .await?
        .ok_or(AppError::NotFound)?;
    let now = chrono::Utc::now().timestamp();
    if meta.is_expired(now) {
        let _ = state.storage.delete_link(&link_id).await;
        return Err(AppError::Gone);
    }

    if let Some(ref dc) = q.d {
        if dc != "1" && !dc.is_empty() {
            return delete_flow_get(&state, &link_id, &meta, dc).await;
        }
    }

    let blob = state.storage.open_blob_path(&link_id).await?;

    let inline = q.p.as_deref() == Some("1");
    let want_data = q.d.as_deref() == Some("1") || inline;

    if want_data {
        if meta.download_password_hash.is_some() {
            return Err(AppError::Forbidden);
        }
        return stream_blob(&state.storage, blob, &meta, inline, meta.one_time).await;
    }

    let need_password = meta.download_password_hash.is_some();
    render_download_page(&state, &meta, &link_id, need_password).await
}

pub async fn download_post(
    State(state): State<AppState>,
    Path(link_id): Path<String>,
    Query(q): Query<DownloadQuery>,
    mut multipart: axum::extract::Multipart,
) -> Result<Response, AppError> {
    if !valid_link_id(&link_id) {
        return Err(AppError::NotFound);
    }
    let meta = state
        .storage
        .read_meta(&link_id)
        .await?
        .ok_or(AppError::NotFound)?;
    let now = chrono::Utc::now().timestamp();
    if meta.is_expired(now) {
        let _ = state.storage.delete_link(&link_id).await;
        return Err(AppError::Gone);
    }

    if let Some(ref dc) = q.d {
        if dc != "1" && !dc.is_empty() {
            if meta.delete_code != *dc {
                return Err(AppError::Forbidden);
            }
            let mut confirm = false;
            while let Some(field) = multipart
                .next_field()
                .await
                .map_err(|_| AppError::BadRequest("multipart".into()))?
            {
                if field.name() == Some("confirm") {
                    let t = field
                        .text()
                        .await
                        .map_err(|_| AppError::BadRequest("confirm".into()))?;
                    confirm = t == "1";
                }
            }
            if confirm {
                let _ = state.storage.delete_link(&link_id).await;
                let html = "<!DOCTYPE html><html><body><p>File deleted.</p></body></html>";
                return Ok(Html(html.to_string()).into_response());
            }
            return delete_flow_get(&state, &link_id, &meta, dc).await;
        }
    }

    let blob = state.storage.open_blob_path(&link_id).await?;

    let mut key = String::new();
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|_| AppError::BadRequest("multipart".into()))?
    {
        if field.name() == Some("key") {
            key = field
                .text()
                .await
                .map_err(|_| AppError::BadRequest("key".into()))?;
        }
    }

    let inline = q.p.as_deref() == Some("1");
    if q.d.as_deref() == Some("1") || inline {
        if let Some(ref h) = meta.download_password_hash {
            if !verify_download_password(h, &key) {
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                return Err(AppError::Forbidden);
            }
        }
        return stream_blob(
            &state.storage,
            blob,
            &meta,
            inline,
            meta.one_time,
        )
        .await;
    }

    let need_password = meta.download_password_hash.is_some();
    render_download_page(&state, &meta, &link_id, need_password).await
}

async fn delete_flow_get(
    state: &AppState,
    link_id: &str,
    meta: &FileMeta,
    delete_code: &str,
) -> Result<Response, AppError> {
    if meta.delete_code != delete_code {
        return Err(AppError::Forbidden);
    }
    let ctx = minijinja::context! {
        link_id => link_id,
        delete_code => delete_code,
        original_name => meta.original_name,
        size_human => human_size(meta.size),
    };
    let html = state
        .minijinja()
        .get_template("delete_confirm.html")
        .map_err(|_| AppError::Internal)?
        .render(ctx)
        .map_err(|_| AppError::Internal)?;
    Ok(Html(html).into_response())
}

async fn render_download_page(
    state: &AppState,
    meta: &FileMeta,
    link_id: &str,
    need_password: bool,
) -> Result<Response, AppError> {
    let preview = state.cfg.features.preview && is_previewable(&meta.mime_type);
    let ctx = minijinja::context! {
        title => meta.original_name.clone(),
        need_password => need_password,
        link_id => link_id,
        original_name => meta.original_name.clone(),
        size_human => human_size(meta.size),
        one_time => meta.one_time,
        preview => preview,
    };
    let html = state
        .minijinja()
        .get_template("download.html")
        .map_err(|_| AppError::Internal)?
        .render(ctx)
        .map_err(|_| AppError::Internal)?;
    Ok(Html(html).into_response())
}

fn is_previewable(mime: &str) -> bool {
    if mime.contains("image/svg+xml") || mime.contains(',') {
        return false;
    }
    mime.starts_with("image/")
        || mime.starts_with("video/")
        || mime.starts_with("audio/")
        || mime == "text/plain"
}

struct OneTimeFileStream {
    file: File,
    buf: Vec<u8>,
    storage: DynStorage,
    link_id: String,
    finished: bool,
}

impl OneTimeFileStream {
    fn new(file: File, storage: DynStorage, link_id: String) -> Self {
        Self {
            file,
            buf: vec![0u8; 64 * 1024],
            storage,
            link_id,
            finished: false,
        }
    }
}

impl Stream for OneTimeFileStream {
    type Item = Result<Bytes, io::Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.as_mut().get_mut();
        if this.finished {
            return Poll::Ready(None);
        }
        let mut read_buf = ReadBuf::new(&mut this.buf);
        match Pin::new(&mut this.file).poll_read(cx, &mut read_buf) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Err(e)) => Poll::Ready(Some(Err(e))),
            Poll::Ready(Ok(())) => {
                let n = read_buf.filled().len();
                if n == 0 {
                    this.finished = true;
                    let id = this.link_id.clone();
                    let st = this.storage.clone();
                    tokio::spawn(async move {
                        let _ = st.delete_link(&id).await;
                    });
                    return Poll::Ready(None);
                }
                Poll::Ready(Some(Ok(Bytes::copy_from_slice(read_buf.filled()))))
            }
        }
    }
}

async fn stream_blob(
    storage: &DynStorage,
    path: std::path::PathBuf,
    meta: &FileMeta,
    inline: bool,
    one_time: bool,
) -> Result<Response, AppError> {
    let cd = content_disposition(&meta.original_name, inline);
    let file = File::open(&path).await.map_err(|_| AppError::NotFound)?;
    let body = if one_time {
        let st: DynStorage = Arc::clone(storage);
        Body::from_stream(OneTimeFileStream::new(file, st, meta.link_id.clone()))
    } else {
        let stream = tokio_util::io::ReaderStream::new(file);
        Body::from_stream(stream)
    };
    let mut resp = Response::new(body);
    resp.headers_mut().insert(
        header::CONTENT_TYPE,
        meta.mime_type.parse().map_err(|_| AppError::Internal)?,
    );
    resp.headers_mut().insert(header::CONTENT_DISPOSITION, cd);
    if let Ok(v) = axum::http::HeaderValue::from_str(&meta.size.to_string()) {
        resp.headers_mut().insert(header::CONTENT_LENGTH, v);
    }
    Ok(resp)
}
