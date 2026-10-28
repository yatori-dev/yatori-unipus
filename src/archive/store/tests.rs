use super::read::get_meta;
use super::*;
use crate::model::{ChildQ, Module, OptionItem, ParsedGroup, VocabWord};

fn conn() -> Connection {
    let c = Connection::open_in_memory().unwrap();
    init(&c).unwrap();
    c
}

fn option(name: &str, value: &str, text: &str) -> OptionItem {
    OptionItem {
        name: name.into(),
        value: value.into(),
        text: text.into(),
    }
}

fn module(module_type: &str) -> Module {
    Module {
        instance_id: "inst-1".into(),
        module_type: module_type.into(),
        direction: "请阅读并作答".into(),
        material: "材料".repeat(200),
        media_sources: vec!["http://a/1.mp3".into(), "http://a/2.mp4".into()],
        transcript: "字幕文本".into(),
        reply_type: "singlechoice".into(),
        word_bank: vec![option("wb", "v", "t")],
        children: vec![ChildQ {
            question_type: "basic".into(),
            reply_type: "singlechoice".into(),
            question_text: "题干".repeat(100),
            options: vec![option("A", "a", "选项A"), option("B", "b", "选项B")],
            option_count: 2,
        }],
    }
}

fn input<'a>(
    group: &'a ParsedGroup,
    vocab: &'a [VocabWord],
    media: &'a [MediaEntry],
) -> TaskInput<'a> {
    TaskInput {
        unit_index: 1,
        key: TaskKey::new("course-x", "u1", "g1"),
        unit_label: Some("Unit 1"),
        tab_type: "task",
        group_type: "singlechoice",
        kind: "task",
        required: true,
        passed: false,
        raw_content: None,
        raw_json: Some("{\n  \"demo\": true\n}"),
        group: Some(group),
        vocab,
        media,
    }
}

