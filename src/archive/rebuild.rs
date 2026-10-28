use super::store;
use anyhow::{Context, Result};
use rusqlite::Connection;
use std::path::{Path, PathBuf};

/// 管理临时课程归档，只有完整抓取成功后才发布其内容。
pub(super) struct Rebuild {
    target: PathBuf,
    staging: PathBuf,
}

impl Rebuild {
    pub fn new(target: &Path) -> Self {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        Self {
            target: target.to_path_buf(),
            staging: target.with_extension(format!("rebuild-{}-{nonce}.db", std::process::id())),
        }
    }

    pub fn path(&self) -> &Path {
        &self.staging
    }

    /// 调用方必须先关闭临时数据库连接。
    pub fn commit(self, course_id: &str) -> Result<()> {
        if self.target.is_file() {
            // SQLite 快照包含已提交的 WAL 内容，不替换目标文件或使已有连接失效。
            let original = Connection::open(&self.target)?;
            original.execute_batch("PRAGMA busy_timeout=5000")?;
            let backup = self.backup_path();
            original
                .execute("VACUUM main INTO ?1", [backup.to_string_lossy().as_ref()])
                .with_context(|| format!("备份旧归档失败 {}", backup.display()))?;
        }
        let mut target = store::open_path(&self.target)?;
        store::replace_course(&mut target, &self.staging, course_id)
            .context("提交重建归档失败，保留原课程任务数据")
    }

    fn backup_path(&self) -> PathBuf {
        let mut path = self
            .target
            .with_extension(format!("previous-{}.db", std::process::id()));
        let mut suffix = 2u32;
        while path.exists() {
            path = self
                .target
                .with_extension(format!("previous-{}-{suffix}.db", std::process::id()));
            suffix += 1;
        }
        path
    }
}

impl Drop for Rebuild {
    fn drop(&mut self) {
        // 仅清理本次操作的临时文件，保留完成的备份。
        for suffix in ["", "-wal", "-shm"] {
            let mut name = self.staging.as_os_str().to_os_string();
            name.push(suffix);
            if let Err(error) = std::fs::remove_file(PathBuf::from(name))
                && error.kind() != std::io::ErrorKind::NotFound
            {
                log::warn!(
                    "清理临时归档失败 {}{suffix}: {error}",
                    self.staging.display()
                );
            }
        }
    }
}
