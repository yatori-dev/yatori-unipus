//! 按账号隔离的内容归档与离线任务查询。

mod format;
mod preview;
mod rebuild;
mod service;
mod store;

pub use preview::Preview;
pub(crate) use preview::load_preview;
pub(crate) use service::archive_course;
pub use service::{ArchiveOptions, ArchiveSummary};
pub use store::{
    ArchiveStats, ArchivedModule, ArchivedQuestion, ArchivedTask, ArchivedTaskSummary,
    ArchivedUnit, TaskCounts, TaskKey,
};

use anyhow::Result;
use rusqlite::Connection;
use std::path::{Path, PathBuf};

/// 打开指定账号的本地归档，不请求平台。
///
/// 目录由宿主提供；数据库不存在时创建，打开时执行既有 schema 迁移。
pub struct Archive {
    connection: Connection,
    path: PathBuf,
}

impl Archive {
    pub fn open(data_dir: impl AsRef<Path>, open_id: &str) -> Result<Self> {
        if open_id.trim().is_empty() {
            anyhow::bail!("open_id 不能为空");
        }
        let path = store::archive_path(data_dir.as_ref(), open_id);
        Ok(Self {
            connection: store::open_path(&path)?,
            path,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn tree(&self, course_id: &str) -> Result<Vec<ArchivedUnit>> {
        store::load_tree(&self.connection, course_id)
    }

    pub fn task(&self, key: TaskKey<'_>) -> Result<Option<ArchivedTask>> {
        validate_key(key)?;
        store::load_stored(&self.connection, key)
    }

    pub fn preview(&self, key: TaskKey<'_>) -> Result<Option<Preview>> {
        Ok(self.task(key)?.map(preview::from_stored))
    }

    pub fn summary(&self, course_id: &str) -> Result<ArchiveStats> {
        store::statistics(&self.connection, course_id)
    }

    pub fn summary_text(&self, course_id: &str) -> Result<Option<String>> {
        Ok(format::summary_text(&self.summary(course_id)?, &self.path))
    }
}

fn validate_key(key: TaskKey<'_>) -> Result<()> {
    if [key.course_id, key.unit_id, key.group_id]
        .iter()
        .any(|part| part.trim().is_empty())
    {
        anyhow::bail!("course_id、unit_id 和 group_id 均不能为空");
    }
    Ok(())
}

pub(crate) fn sync_task_status(
    session: &crate::api::session::Session,
    task: &crate::api::course::GroupTask,
    passed: bool,
) -> bool {
    let result: Result<bool> = (|| {
        let archive = Archive::open(session.data_dir(), &session.open_id())?;
        store::set_passed(
            &archive.connection,
            TaskKey::new(&session.course_id(), &task.unit_id, &task.group_id),
            passed,
        )
    })();
    match result {
        Ok(changed) => changed,
        Err(error) => {
            log::warn!("同步归档状态失败: {error:#}");
            false
        }
    }
}
