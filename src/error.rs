use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("not found")]
    NotFound,
    #[error("gone")]
    Gone,
    #[error("forbidden")]
    Forbidden,
    #[error("bad request: {0}")]
    BadRequest(String),
    #[error("payload too large")]
    PayloadTooLarge,
    #[error("conflict")]
    Conflict,
    #[error("internal")]
    Internal,
}

#[derive(Serialize)]
struct ApiErr<'a> {
    error: &'a str,
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, msg) = match &self {
            AppError::NotFound => (StatusCode::NOT_FOUND, self.to_string()),
            AppError::Gone => (StatusCode::GONE, self.to_string()),
            AppError::Forbidden => (StatusCode::FORBIDDEN, self.to_string()),
            AppError::BadRequest(m) => (StatusCode::BAD_REQUEST, m.clone()),
            AppError::PayloadTooLarge => (StatusCode::PAYLOAD_TOO_LARGE, self.to_string()),
            AppError::Conflict => (StatusCode::CONFLICT, self.to_string()),
            AppError::Internal => (StatusCode::INTERNAL_SERVER_ERROR, self.to_string()),
        };
        let body = Json(ApiErr { error: &msg });
        (status, body).into_response()
    }
}

impl AppError {
    pub fn plain_response(self, status: StatusCode, body: &'static str) -> Response {
        (status, body).into_response()
    }
}
