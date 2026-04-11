use axum::body::Body;
use axum::extract::Path;
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "static/"]
struct StaticAssets;

pub async fn static_get(Path(path): Path<String>) -> Response {
    if path.is_empty() || path.contains("..") {
        return StatusCode::NOT_FOUND.into_response();
    }

    let key = path.trim_start_matches('/').replace('\\', "/");
    let Some(file) = StaticAssets::get(&key) else {
        return StatusCode::NOT_FOUND.into_response();
    };

    let mime = mime_guess::from_path(&key).first_or_octet_stream();
    let Ok(ct) = HeaderValue::from_str(mime.as_ref()) else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };

    let body = Body::from(file.data.into_owned());
    (StatusCode::OK, [(header::CONTENT_TYPE, ct)], body).into_response()
}
