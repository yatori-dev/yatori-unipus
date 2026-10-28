use super::model::{TaskInput, TaskKey, TaskStatus};
use crate::model::OptionItem;
use anyhow::Result;
use rusqlite::{Connection, params};

pub(crate) fn now_local_string() -> String {
    let now = time::OffsetDateTime::now_local().unwrap_or_else(|_| time::OffsetDateTime::now_utc());
    let fmt = time::macros::format_description!("[year]-[month]-[day] [hour]:[minute]:[second]");
    now.format(fmt).unwrap_or_default()
}

fn options_json(opts: &[OptionItem]) -> Result<String> {
    Ok(serde_json::to_string(opts)?)
}

pub(crate) fn save_task(conn: &mut Connection, t: &TaskInput<'_>) -> Result<i64> {
    let tx = conn.transaction()?;
    let now = now_local_string();
    tx.execute(
        "INSERT INTO tasks (unit_index, unit_id, unit_label, group_id, tab_type, group_type,
                            kind, required, passed, raw_content, raw_json, course_id, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
         ON CONFLICT(course_id, unit_id, group_id) DO UPDATE SET
            unit_index=excluded.unit_index,
            unit_label=excluded.unit_label,
            tab_type=excluded.tab_type,
            group_type=excluded.group_type,
            kind=excluded.kind,
            required=excluded.required,
            passed=excluded.passed,
            raw_content=excluded.raw_content,
            raw_json=excluded.raw_json,
            course_id=excluded.course_id,
            updated_at=excluded.updated_at",
        params![
            t.unit_index as i64,
            t.key.unit_id,
            t.unit_label,
            t.key.group_id,
            t.tab_type,
            t.group_type,
            t.kind,
            t.required as i64,
            t.passed as i64,
            t.raw_content,
            t.raw_json,
            t.key.course_id,
            now
        ],
    )?;
    let task_id: i64 = tx.query_row(
        "SELECT id FROM tasks WHERE course_id=?1 AND unit_id=?2 AND group_id=?3",
        params![t.key.course_id, t.key.unit_id, t.key.group_id],
        |r| r.get(0),
    )?;

    tx.execute("DELETE FROM modules WHERE task_id=?1", [task_id])?;

    if let Some(group) = t.group {
        for (mi, m) in group.modules.iter().enumerate() {
            tx.execute(
                "INSERT INTO modules (task_id, idx, module_type, reply_type, instance_id,
                                      direction, material, transcript, word_bank_json)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    task_id,
                    mi as i64,
                    m.module_type,
                    m.reply_type,
                    m.instance_id,
                    m.direction,
                    m.material,
                    m.transcript,
                    options_json(&m.word_bank)?
                ],
            )?;
            let module_id = tx.last_insert_rowid();

            if m.module_type == "vocabulary" {
                for (wi, w) in t.vocab.iter().enumerate() {
                    tx.execute(
                        "INSERT INTO vocabulary (module_id, idx, name, sound) VALUES (?1, ?2, ?3, ?4)",
                        params![module_id, wi as i64, w.name, w.sound],
                    )?;
                }
            }
            for e in t.media.iter().filter(|e| e.module_idx == mi) {
                tx.execute(
                    "INSERT INTO media (module_id, idx, url, text, error) VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![module_id, e.url_idx as i64, e.url, e.text, e.error],
                )?;
            }
            for (ci, c) in m.children.iter().enumerate() {
                tx.execute(
                    "INSERT INTO questions (module_id, idx, reply_type, question_type,
                                            question_text, options_json)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![
                        module_id,
                        ci as i64,
                        c.reply_type,
                        c.question_type,
                        c.question_text,
                        options_json(&c.options)?
                    ],
                )?;
            }
        }
    }
    tx.commit()?;
    Ok(task_id)
}

pub(crate) fn set_passed(conn: &Connection, key: TaskKey<'_>, passed: bool) -> Result<bool> {
    let n = conn.execute(
        "UPDATE tasks SET passed=?1, updated_at=?2 WHERE course_id=?3 AND unit_id=?4 AND group_id=?5",
        params![passed as i64, now_local_string(), key.course_id, key.unit_id, key.group_id],
    )?;
    Ok(n > 0)
}

pub(crate) fn update_status(
    conn: &Connection,
    key: TaskKey<'_>,
    unit_index: usize,
    unit_label: Option<&str>,
    tab_type: &str,
    status: TaskStatus,
) -> Result<bool> {
    let n = conn.execute(
        "UPDATE tasks SET unit_index=?4, unit_label=COALESCE(?5, unit_label), tab_type=?6,
                          required=?7, passed=?8, updated_at=?9
         WHERE course_id=?1 AND unit_id=?2 AND group_id=?3",
        params![
            key.course_id,
            key.unit_id,
            key.group_id,
            unit_index as i64,
            unit_label,
            tab_type,
            status.required as i64,
            status.passed as i64,
            now_local_string()
        ],
    )?;
    Ok(n > 0)
}

pub(crate) fn save_meta(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO meta (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        params![key, value],
    )?;
    Ok(())
}
