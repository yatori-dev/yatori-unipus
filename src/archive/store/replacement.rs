use anyhow::Result;
use rusqlite::{Connection, Transaction, TransactionBehavior, params};
use std::path::Path;

/// 在事务中发布当前课程，保留其他课程的现有内容。
pub(crate) fn replace_course(
    target: &mut Connection,
    staging: &Path,
    course_id: &str,
) -> Result<()> {
    target.execute(
        "ATTACH DATABASE ?1 AS rebuilt",
        [staging.to_string_lossy().as_ref()],
    )?;
    let result = (|| -> Result<()> {
        let tx = target.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute("DELETE FROM main.tasks WHERE course_id=?1", [course_id])?;
        let task_offset = max_id(&tx, "tasks")?;
        let module_offset = max_id(&tx, "modules")?;
        tx.execute(
            "INSERT INTO main.tasks
                (id, unit_index, unit_id, unit_label, group_id, tab_type, group_type, kind,
                 required, passed, raw_content, raw_json, course_id, updated_at)
             SELECT id+?1, unit_index, unit_id, unit_label, group_id, tab_type, group_type, kind,
                    required, passed, raw_content, raw_json, course_id, updated_at
             FROM rebuilt.tasks WHERE course_id=?2",
            params![task_offset, course_id],
        )?;
        tx.execute(
            "INSERT INTO main.modules
                (id, task_id, idx, module_type, reply_type, instance_id, direction, material,
                 transcript, word_bank_json)
             SELECT m.id+?1, m.task_id+?2, m.idx, m.module_type, m.reply_type, m.instance_id,
                    m.direction, m.material, m.transcript, m.word_bank_json
             FROM rebuilt.modules m JOIN rebuilt.tasks t ON t.id=m.task_id
             WHERE t.course_id=?3",
            params![module_offset, task_offset, course_id],
        )?;
        copy_children(&tx, course_id, module_offset)?;
        tx.execute(
            "INSERT INTO main.meta (key, value)
             SELECT key, value FROM rebuilt.meta
             WHERE key IN ('course_id', 'course_name', ?1)
             ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            [format!("course_name:{course_id}")],
        )?;
        tx.commit()?;
        Ok(())
    })();
    let detached = target.execute_batch("DETACH DATABASE rebuilt");
    result?;
    detached?;
    Ok(())
}

fn max_id(tx: &Transaction<'_>, table: &str) -> Result<i64> {
    Ok(tx.query_row(
        &format!("SELECT COALESCE(MAX(id), 0) FROM main.{table}"),
        [],
        |row| row.get(0),
    )?)
}

fn copy_children(tx: &Transaction<'_>, course_id: &str, module_offset: i64) -> Result<()> {
    for (table, columns) in [
        ("media", "idx, url, text, error"),
        ("vocabulary", "idx, name, sound"),
        (
            "questions",
            "idx, reply_type, question_type, question_text, options_json",
        ),
    ] {
        let selected = columns
            .split(", ")
            .map(|column| format!("c.{column}"))
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            "INSERT INTO main.{table} (module_id, {columns})
             SELECT c.module_id+?1, {selected}
             FROM rebuilt.{table} c
             JOIN rebuilt.modules m ON m.id=c.module_id
             JOIN rebuilt.tasks t ON t.id=m.task_id WHERE t.course_id=?2"
        );
        tx.execute(&sql, params![module_offset, course_id])?;
    }
    Ok(())
}
