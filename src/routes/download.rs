use crate::error::AppError;
use crate::i18n::Locale;
use crate::models::FileMeta;
use crate::multipart_util::{field_text_limited, MAX_MULTIPART_FIELDS};
use crate::password::verify_download_password;
use crate::routes::common::{content_disposition, human_size, valid_link_id};
use crate::routes::locale::{request_locale, tr_value};
use crate::state::AppState;
use crate::storage::DynStorage;
use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::header;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum_extra::extract::cookie::CookieJar;
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

#[derive(Clone, Copy)]
pub(crate) enum FileUnavailableKind {
    Missing,
    Expired,
}

#[derive(Clone, Copy)]
enum DownloadForbiddenReason {
    WrongPassword,
    DataRequiresPassword,
}

async fn render_download_forbidden(
    state: &AppState,
    link_id: &str,
    reason: DownloadForbiddenReason,
    loc: Locale,
) -> Result<Response, AppError> {
    let (hk, dk) = match reason {
        DownloadForbiddenReason::WrongPassword => (
            "download_errors.wrong_password_title",
            "download_errors.wrong_password_body",
        ),
        DownloadForbiddenReason::DataRequiresPassword => (
            "download_errors.data_requires_password_title",
            "download_errors.data_requires_password_body",
        ),
    };
    let headline = state.i18n.get(loc, hk);
    let description = state.i18n.get(loc, dk);
    let site_title = state.cfg.ui.title.clone();
    let organisation = state.cfg.ui.organisation.clone();
    let status_line = state.i18n.get(loc, "file_unavailable.status_forbidden");
    let page_title = format!("{status_line} - {site_title}");
    let tr = tr_value(&state.i18n, loc);
    let ctx = minijinja::context! {
        page_title => page_title,
        site_title => site_title,
        organisation => organisation,
        status_code => status_line,
        headline => headline,
        description => description,
        link_id => link_id,
        retry_action_label => state.i18n.get(loc, "common.try_again"),
        home_action_label => state.i18n.get(loc, "common.back_home"),
        locale => loc.as_str(),
        tr => tr,
    };
    let html = state
        .minijinja()
        .get_template("download_forbidden.html")
        .map_err(|_| AppError::Internal)?
        .render(ctx)
        .map_err(|_| AppError::Internal)?;
    Ok((StatusCode::FORBIDDEN, Html(html)).into_response())
}

pub(crate) async fn render_file_unavailable(
    state: &AppState,
    kind: FileUnavailableKind,
    loc: Locale,
) -> Result<Response, AppError> {
    let (status, status_u16, hk, dk) = match kind {
        FileUnavailableKind::Missing => (
            StatusCode::NOT_FOUND,
            404u16,
            "download_errors.not_found_title",
            "download_errors.not_found_body",
        ),
        FileUnavailableKind::Expired => (
            StatusCode::GONE,
            410u16,
            "download_errors.expired_title",
            "download_errors.expired_body",
        ),
    };
    let headline = state.i18n.get(loc, hk);
    let description = state.i18n.get(loc, dk);
    let site_title = state.cfg.ui.title.clone();
    let organisation = state.cfg.ui.organisation.clone();
    let page_title = format!("{headline} - {site_title}");
    let tr = tr_value(&state.i18n, loc);
    let ctx = minijinja::context! {
        page_title => page_title,
        site_title => site_title,
        organisation => organisation,
        status_code => status_u16.to_string(),
        headline => headline,
        description => description,
        home_action_label => state.i18n.get(loc, "common.back_home"),
        locale => loc.as_str(),
        tr => tr,
    };
    let html = state
        .minijinja()
        .get_template("file_unavailable.html")
        .map_err(|_| AppError::Internal)?
        .render(ctx)
        .map_err(|_| AppError::Internal)?;
    Ok((status, Html(html)).into_response())
}

async fn stream_blob_or_unavailable(
    state: &AppState,
    storage: &DynStorage,
    path: std::path::PathBuf,
    meta: &FileMeta,
    inline: bool,
    one_time: bool,
    loc: Locale,
) -> Result<Response, AppError> {
    match stream_blob(storage, path, meta, inline, one_time).await {
        Ok(r) => Ok(r),
        Err(AppError::NotFound) => {
            render_file_unavailable(state, FileUnavailableKind::Missing, loc).await
        }
        Err(e) => Err(e),
    }
}

