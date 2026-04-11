use super::StorageBackend;
use crate::error::AppError;
use crate::models::FileMeta;
use async_trait::async_trait;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

pub struct LocalFsStorage {
    root: PathBuf,
}

impl LocalFsStorage {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    fn files_dir(&self) -> PathBuf {
        self.root.join("files")
    }

    pub fn shard_dir(id: &str) -> String {
        if id.len() >= 2 {
            id[0..2].to_string()
        } else {
            "xx".into()
        }
    }

    fn blob_path(&self, link_id: &str) -> PathBuf {
        let s = Self::shard_dir(link_id);
        self.files_dir().join(&s).join(link_id)
    }

    fn meta_path(&self, link_id: &str) -> PathBuf {
        self.blob_path(link_id).with_extension("meta.toml")
    }

    pub fn async_dir(&self) -> PathBuf {
        self.root.join("async")
    }
}

#[async_trait]
impl StorageBackend for LocalFsStorage {
    async fn finalize_upload(
        &self,
        link_id: &str,
        temp_path: &Path,
        meta: &FileMeta,
    ) -> Result<(), AppError> {
        let dest = self.blob_path(link_id);
        if let Some(parent) = dest.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|_| AppError::Internal)?;
        }
        match tokio::fs::rename(temp_path, &dest).await {
            Ok(()) => {}
            Err(_) => {
                tokio::fs::copy(temp_path, &dest)
                    .await
                    .map_err(|_| AppError::Internal)?;
                let _ = tokio::fs::remove_file(temp_path).await;
            }
        }
        let meta_toml = toml::to_string_pretty(meta).map_err(|_| AppError::Internal)?;
        tokio::fs::write(self.meta_path(link_id), meta_toml)
            .await
            .map_err(|_| AppError::Internal)?;
        Ok(())
    }

    async fn read_meta(&self, link_id: &str) -> Result<Option<FileMeta>, AppError> {
        let p = self.meta_path(link_id);
        if !tokio::fs::try_exists(&p).await.map_err(|_| AppError::Internal)? {
            return Ok(None);
        }
        let raw = tokio::fs::read_to_string(&p)
            .await
            .map_err(|_| AppError::Internal)?;
        let m: FileMeta = toml::from_str(&raw).map_err(|_| AppError::Internal)?;
        Ok(Some(m))
    }

    async fn open_blob_path(&self, link_id: &str) -> Result<PathBuf, AppError> {
        let p = self.blob_path(link_id);
        if tokio::fs::try_exists(&p).await.map_err(|_| AppError::Internal)? {
            Ok(p)
        } else {
            Err(AppError::NotFound)
        }
    }

    async fn delete_link(&self, link_id: &str) -> Result<(), AppError> {
        let _ = tokio::fs::remove_file(self.blob_path(link_id)).await;
        let _ = tokio::fs::remove_file(self.meta_path(link_id)).await;
        Ok(())
    }

    fn data_root(&self) -> PathBuf {
        self.root.clone()
    }

    async fn list_link_ids(&self) -> Result<Vec<String>, AppError> {
        let base = self.files_dir();
        if !base.exists() {
            return Ok(vec![]);
        }
        let mut out = Vec::new();
        for e in WalkDir::new(&base).max_depth(3) {
            let e = e.map_err(|_| AppError::Internal)?;
            if !e.file_type().is_file() {
                continue;
            }
            let name = e.file_name().to_string_lossy();
            if let Some(id) = name.strip_suffix(".meta.toml") {
                out.push(id.to_string());
            }
        }
        out.sort();
        out.dedup();
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shard_two_chars() {
        assert_eq!(LocalFsStorage::shard_dir("abcdef"), "ab");
    }
}
