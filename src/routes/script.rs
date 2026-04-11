use crate::error::AppError;
use crate::routes::upload::process_multipart;
use crate::state::AppState;
use axum::extract::{ConnectInfo, Multipart, State};
use axum::response::IntoResponse;
use std::net::SocketAddr;

pub async fn script_upload(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    multipart: Multipart,
) -> Result<impl IntoResponse, AppError> {
    let ip = addr.ip().to_string();
    let r = process_multipart(&state, multipart, &ip).await?;
    let base = state.cfg.public_base_url_normalized();
    let body = format!(
        "{}{}\n{}\n",
        base,
        r.link_id,
        r.delete_code,
    );
    Ok(([(axum::http::header::CONTENT_TYPE, "text/plain; charset=utf-8")], body))
}
