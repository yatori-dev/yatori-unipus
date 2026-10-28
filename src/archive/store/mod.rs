mod model;
mod read;
mod replacement;
mod schema;
mod write;

#[cfg(test)]
mod tests;

pub use model::{
    ArchiveStats, ArchivedModule, ArchivedQuestion, ArchivedTask, ArchivedTaskSummary,
    ArchivedUnit, TaskCounts, TaskKey,
};
pub(crate) use model::{MediaEntry, TaskInput, TaskStatus};
pub(crate) use read::{load_stored, load_tree, statistics, task_count, task_status};
pub(crate) use replacement::replace_course;
pub(crate) use schema::init;
pub(crate) use write::{now_local_string, save_meta, save_task, set_passed, update_status};

use anyhow::{Context, Result};
use rusqlite::Connection;
use std::path::{Path, PathBuf};

fn encode_path_component(value: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_') {
            encoded.push(char::from(byte));
        } else {
            encoded.push('%');
            encoded.push(char::from(HEX[(byte >> 4) as usize]));
            encoded.push(char::from(HEX[(byte & 0x0f) as usize]));
        }
    }
    encoded
}

fn archive_dir(data_dir: &Path) -> PathBuf {
    if data_dir == Path::new(".") {
        PathBuf::from(ARCHIVE_DIR)
    } else {
        data_dir.join(ARCHIVE_DIR)
    }
}

const ARCHIVE_DIR: &str = "dump_text";
const DB_FILE: &str = "dump.db";

pub(crate) fn archive_path(data_dir: &Path, open_id: &str) -> PathBuf {
    if open_id.is_empty() {
        archive_dir(data_dir).join(DB_FILE)
    } else {
        archive_dir(data_dir).join(format!("dump-{}.db", encode_path_component(open_id)))
    }
}

pub(crate) fn open_path(path: &Path) -> Result<Connection> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let conn =
        Connection::open(path).with_context(|| format!("打开数据库 {} 失败", path.display()))?;
    init(&conn)?;
    Ok(conn)
}
