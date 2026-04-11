mod local_fs;

pub use local_fs::LocalFsStorage;

use crate::error::AppError;
use crate::models::FileMeta;
use async_trait::async_trait;
use std::path::PathBuf;
use std::sync::Arc;

#[async_trait]
pub trait StorageBackend: Send + Sync {
    async fn finalize_upload(
        &self,
        link_id: &str,
        temp_path: &std::path::Path,
        meta: &FileMeta,
    ) -> Result<(), AppError>;

    async fn read_meta(&self, link_id: &str) -> Result<Option<FileMeta>, AppError>;

    async fn open_blob_path(&self, link_id: &str) -> Result<PathBuf, AppError>;

    async fn delete_link(&self, link_id: &str) -> Result<(), AppError>;

    fn data_root(&self) -> PathBuf;

    /// Iterate stored link ids (for admin listing).
    async fn list_link_ids(&self) -> Result<Vec<String>, AppError>;
}

pub type DynStorage = Arc<dyn StorageBackend>;
