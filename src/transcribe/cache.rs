use anyhow::{Context, Result};
use sha1::{Digest, Sha1};
use std::path::{Path, PathBuf};

pub(super) struct Entry {
    root: PathBuf,
    key: String,
}

impl Entry {
    pub(super) async fn new(data_dir: &Path, url: &str) -> Result<Self> {
        let root = data_dir.join(".media_cache");
        tokio::fs::create_dir_all(&root)
            .await
            .with_context(|| format!("创建媒体缓存目录失败: {}", root.display()))?;
        let key = hex::encode(Sha1::digest(url.as_bytes()));
        Ok(Self { root, key })
    }

    pub(super) fn path(&self, extension: &str) -> PathBuf {
        self.root.join(format!("{}.{extension}", self.key))
    }

    pub(super) async fn read(&self) -> Option<String> {
        let text = tokio::fs::read_to_string(self.path("txt")).await.ok()?;
        let text = text.trim();
        (!text.is_empty()).then(|| text.to_owned())
    }

    pub(super) fn temporary(&self, extension: &str) -> TemporaryFile {
        TemporaryFile(self.path(&format!("{}.{extension}.tmp", rand::random::<u64>())))
    }

    pub(super) async fn write(&self, text: &str) {
        if text.trim().is_empty() {
            return;
        }
        let partial = self.temporary("txt");
        if tokio::fs::write(partial.path(), text).await.is_ok() {
            let _ = tokio::fs::rename(partial.path(), self.path("txt")).await;
        }
    }
}

pub(super) struct TemporaryFile(PathBuf);

impl TemporaryFile {
    pub(super) fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TemporaryFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}
