use crate::config::AppConfig;
use crate::routes::{
    admin_delete, admin_get, admin_login, admin_logout, async_end, async_init, async_push,
    download_get, download_post, index_get, locale_post, script_upload, upload_complete_get,
    upload_multipart,
};
use crate::state::AppState;
use crate::static_assets::static_get;
use crate::storage::{DynStorage, LocalFsStorage};
use crate::templates::TemplateEngine;
use axum::extract::DefaultBodyLimit;
use axum::routing::{get, post};
use axum::Router;
use hex::FromHex;
use std::sync::Arc;
use tower_cookies::CookieManagerLayer;
use tower_http::compression::CompressionLayer;
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::trace::TraceLayer;

pub fn create_app(cfg: Arc<AppConfig>) -> anyhow::Result<Router> {
    let data_dir = cfg.server.data_dir.clone();
    std::fs::create_dir_all(&data_dir)?;
    std::fs::create_dir_all(data_dir.join("files"))?;
    std::fs::create_dir_all(data_dir.join("async"))?;
    std::fs::create_dir_all(data_dir.join("tmp"))?;

    let storage: DynStorage = Arc::new(LocalFsStorage::new(data_dir.clone()));
    let jinja = Arc::new(TemplateEngine::embedded()?);
    let i18n = crate::i18n::Catalog::embedded()?;

    let signing_key: Vec<u8> = <[u8; 32]>::from_hex(cfg.admin.session_signing_key_hex.as_str())
        .map(|a| a.to_vec())
        .map_err(|_| anyhow::anyhow!("invalid session_signing_key_hex"))?;

    let state = AppState::new(cfg.clone(), storage, jinja, i18n, signing_key);

    let max_bytes = if cfg.server.max_body_mb == 0 {
        usize::MAX
    } else {
        (cfg.server.max_body_mb as usize).saturating_mul(1024 * 1024)
    };

    // `Multipart` uses `DefaultBodyLimit` (Axum default 2 MiB). `RequestBodyLimitLayer` alone does not
    // raise that cap, so large uploads were cut off mid-body unless this matches `max_body_mb`.
    let default_body_limit = if cfg.server.max_body_mb == 0 {
        DefaultBodyLimit::disable()
    } else {
        DefaultBodyLimit::max(max_bytes)
    };

    let router = Router::new()
        .route("/", get(index_get))
        .route("/locale", post(locale_post))
        .route("/upload", post(upload_multipart))
        .route("/upload/complete/{link_id}", get(upload_complete_get))
        .route("/script", post(script_upload))
        .route(
            "/api/upload/async/init",
            post(async_init).layer(DefaultBodyLimit::max(1 << 20)),
        )
        .route("/api/upload/async/push", post(async_push))
        .route(
            "/api/upload/async/end",
            post(async_end).layer(DefaultBodyLimit::max(1 << 20)),
        )
        .route("/f/{link_id}", get(download_get).post(download_post))
        .route("/admin", get(admin_get))
        .route("/admin/login", post(admin_login))
        .route("/admin/logout", post(admin_logout))
        .route("/admin/delete", post(admin_delete))
        .route("/static/{*path}", get(static_get))
        .layer(CookieManagerLayer::new())
        .layer(CompressionLayer::new())
        .layer(TraceLayer::new_for_http())
        .layer(default_body_limit)
        .layer(RequestBodyLimitLayer::new(max_bytes))
        .with_state(state);

    Ok(router)
}