#[test]
fn save_task_roundtrip_stores_full_text() {
    let mut c = conn();
    let group = ParsedGroup {
        modules: vec![module("vocabulary")],
    };
    let vocab = vec![VocabWord {
        name: "apple".into(),
        sound: "http://s/apple.mp3".into(),
    }];
    let media = vec![
        MediaEntry {
            module_idx: 0,
            url_idx: 0,
            url: "http://a/1.mp3".into(),
            text: Some("转写".repeat(6000)),
            error: None,
        },
        MediaEntry {
            module_idx: 0,
            url_idx: 1,
            url: "http://a/2.mp4".into(),
            text: None,
            error: Some("下载失败".into()),
        },
    ];
    let id = save_task(&mut c, &input(&group, &vocab, &media)).unwrap();
    assert!(id > 0);
    assert!(
        task_status(&c, TaskKey::new("course-x", "u1", "g1"))
            .unwrap()
            .is_some()
    );
    assert_eq!(
        task_status(&c, TaskKey::new("course-x", "u1", "g1")).unwrap(),
        Some(TaskStatus {
            required: true,
            passed: false,
            needs_refresh: false
        })
    );

    let module_count: i64 = c
        .query_row("SELECT COUNT(*) FROM modules", [], |r| r.get(0))
        .unwrap();
    assert_eq!(module_count, 1);
    let (material, transcript): (String, String) = c
        .query_row("SELECT material, transcript FROM modules", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap();
    assert_eq!(material.chars().count(), 400);
    assert_eq!(transcript, "字幕文本");

    let vocab_count: i64 = c
        .query_row("SELECT COUNT(*) FROM vocabulary", [], |r| r.get(0))
        .unwrap();
    assert_eq!(vocab_count, 1);

    let media_count: i64 = c
        .query_row("SELECT COUNT(*) FROM media", [], |r| r.get(0))
        .unwrap();
    assert_eq!(media_count, 2);
    let full_len: i64 = c
        .query_row("SELECT length(text) FROM media WHERE idx=0", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(full_len as usize, "转写".chars().count() * 6000);

    let (qtext, opts): (String, String) = c
        .query_row(
            "SELECT question_text, options_json FROM questions",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(qtext.chars().count(), 200);
    let v: serde_json::Value = serde_json::from_str(&opts).unwrap();
    assert_eq!(v.as_array().unwrap().len(), 2);
    assert_eq!(v[0]["name"], "A");
}

#[test]
fn save_task_upsert_keeps_single_row_and_updates_status() {
    let mut c = conn();
    let group = ParsedGroup {
        modules: vec![module("basic")],
    };
    save_task(&mut c, &input(&group, &[], &[])).unwrap();
    let mut t2 = input(&group, &[], &[]);
    t2.passed = true;
    t2.required = false;
    save_task(&mut c, &t2).unwrap();

    assert_eq!(task_count(&c, "course-x").unwrap(), 1);
    assert_eq!(
        task_status(&c, TaskKey::new("course-x", "u1", "g1")).unwrap(),
        Some(TaskStatus {
            required: false,
            passed: true,
            needs_refresh: false
        })
    );
    let module_count: i64 = c
        .query_row("SELECT COUNT(*) FROM modules", [], |r| r.get(0))
        .unwrap();
    assert_eq!(module_count, 1);
}

#[test]
fn set_passed_updates_task() {
    let mut c = conn();
    let group = ParsedGroup {
        modules: vec![module("basic")],
    };
    save_task(&mut c, &input(&group, &[], &[])).unwrap();
    assert!(set_passed(&c, TaskKey::new("course-x", "u1", "g1"), true).unwrap());
    assert_eq!(
        task_status(&c, TaskKey::new("course-x", "u1", "g1")).unwrap(),
        Some(TaskStatus {
            required: true,
            passed: true,
            needs_refresh: false
        })
    );
    assert!(!set_passed(&c, TaskKey::new("course-x", "u9", "g9"), true).unwrap());
}

#[test]
fn replace_course_preserves_other_courses_and_live_readers_and_rolls_back_failure() {
    let root = std::env::temp_dir().join(format!(
        "unipus-rebuild-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
    ));
    let path = archive_path(&root, "account");
    let mut target = open_path(&path).unwrap();
    let group = ParsedGroup {
        modules: vec![module("vocabulary")],
    };
    let vocab = vec![VocabWord {
        name: "apple".into(),
        sound: "sound-url".into(),
    }];
    let media = vec![MediaEntry {
        module_idx: 0,
        url_idx: 0,
        url: "media-url".into(),
        text: Some("transcript".into()),
        error: None,
    }];
    save_task(&mut target, &input(&group, &vocab, &media)).unwrap();
    let mut other = input(&group, &vocab, &media);
    other.key = TaskKey::new("course-y", "u1", "g2");
    save_task(&mut target, &other).unwrap();
    save_meta(&target, "course_name:course-y", "Other course").unwrap();
    let reader = crate::archive::Archive::open(&root, "account").unwrap();
    assert!(
        reader
            .task(TaskKey::new("course-x", "u1", "g1"))
            .unwrap()
            .is_some()
    );

    let rebuild = crate::archive::rebuild::Rebuild::new(&path);
    let mut stage = open_path(rebuild.path()).unwrap();
    let mut replacement = input(&group, &vocab, &media);
    replacement.key.group_id = "g3";
    replacement.passed = true;
    save_task(&mut stage, &replacement).unwrap();
    save_meta(&stage, "course_name:course-x", "Rebuilt course").unwrap();
    drop(stage);
    rebuild.commit("course-x").unwrap();

    assert!(
        reader
            .task(TaskKey::new("course-x", "u1", "g1"))
            .unwrap()
            .is_none()
    );
    let rebuilt = reader
        .task(TaskKey::new("course-x", "u1", "g3"))
        .unwrap()
        .unwrap();
    assert!(rebuilt.passed);
    assert_eq!(rebuilt.modules.len(), 1);
    assert_eq!(rebuilt.modules[0].questions.len(), 1);
    assert_eq!(rebuilt.modules[0].media_urls, vec!["media-url"]);
    assert_eq!(
        rebuilt.vocab,
        vec![("apple".to_string(), "sound-url".to_string())]
    );
    assert!(
        reader
            .task(TaskKey::new("course-y", "u1", "g2"))
            .unwrap()
            .is_some()
    );
    assert_eq!(
        get_meta(&target, "course_name:course-y")
            .unwrap()
            .as_deref(),
        Some("Other course")
    );
    assert_eq!(
        get_meta(&target, "course_name:course-x")
            .unwrap()
            .as_deref(),
        Some("Rebuilt course")
    );
    let backups: Vec<_> = std::fs::read_dir(path.parent().unwrap())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.to_string_lossy().contains("previous-"))
        .collect();
    assert_eq!(backups.len(), 1);
    let backup = Connection::open(&backups[0]).unwrap();
    assert!(
        load_stored(&backup, TaskKey::new("course-x", "u1", "g1"))
            .unwrap()
            .is_some()
    );
    assert!(
        load_stored(&backup, TaskKey::new("course-y", "u1", "g2"))
            .unwrap()
            .is_some()
    );
    drop(backup);

    let failed = crate::archive::rebuild::Rebuild::new(&path);
    let failed_stage = failed.path().to_path_buf();
    let mut stage = open_path(failed.path()).unwrap();
    replacement.key.group_id = "g4";
    save_task(&mut stage, &replacement).unwrap();
    drop(stage);
    target
        .execute_batch(
            "CREATE TRIGGER reject_replacement BEFORE INSERT ON tasks
             BEGIN SELECT RAISE(ABORT, 'replacement rejected'); END",
        )
        .unwrap();
    assert!(failed.commit("course-x").is_err());
    assert!(
        reader
            .task(TaskKey::new("course-x", "u1", "g3"))
            .unwrap()
            .is_some()
    );
    assert!(
        reader
            .task(TaskKey::new("course-x", "u1", "g4"))
            .unwrap()
            .is_none()
    );
    assert!(
        reader
            .task(TaskKey::new("course-y", "u1", "g2"))
            .unwrap()
            .is_some()
    );
    assert!(!failed_stage.exists());
    drop(reader);
    drop(target);
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn view_only_content_roundtrip() {
    let mut c = conn();
    let raw = "页面原文".repeat(3000);
    let t = TaskInput {
        unit_index: 2,
        key: TaskKey::new("course-x", "u2", "g2"),
        unit_label: None,
        tab_type: "text",
        group_type: "view-only",
        kind: "view-only",
        required: false,
        passed: true,
        raw_content: Some(raw.as_str()),
        raw_json: None,
        group: None,
        vocab: &[],
        media: &[],
    };
    save_task(&mut c, &t).unwrap();
    let raw: String = c
        .query_row(
            "SELECT raw_content FROM tasks WHERE group_id='g2'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(raw.chars().count(), 4 * 3000);
}

#[test]
fn summary_text_empty_and_filled() {
    let mut c = conn();
    assert!(
        crate::archive::format::summary_text(
            &statistics(&c, "course-x").unwrap(),
            Path::new(":memory:")
        )
        .is_none()
    );

    let group = ParsedGroup {
        modules: vec![module("basic")],
    };
    save_task(&mut c, &input(&group, &[], &[])).unwrap();
    let mut t2 = input(&group, &[], &[]);
    t2.key.group_id = "g2";
    t2.tab_type = "text";
    t2.required = false;
    t2.passed = true;
    save_task(&mut c, &t2).unwrap();

    let text = crate::archive::format::summary_text(
        &statistics(&c, "course-x").unwrap(),
        Path::new(":memory:"),
    )
    .unwrap();
    assert!(text.contains("任务组: 2 个"), "{}", text);
    assert!(text.contains("必修: 1 (已完成 0, 未完成 1)"), "{}", text);
    assert!(text.contains("选修: 1 (已完成 1, 未完成 0)"), "{}", text);
    assert!(
        text.contains("01_u1: 必修 1 (已完成 0) / 选修 1 (已完成 1)"),
        "{}",
        text
    );
    assert!(
        text.contains("01_u1/singlechoice/g1 [必修 未完成]"),
        "{}",
        text
    );
}

#[test]
fn init_migrates_missing_raw_json() {
    let c = Connection::open_in_memory().unwrap();

    c.execute_batch(
        "CREATE TABLE tasks (
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
                updated_at TEXT NOT NULL,
                UNIQUE(unit_id, group_id)
            );",
    )
    .unwrap();
    init(&c).unwrap();
    let mut stmt = c.prepare("PRAGMA table_info(tasks)").unwrap();
    let cols: Vec<String> = stmt
        .query_map([], |r| r.get::<_, String>(1))
        .unwrap()
        .filter_map(|x| x.ok())
        .collect();
    assert!(cols.contains(&"raw_json".to_string()), "{:?}", cols);
    assert!(cols.contains(&"course_id".to_string()), "{:?}", cols);

    save_meta(&c, "k", "v").unwrap();
    assert_eq!(get_meta(&c, "k").unwrap().as_deref(), Some("v"));
}

#[test]
fn db_path_per_account() {
    assert_eq!(
        archive_path(Path::new("."), "").file_name().unwrap(),
        "dump.db"
    );
    assert_eq!(
        archive_path(Path::new("."), "abc123").file_name().unwrap(),
        "dump-abc123.db"
    );
    assert!(archive_path(Path::new("."), "abc123").starts_with(ARCHIVE_DIR));
}

#[test]
fn db_path_uses_configured_data_directory() {
    let root = Path::new("/tmp/unipus-sdk-data");
    assert_eq!(
        archive_path(root, "abc123"),
        root.join("dump_text/dump-abc123.db")
    );
    assert_eq!(archive_path(root, ""), root.join("dump_text/dump.db"));
}

#[test]
fn account_id_cannot_escape_the_configured_data_directory() {
    let root = Path::new("/tmp/unipus-sdk-data");
    let path = archive_path(root, "../../outside");
    assert!(path.starts_with(root));
    assert_eq!(
        path,
        root.join("dump_text/dump-%2E%2E%2F%2E%2E%2Foutside.db")
    );
}

#[test]
fn two_db_files_isolated() {
    let dir = std::env::temp_dir().join(format!("unipus_db_iso_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let p1 = dir.join("acct-a.db");
    let p2 = dir.join("acct-b.db");
    for p in [&p1, &p2] {
        let _ = std::fs::remove_file(p);
        let _ = std::fs::remove_file(p.with_extension("db-wal"));
        let _ = std::fs::remove_file(p.with_extension("db-shm"));
    }
    let mut c1 = open_path(&p1).unwrap();
    let group = ParsedGroup {
        modules: vec![module("basic")],
    };
    save_task(&mut c1, &input(&group, &[], &[])).unwrap();
    let c2 = open_path(&p2).unwrap();
    assert_eq!(task_count(&c1, "course-x").unwrap(), 1);
    assert_eq!(
        task_count(&c2, "course-x").unwrap(),
        0,
        "双文件必须互相隔离"
    );
    assert!(load_tree(&c2, "course-x").unwrap().is_empty());
    drop(c1);
    drop(c2);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn task_identity_isolated_by_course_and_unit() {
    let mut c = conn();
    let group = ParsedGroup {
        modules: vec![module("basic")],
    };
    save_task(&mut c, &input(&group, &[], &[])).unwrap();
    let mut other = input(&group, &[], &[]);
    other.key.course_id = "course-y";
    other.passed = true;
    save_task(&mut c, &other).unwrap();
    assert_eq!(task_count(&c, "course-x").unwrap(), 1);
    assert_eq!(task_count(&c, "course-y").unwrap(), 1);
    assert_eq!(
        task_status(&c, TaskKey::new("course-x", "u1", "g1")).unwrap(),
        Some(TaskStatus {
            required: true,
            passed: false,
            needs_refresh: false
        })
    );
    assert_eq!(
        task_status(&c, TaskKey::new("course-y", "u1", "g1")).unwrap(),
        Some(TaskStatus {
            required: true,
            passed: true,
            needs_refresh: false
        })
    );
    other.key = TaskKey::new("course-x", "u2", "g1");
    save_task(&mut c, &other).unwrap();
    let first = load_stored(&c, TaskKey::new("course-x", "u1", "g1"))
        .unwrap()
        .unwrap();
    let second = load_stored(&c, TaskKey::new("course-x", "u2", "g1"))
        .unwrap()
        .unwrap();
    assert!(!first.passed);
    assert!(second.passed);
    assert!(
        load_stored(&c, TaskKey::new("course-x", "missing-unit", "g1"))
            .unwrap()
            .is_none()
    );
}

#[test]
fn migrates_legacy_task_identity() {
    let c = Connection::open_in_memory().unwrap();
    c.execute_batch(
            "CREATE TABLE tasks (
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
                updated_at TEXT NOT NULL,
                UNIQUE(unit_id, group_id)
            );
            INSERT INTO tasks VALUES (1, 1, 'u1', NULL, 'g1', 'task', 'basic', 'task', 1, 0, NULL, 'now');",
        )
        .unwrap();
    c.execute_batch(
        "CREATE TABLE modules (
                id INTEGER PRIMARY KEY,
                task_id INTEGER NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
                idx INTEGER NOT NULL,
                module_type TEXT NOT NULL,
                reply_type TEXT NOT NULL,
                instance_id TEXT NOT NULL,
                direction TEXT NOT NULL,
                material TEXT NOT NULL,
                transcript TEXT NOT NULL,
                word_bank_json TEXT,
                UNIQUE(task_id, idx)
            );
            INSERT INTO modules VALUES (1, 1, 0, 'basic', 'singlechoice', 'm', '', '', '', NULL);",
    )
    .unwrap();
    init(&c).unwrap();
    let mut stmt = c.prepare("PRAGMA index_list(tasks)").unwrap();
    let indexes: Vec<(String, bool)> = stmt
        .query_map([], |r| Ok((r.get(1)?, r.get::<_, i64>(2)? != 0)))
        .unwrap()
        .filter_map(Result::ok)
        .collect();
    assert!(indexes.iter().any(|(_, unique)| *unique));
    c.execute(
        "INSERT INTO tasks (unit_index, unit_id, group_id, tab_type, group_type, kind,
                                required, passed, updated_at, course_id)
             VALUES (1, 'u1', 'g1', 'task', 'basic', 'task', 1, 0, 'now', 'course-y')",
        [],
    )
    .unwrap();
    assert_eq!(task_count(&c, "").unwrap(), 0);
    assert_eq!(task_count(&c, "course-y").unwrap(), 1);
    c.execute("DELETE FROM tasks WHERE id=1", []).unwrap();
    let module_count: i64 = c
        .query_row("SELECT COUNT(*) FROM modules", [], |r| r.get(0))
        .unwrap();
    assert_eq!(module_count, 0);
}

#[test]
fn load_tree_filters_by_course() {
    let mut c = conn();
    let group = ParsedGroup {
        modules: vec![module("basic")],
    };
    save_task(&mut c, &input(&group, &[], &[])).unwrap();
    let mut t2 = input(&group, &[], &[]);
    t2.key.group_id = "g2";
    t2.key.course_id = "course-y";
    save_task(&mut c, &t2).unwrap();

    let units = load_tree(&c, "course-x").unwrap();
    assert_eq!(units.len(), 1);
    assert_eq!(units[0].tasks.len(), 1);
    assert_eq!(units[0].tasks[0].group_id, "g1");

    let units = load_tree(&c, "course-y").unwrap();
    assert_eq!(units.len(), 1);
    assert_eq!(units[0].tasks[0].group_id, "g2");

    assert!(load_tree(&c, "course-z").unwrap().is_empty());
    assert_eq!(task_count(&c, "course-z").unwrap(), 0);
}

#[test]
fn meta_roundtrip() {
    let c = conn();
    assert!(get_meta(&c, "course_name").unwrap().is_none());
    save_meta(&c, "course_name", "新视野").unwrap();
    save_meta(&c, "course_name", "新视野2").unwrap();
    assert_eq!(
        get_meta(&c, "course_name").unwrap().as_deref(),
        Some("新视野2")
    );
}

#[test]
fn load_tree_orders_and_rebuilds() {
    let mut c = conn();
    let group = ParsedGroup {
        modules: vec![module("basic")],
    };
    let mut t1 = input(&group, &[], &[]);
    t1.unit_index = 2;
    t1.key.unit_id = "u2";
    t1.unit_label = None;
    t1.key.group_id = "g2";
    save_task(&mut c, &t1).unwrap();

    let mut t0 = input(&group, &[], &[]);
    t0.unit_index = 1;
    t0.unit_label = None;
    t0.key.group_id = "g0";
    save_task(&mut c, &t0).unwrap();
    let mut t2 = input(&group, &[], &[]);
    t2.unit_index = 1;
    t2.passed = true;
    save_task(&mut c, &t2).unwrap();
    let mut t3 = input(&group, &[], &[]);
    t3.unit_index = 1;
    t3.key.group_id = "g1b";
    t3.required = false;
    save_task(&mut c, &t3).unwrap();

    let units = load_tree(&c, "course-x").unwrap();
    assert_eq!(units.len(), 2);
    assert_eq!(units[0].unit_id, "u1");
    assert_eq!(units[0].unit_label.as_deref(), Some("Unit 1"));
    assert_eq!(units[0].tasks.len(), 3);
    assert_eq!(units[0].tasks[0].group_id, "g0");
    assert_eq!(units[0].tasks[1].group_id, "g1");
    assert!(units[0].tasks[1].passed);
    assert_eq!(units[0].tasks[2].group_id, "g1b");
    assert!(!units[0].tasks[2].required);
    assert_eq!(units[1].unit_id, "u2");
    assert_eq!(units[1].unit_label, None);
    assert_eq!(units[1].tasks.len(), 1);
}

#[test]
fn load_stored_rebuilds_group() {
    let mut c = conn();
    let group = ParsedGroup {
        modules: vec![module("vocabulary")],
    };
    let vocab = vec![VocabWord {
        name: "apple".into(),
        sound: "http://s/a.mp3".into(),
    }];
    let media = vec![MediaEntry {
        module_idx: 0,
        url_idx: 0,
        url: "http://a/1.mp3".into(),
        text: Some("t".into()),
        error: None,
    }];
    save_task(&mut c, &input(&group, &vocab, &media)).unwrap();

    let st = load_stored(&c, TaskKey::new("course-x", "u1", "g1"))
        .unwrap()
        .unwrap();
    assert_eq!(st.group_id, "g1");
    assert_eq!(st.kind, "task");
    assert_eq!(st.raw_json.as_deref(), Some("{\n  \"demo\": true\n}"));
    assert_eq!(st.modules.len(), 1);
    let m = &st.modules[0];
    assert_eq!(m.module_type, "vocabulary");
    assert_eq!(m.media_urls, vec!["http://a/1.mp3".to_string()]);
    assert_eq!(m.questions.len(), 1);
    let opts: serde_json::Value = serde_json::from_str(&m.questions[0].options_json).unwrap();
    assert_eq!(opts.as_array().unwrap().len(), 2);
    assert_eq!(opts[0]["name"], "A");
    assert_eq!(
        st.vocab,
        vec![("apple".to_string(), "http://s/a.mp3".to_string())]
    );

    assert!(
        load_stored(&c, TaskKey::new("course-x", "u1", "missing"))
            .unwrap()
            .is_none()
    );
}
