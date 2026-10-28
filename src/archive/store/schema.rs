use super::{read::get_meta, write::save_meta};
use anyhow::Result;
use rusqlite::Connection;

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS tasks (
    id          INTEGER PRIMARY KEY,
    unit_index  INTEGER NOT NULL,
    unit_id     TEXT    NOT NULL,
    unit_label  TEXT,
    group_id    TEXT    NOT NULL,
    tab_type    TEXT    NOT NULL,
    group_type  TEXT    NOT NULL,
    kind        TEXT    NOT NULL,
    required    INTEGER NOT NULL,
    passed      INTEGER NOT NULL,
    raw_content TEXT,
    raw_json    TEXT,
    course_id   TEXT,
    updated_at  TEXT    NOT NULL,
    UNIQUE(course_id, unit_id, group_id)
);
CREATE TABLE IF NOT EXISTS meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS modules (
    id            INTEGER PRIMARY KEY,
    task_id       INTEGER NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    idx           INTEGER NOT NULL,
    module_type   TEXT    NOT NULL,
    reply_type    TEXT    NOT NULL,
    instance_id   TEXT    NOT NULL,
    direction     TEXT    NOT NULL,
    material      TEXT    NOT NULL,
    transcript    TEXT    NOT NULL,
    word_bank_json TEXT,
    UNIQUE(task_id, idx)
);
CREATE TABLE IF NOT EXISTS media (
    id        INTEGER PRIMARY KEY,
    module_id INTEGER NOT NULL REFERENCES modules(id) ON DELETE CASCADE,
    idx       INTEGER NOT NULL,
    url       TEXT    NOT NULL,
    text      TEXT,
    error     TEXT
);
CREATE TABLE IF NOT EXISTS vocabulary (
    id        INTEGER PRIMARY KEY,
    module_id INTEGER NOT NULL REFERENCES modules(id) ON DELETE CASCADE,
    idx       INTEGER NOT NULL,
    name      TEXT    NOT NULL,
    sound     TEXT    NOT NULL
);
CREATE TABLE IF NOT EXISTS questions (
    id            INTEGER PRIMARY KEY,
    module_id     INTEGER NOT NULL REFERENCES modules(id) ON DELETE CASCADE,
    idx           INTEGER NOT NULL,
    reply_type    TEXT    NOT NULL,
    question_type TEXT    NOT NULL,
    question_text TEXT    NOT NULL,
    options_json  TEXT    NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_modules_task     ON modules(task_id);
CREATE INDEX IF NOT EXISTS idx_media_module     ON media(module_id);
CREATE INDEX IF NOT EXISTS idx_vocabulary_module ON vocabulary(module_id);
CREATE INDEX IF NOT EXISTS idx_questions_module ON questions(module_id);
"#;

const SCHEMA_INDEXES: &str = r#"
CREATE INDEX IF NOT EXISTS idx_tasks_unit       ON tasks(course_id, unit_id);
CREATE INDEX IF NOT EXISTS idx_tasks_course     ON tasks(course_id, unit_index);
"#;

const SCHEMA_VERSION: i64 = 2;

pub(crate) fn init(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "PRAGMA journal_mode=WAL; PRAGMA busy_timeout=5000; PRAGMA foreign_keys=ON;",
    )?;
    conn.execute_batch(SCHEMA)?;

    ensure_column(conn, "tasks", "raw_json", "raw_json TEXT")?;
    ensure_column(conn, "tasks", "course_id", "course_id TEXT")?;
    migrate_task_identity(conn)?;
    conn.execute_batch(SCHEMA_INDEXES)?;

    if let Some(cid) = get_meta(conn, "course_id")?
        && !cid.is_empty()
    {
        conn.execute(
            "UPDATE tasks SET course_id=?1 WHERE course_id IS NULL",
            [&cid],
        )?;
    }
    save_meta(conn, "schema_version", &SCHEMA_VERSION.to_string())?;
    Ok(())
}

fn has_course_identity(conn: &Connection) -> Result<bool> {
    let mut indexes = conn.prepare("PRAGMA index_list(tasks)")?;
    let rows = indexes.query_map([], |row| {
        Ok((row.get::<_, String>(1)?, row.get::<_, i64>(2)? != 0))
    })?;
    for row in rows {
        let (name, unique) = row?;
        if !unique {
            continue;
        }
        let pragma = format!("PRAGMA index_info('{}')", name.replace('\'', "''"));
        let mut info = conn.prepare(&pragma)?;
        let columns: Vec<String> = info
            .query_map([], |r| r.get::<_, String>(2))?
            .collect::<rusqlite::Result<_>>()?;
        if columns == ["course_id", "unit_id", "group_id"] {
            return Ok(true);
        }
    }
    Ok(false)
}

fn migrate_task_identity(conn: &Connection) -> Result<()> {
    if has_course_identity(conn)? {
        return Ok(());
    }
    conn.execute_batch("PRAGMA foreign_keys=OFF; BEGIN;")?;
    let result = (|| -> Result<()> {
        conn.execute_batch(
            "CREATE TABLE tasks_v2 (
                id INTEGER PRIMARY KEY,
                unit_index INTEGER NOT NULL,
                unit_id TEXT NOT NULL,
                unit_label TEXT,
                group_id TEXT NOT NULL,
                tab_type TEXT NOT NULL,
                group_type TEXT NOT NULL,
                kind TEXT NOT NULL,
                required INTEGER NOT NULL,
                passed INTEGER NOT NULL,
                raw_content TEXT,
                raw_json TEXT,
                course_id TEXT,
                updated_at TEXT NOT NULL,
                UNIQUE(course_id, unit_id, group_id)
            );
            INSERT INTO tasks_v2
                SELECT id, unit_index, unit_id, unit_label, group_id, tab_type, group_type,
                       kind, required, passed, raw_content, raw_json, course_id, updated_at
                FROM tasks;
            DROP TABLE tasks;
            ALTER TABLE tasks_v2 RENAME TO tasks;
            CREATE INDEX IF NOT EXISTS idx_tasks_unit ON tasks(course_id, unit_id);
            CREATE INDEX IF NOT EXISTS idx_tasks_course ON tasks(course_id, unit_index);
        ",
        )?;
        conn.execute_batch("COMMIT; PRAGMA foreign_keys=ON;")?;
        Ok(())
    })();
    if result.is_err() {
        let _ = conn.execute_batch("ROLLBACK; PRAGMA foreign_keys=ON;");
    }
    result
}

fn ensure_column(conn: &Connection, table: &str, column: &str, ddl: &str) -> Result<()> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({})", table))?;
    let exists = stmt
        .query_map([], |r| r.get::<_, String>(1))?
        .filter_map(|x| x.ok())
        .any(|name| name == column);
    drop(stmt);
    if !exists {
        conn.execute_batch(&format!("ALTER TABLE {} ADD COLUMN {}", table, ddl))?;
    }
    Ok(())
}
