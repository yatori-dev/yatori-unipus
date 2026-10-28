use super::{ArchiveStats, TaskCounts, store};
use crate::model::ParsedGroup;
use std::fmt::Write;
use std::path::Path;

pub(super) fn summary_text(stats: &ArchiveStats, path: &Path) -> Option<String> {
    if stats.total_tasks == 0 {
        return None;
    }
    let mut text = format!(
        "归档数据汇总 (生成时间: {})\n任务组: {} 个 | 必修: {} (已完成 {}, 未完成 {}) | 选修: {} (已完成 {}, 未完成 {})\n数据库: {}\n\n按单元:\n",
        store::now_local_string(),
        stats.total_tasks,
        stats.required.total,
        stats.required.passed,
        stats.required.total - stats.required.passed,
        stats.optional.total,
        stats.optional.passed,
        stats.optional.total - stats.optional.passed,
        path.display(),
    );
    for unit in &stats.units {
        let mut required = TaskCounts::default();
        let mut optional = TaskCounts::default();
        for task in &unit.tasks {
            let counts = if task.required {
                &mut required
            } else {
                &mut optional
            };
            counts.total += 1;
            counts.passed += usize::from(task.passed);
        }
        let _ = writeln!(
            text,
            "  {:02}_{}: 必修 {} (已完成 {}) / 选修 {} (已完成 {})",
            unit.unit_index,
            unit.unit_id,
            required.total,
            required.passed,
            optional.total,
            optional.passed,
        );
    }
    text.push_str("\n任务清单:\n");
    for unit in &stats.units {
        for task in &unit.tasks {
            let _ = writeln!(
                text,
                "  {:02}_{}/{}/{} [{} {}]",
                unit.unit_index,
                unit.unit_id,
                task.group_type,
                task.group_id,
                if task.required { "必修" } else { "选修" },
                if task.passed {
                    "已完成"
                } else {
                    "未完成"
                },
            );
        }
    }
    Some(text)
}

pub(super) fn group_type_name(group: &ParsedGroup) -> String {
    let mut names: Vec<String> = Vec::new();
    for m in &group.modules {
        let raw = if m.reply_type.is_empty() {
            m.module_type.as_str()
        } else {
            m.reply_type.as_str()
        };
        let name = sanitize_dir_name(raw);
        if !name.is_empty() && !names.contains(&name) {
            names.push(name);
        }
    }
    if names.is_empty() {
        "unknown".to_string()
    } else {
        names.join("+")
    }
}

fn sanitize_dir_name(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            _ => c,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Module;

    fn module(module_type: &str, reply_type: &str) -> Module {
        Module {
            instance_id: "m".into(),
            module_type: module_type.into(),
            direction: String::new(),
            material: String::new(),
            media_sources: Vec::new(),
            transcript: String::new(),
            reply_type: reply_type.into(),
            word_bank: Vec::new(),
            children: Vec::new(),
        }
    }

    fn group(modules: Vec<Module>) -> ParsedGroup {
        ParsedGroup { modules }
    }

    #[test]
    fn type_name_prefers_reply_type() {
        assert_eq!(
            group_type_name(&group(vec![module("basic", "singlechoice")])),
            "singlechoice"
        );
        assert_eq!(
            group_type_name(&group(vec![module("material-banked-cloze", "bankedcloze")])),
            "bankedcloze"
        );
    }

    #[test]
    fn type_name_falls_back_to_module_type() {
        assert_eq!(
            group_type_name(&group(vec![module("vocabulary", "")])),
            "vocabulary"
        );
        assert_eq!(
            group_type_name(&group(vec![module("video-popup", "")])),
            "video-popup"
        );
    }

    #[test]
    fn type_name_dedupes_and_joins() {
        assert_eq!(
            group_type_name(&group(vec![
                module("basic", "bankedcloze"),
                module("basic", "bankedcloze"),
            ])),
            "bankedcloze"
        );
        assert_eq!(
            group_type_name(&group(vec![
                module("basic", "bankedcloze"),
                module("basic", "singlechoice"),
            ])),
            "bankedcloze+singlechoice"
        );
        assert_eq!(group_type_name(&group(vec![module("", "")])), "unknown");
    }

    #[test]
    fn dir_name_sanitized() {
        assert_eq!(sanitize_dir_name("a/b:c*d?"), "a_b_c_d_");
        assert_eq!(sanitize_dir_name("bankedcloze"), "bankedcloze");
    }
}
