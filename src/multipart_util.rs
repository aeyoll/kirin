use axum::extract::multipart::Field;
use crate::error::AppError;

pub const MAX_MULTIPART_FIELDS: usize = 64;
pub const MAX_TEXT_PART_BYTES: usize = 64 * 1024;

pub async fn field_text_limited(field: &mut Field<'_>) -> Result<String, AppError> {
    let mut acc = Vec::new();
    while let Some(chunk) = field
        .chunk()
        .await
        .map_err(|_| AppError::BadRequest("multipart read".into()))?
    {
        let next = acc.len().saturating_add(chunk.len());
        if next > MAX_TEXT_PART_BYTES {
            return Err(AppError::BadRequest("multipart text field too large".into()));
        }
        acc.extend_from_slice(&chunk);
    }
    String::from_utf8(acc).map_err(|_| AppError::BadRequest("multipart invalid utf-8".into()))
}