pub async fn download_get(
    State(state): State<AppState>,
    Path(link_id): Path<String>,
    Query(q): Query<DownloadQuery>,
    headers: HeaderMap,
    jar: CookieJar,
) -> Result<Response, AppError> {
    let loc = request_locale(&state.cfg, &headers, &jar);
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

    if let Some(ref dc) = q.d {
        if dc != "1" && !dc.is_empty() {
            return delete_flow_get(&state, &link_id, &meta, dc, loc).await;
        }
    }

    let blob = match state.storage.open_blob_path(&link_id).await {
        Ok(p) => p,
        Err(AppError::NotFound) => {
            return render_file_unavailable(&state, FileUnavailableKind::Missing, loc).await;
        }
        Err(e) => return Err(e),
    };

    let inline = q.p.as_deref() == Some("1");
    let want_data = q.d.as_deref() == Some("1") || inline;

    if want_data {
        if meta.download_password_hash.is_some() {
            return render_download_forbidden(
                &state,
                &link_id,
                DownloadForbiddenReason::DataRequiresPassword,
                loc,
            )
            .await;
        }
        return stream_blob_or_unavailable(
            &state,
            &state.storage,
            blob,
            &meta,
            inline,
            meta.one_time,
            loc,
        )
        .await;
    }

    let need_password = meta.download_password_hash.is_some();
    render_download_page(&state, &meta, &link_id, need_password, loc).await
}

pub async fn download_post(
    State(state): State<AppState>,
    Path(link_id): Path<String>,
    Query(q): Query<DownloadQuery>,
    headers: HeaderMap,
    jar: CookieJar,
    mut multipart: axum::extract::Multipart,
) -> Result<Response, AppError> {
    let loc = request_locale(&state.cfg, &headers, &jar);
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

    if let Some(ref dc) = q.d {
        if dc != "1" && !dc.is_empty() {
            if meta.delete_code != *dc {
                return Err(AppError::Forbidden);
            }
            let mut confirm = false;
            let mut field_count = 0usize;
            while let Some(mut field) = multipart
                .next_field()
                .await
                .map_err(|_| AppError::BadRequest("multipart".into()))?
            {
                field_count += 1;
                if field_count > MAX_MULTIPART_FIELDS {
                    return Err(AppError::BadRequest("too many multipart parts".into()));
                }
                if field.name() == Some("confirm") {
                    let t = field_text_limited(&mut field).await?;
                    confirm = t == "1";
                }
            }
            if confirm {
                let _ = state.storage.delete_link(&link_id).await;
                let tr = tr_value(&state.i18n, loc);
                let html = state
                    .minijinja()
                    .get_template("delete_done.html")
                    .map_err(|_| AppError::Internal)?
                    .render(minijinja::context! {
                        tr => tr,
                        locale => loc.as_str(),
                    })
                    .map_err(|_| AppError::Internal)?;
                return Ok(Html(html).into_response());
            }
            return delete_flow_get(&state, &link_id, &meta, dc, loc).await;
        }
    }

    let blob = match state.storage.open_blob_path(&link_id).await {
        Ok(p) => p,
        Err(AppError::NotFound) => {
            return render_file_unavailable(&state, FileUnavailableKind::Missing, loc).await;
        }
        Err(e) => return Err(e),
    };

    let mut key = String::new();
    let mut field_count = 0usize;
    while let Some(mut field) = multipart
        .next_field()
        .await
        .map_err(|_| AppError::BadRequest("multipart".into()))?
    {
        field_count += 1;
        if field_count > MAX_MULTIPART_FIELDS {
            return Err(AppError::BadRequest("too many multipart parts".into()));
        }
        if field.name() == Some("key") {
            key = field_text_limited(&mut field).await?;
        }
    }

    let inline = q.p.as_deref() == Some("1");
    if q.d.as_deref() == Some("1") || inline {
        if let Some(ref h) = meta.download_password_hash {
            if !verify_download_password(h, &key) {
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                return render_download_forbidden(
                    &state,
                    &link_id,
                    DownloadForbiddenReason::WrongPassword,
                    loc,
                )
                .await;
            }
        }
        return stream_blob_or_unavailable(
            &state,
            &state.storage,
            blob,
            &meta,
            inline,
            meta.one_time,
            loc,
        )
        .await;
    }

    let need_password = meta.download_password_hash.is_some();
    render_download_page(&state, &meta, &link_id, need_password, loc).await
}

async fn delete_flow_get(
    state: &AppState,
    link_id: &str,
    meta: &FileMeta,
    delete_code: &str,
    loc: Locale,
) -> Result<Response, AppError> {
    if meta.delete_code != delete_code {
        return Err(AppError::Forbidden);
    }
    let tr = tr_value(&state.i18n, loc);
    let ctx = minijinja::context! {
        link_id => link_id,
        delete_code => delete_code,
        original_name => meta.original_name,
        size_human => human_size(meta.size),
        locale => loc.as_str(),
        tr => tr,
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
    loc: Locale,
) -> Result<Response, AppError> {
    let preview = state.cfg.features.preview && is_previewable(&meta.mime_type);
    let tr = tr_value(&state.i18n, loc);
    let ctx = minijinja::context! {
        title => meta.original_name.clone(),
        need_password => need_password,
        link_id => link_id,
        original_name => meta.original_name.clone(),
        size_human => human_size(meta.size),
        one_time => meta.one_time,
        preview => preview,
        locale => loc.as_str(),
        tr => tr,
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
