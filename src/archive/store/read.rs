use super::model::{
    ArchiveStats, ArchivedModule, ArchivedQuestion, ArchivedTask, ArchivedTaskSummary,
    ArchivedUnit, TaskKey, TaskStatus,
};
use anyhow::Result;
use rusqlite::{Connection, OptionalExtension, params};

pub(crate) fn task_count(conn: &Connection, course_id: &str) -> Result<usize> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM tasks WHERE course_id=?1",
        [course_id],
        |r| r.get(0),
    )?;
    Ok(n as usize)
}

pub(crate) fn get_meta(conn: &Connection, key: &str) -> Result<Option<String>> {
    let mut stmt = conn.prepare("SELECT value FROM meta WHERE key=?1")?;
    let mut rows = stmt.query([key])?;
    match rows.next()? {
        Some(r) => Ok(Some(r.get(0)?)),
        None => Ok(None),
    }
}

pub(crate) fn load_tree(conn: &Connection, course_id: &str) -> Result<Vec<ArchivedUnit>> {
    let mut stmt = conn.prepare(
        "SELECT unit_index, unit_id, unit_label, group_id, tab_type, group_type, kind,
                required, passed
         FROM tasks WHERE course_id=?1 ORDER BY unit_index, rowid",
    )?;
    let mut rows = stmt.query([course_id])?;
    let mut units: Vec<ArchivedUnit> = Vec::new();
    while let Some(r) = rows.next()? {
        let unit_index: i64 = r.get(0)?;
        let unit_id: String = r.get(1)?;
        let unit_label: Option<String> = r.get(2)?;
        let task = ArchivedTaskSummary {
            group_id: r.get(3)?,
            tab_type: r.get(4)?,
            group_type: r.get(5)?,
            kind: r.get(6)?,
            required: r.get::<_, i64>(7)? != 0,
            passed: r.get::<_, i64>(8)? != 0,
        };
        match units.last_mut() {
            Some(u) if u.unit_id == unit_id && u.unit_index == unit_index => {
                if u.unit_label.is_none() && unit_label.is_some() {
                    u.unit_label = unit_label;
                }
                u.tasks.push(task);
            }
            _ => units.push(ArchivedUnit {
                unit_index,
                unit_id,
                unit_label,
                tasks: vec![task],
            }),
        }
    }
    Ok(units)
}

pub(crate) fn load_stored(conn: &Connection, key: TaskKey<'_>) -> Result<Option<ArchivedTask>> {
    let mut stmt = conn.prepare(
        "SELECT id, unit_id, unit_index, unit_label, group_id, tab_type, group_type, kind,
                required, passed, raw_content, raw_json
         FROM tasks WHERE course_id=?1 AND unit_id=?2 AND group_id=?3",
    )?;
    let mut rows = stmt.query(params![key.course_id, key.unit_id, key.group_id])?;
    let Some(r) = rows.next()? else {
        return Ok(None);
    };
    let task_id: i64 = r.get(0)?;
    let mut task = ArchivedTask {
        unit_id: r.get(1)?,
        unit_index: r.get(2)?,
        unit_label: r.get(3)?,
        group_id: r.get(4)?,
        tab_type: r.get(5)?,
        group_type: r.get(6)?,
        kind: r.get(7)?,
        required: r.get::<_, i64>(8)? != 0,
        passed: r.get::<_, i64>(9)? != 0,
        raw_content: r.get(10)?,
        raw_json: r.get(11)?,
        modules: Vec::new(),
        vocab: Vec::new(),
    };
    drop(rows);

    let mut mstmt = conn.prepare(
        "SELECT id, module_type, reply_type, instance_id, direction, material, transcript,
                word_bank_json
         FROM modules WHERE task_id=?1 ORDER BY idx",
    )?;
    let mut mrows = mstmt.query([task_id])?;
    while let Some(m) = mrows.next()? {
        let module_id: i64 = m.get(0)?;
        let module_type: String = m.get(1)?;
        let mut media_stmt =
            conn.prepare("SELECT url FROM media WHERE module_id=?1 ORDER BY idx")?;
        let media_urls: Vec<String> = media_stmt
            .query_map([module_id], |x| x.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        let mut q_stmt = conn.prepare(
            "SELECT reply_type, question_type, question_text, options_json
             FROM questions WHERE module_id=?1 ORDER BY idx",
        )?;
        let questions: Vec<ArchivedQuestion> = q_stmt
            .query_map([module_id], |x| {
                Ok(ArchivedQuestion {
                    reply_type: x.get(0)?,
                    question_type: x.get(1)?,
                    question_text: x.get(2)?,
                    options_json: x.get(3)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        if module_type == "vocabulary" {
            let mut v_stmt =
                conn.prepare("SELECT name, sound FROM vocabulary WHERE module_id=?1 ORDER BY idx")?;
            task.vocab = v_stmt
                .query_map([module_id], |x| Ok((x.get(0)?, x.get(1)?)))?
                .collect::<rusqlite::Result<_>>()?;
        }
        task.modules.push(ArchivedModule {
            module_type,
            reply_type: m.get(2)?,
            instance_id: m.get(3)?,
            direction: m.get(4)?,
            material: m.get(5)?,
            transcript: m.get(6)?,
            word_bank_json: m.get::<_, Option<String>>(7)?.unwrap_or_default(),
            media_urls,
            questions,
        });
    }
    Ok(Some(task))
}

pub(crate) fn task_status(conn: &Connection, key: TaskKey<'_>) -> Result<Option<TaskStatus>> {
    conn.query_row(
        "SELECT required, passed, kind='task' AND raw_json IS NULL
         FROM tasks WHERE course_id=?1 AND unit_id=?2 AND group_id=?3",
        params![key.course_id, key.unit_id, key.group_id],
        |row| {
            Ok(TaskStatus {
                required: row.get::<_, i64>(0)? != 0,
                passed: row.get::<_, i64>(1)? != 0,
                needs_refresh: row.get::<_, i64>(2)? != 0,
            })
        },
    )
    .optional()
    .map_err(Into::into)
}

pub(crate) fn statistics(conn: &Connection, course_id: &str) -> Result<ArchiveStats> {
    let units = load_tree(conn, course_id)?;
    let mut stats = ArchiveStats {
        units,
        ..Default::default()
    };
    for task in stats.units.iter().flat_map(|unit| &unit.tasks) {
        stats.total_tasks += 1;
        let counts = if task.required {
            &mut stats.required
        } else {
            &mut stats.optional
        };
        counts.total += 1;
        counts.passed += usize::from(task.passed);
    }
    Ok(stats)
}
