use crate::model::{ParsedGroup, VocabWord};
use serde::Serialize;

#[derive(Debug, Clone)]
pub(crate) struct MediaEntry {
    pub module_idx: usize,
    pub url_idx: usize,
    pub url: String,
    pub text: Option<String>,
    pub error: Option<String>,
}

pub(crate) struct TaskInput<'a> {
    pub unit_index: usize,
    pub key: TaskKey<'a>,
    pub unit_label: Option<&'a str>,
    pub tab_type: &'a str,
    pub group_type: &'a str,

    pub kind: &'a str,
    pub required: bool,
    pub passed: bool,

    pub raw_content: Option<&'a str>,

    pub raw_json: Option<&'a str>,

    pub group: Option<&'a ParsedGroup>,

    pub vocab: &'a [VocabWord],

    pub media: &'a [MediaEntry],
}

#[derive(Debug, Clone, Serialize)]
pub struct ArchivedUnit {
    pub unit_index: i64,
    pub unit_id: String,
    pub unit_label: Option<String>,
    pub tasks: Vec<ArchivedTaskSummary>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ArchivedTaskSummary {
    pub group_id: String,
    pub tab_type: String,
    pub group_type: String,
    pub kind: String,
    pub required: bool,
    pub passed: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ArchivedQuestion {
    pub reply_type: String,
    pub question_type: String,
    pub question_text: String,
    pub options_json: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ArchivedModule {
    pub module_type: String,
    pub reply_type: String,
    pub instance_id: String,
    pub direction: String,
    pub material: String,
    pub transcript: String,
    pub word_bank_json: String,
    pub media_urls: Vec<String>,
    pub questions: Vec<ArchivedQuestion>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ArchivedTask {
    pub unit_id: String,
    pub unit_index: i64,
    pub unit_label: Option<String>,
    pub group_id: String,
    pub tab_type: String,
    pub group_type: String,
    pub kind: String,
    pub required: bool,
    pub passed: bool,
    pub raw_content: Option<String>,
    pub raw_json: Option<String>,
    pub modules: Vec<ArchivedModule>,

    pub vocab: Vec<(String, String)>,
}

/// 账号归档内的完整任务身份。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct TaskKey<'a> {
    pub course_id: &'a str,
    pub unit_id: &'a str,
    pub group_id: &'a str,
}

impl<'a> TaskKey<'a> {
    pub fn new(course_id: &'a str, unit_id: &'a str, group_id: &'a str) -> Self {
        Self {
            course_id,
            unit_id,
            group_id,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TaskStatus {
    pub required: bool,
    pub passed: bool,
    pub needs_refresh: bool,
}

#[derive(Debug, Clone, Copy, Default, Serialize)]
pub struct TaskCounts {
    pub total: usize,
    pub passed: usize,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ArchiveStats {
    pub total_tasks: usize,
    pub required: TaskCounts,
    pub optional: TaskCounts,
    pub units: Vec<ArchivedUnit>,
}
