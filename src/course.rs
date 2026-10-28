//! Course identity, progress and task models returned by the SDK.
use crate::serde_util::{
    null_default, tolerant_i64, tolerant_opt_i32, tolerant_opt_i64, tolerant_string, tolerant_u8,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CourseProgress {
    #[serde(default)]
    pub units: BTreeMap<String, CourseUnitEntry>,
    #[serde(default, deserialize_with = "tolerant_string")]
    pub publish_version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CourseUnitEntry {
    #[serde(default)]
    pub strategies: Option<Strategies>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnitProgress {
    #[serde(default, deserialize_with = "tolerant_i64")]
    pub duration_time: i64,
    #[serde(default, deserialize_with = "tolerant_string")]
    pub flag: String,
    #[serde(default)]
    pub leafs: BTreeMap<String, LeafEntry>,
    #[serde(default)]
    pub micros: BTreeMap<String, MicroEntry>,
    #[serde(default, deserialize_with = "tolerant_string")]
    pub open_id: String,
    #[serde(default, deserialize_with = "tolerant_string")]
    pub publish_version: String,
    #[serde(default, rename = "tutorialId", deserialize_with = "tolerant_string")]
    pub tutorial_id: String,
    #[serde(default, deserialize_with = "tolerant_string")]
    pub unit_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LeafEntry {
    #[serde(default, deserialize_with = "tolerant_i64")]
    pub duration: i64,
    #[serde(default)]
    pub state: Option<LeafState>,
    #[serde(default)]
    pub strategies: Option<Strategies>,
    #[serde(default, deserialize_with = "tolerant_string")]
    pub tab_type: String,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct LeafState {
    #[serde(default, deserialize_with = "tolerant_u8")]
    pub pass: u8,
    #[serde(default, deserialize_with = "tolerant_u8")]
    pub pass2: u8,
    #[serde(default, deserialize_with = "tolerant_u8")]
    pub perm: u8,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Strategies {
    #[serde(default, deserialize_with = "tolerant_opt_i64")]
    pub end_time: Option<i64>,
    #[serde(default, deserialize_with = "tolerant_opt_i32")]
    pub min_score_pct: Option<i32>,
    #[serde(default, deserialize_with = "null_default")]
    pub required: bool,
    #[serde(default, deserialize_with = "tolerant_opt_i64")]
    pub start_time: Option<i64>,
    #[serde(default, deserialize_with = "null_default")]
    pub statistic_mode_out: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MicroEntry {
    #[serde(default)]
    pub state: Option<LeafState>,
    #[serde(default)]
    pub strategies: Option<Strategies>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GroupTask {
    pub group_id: String,
    pub unit_id: String,
    pub tab_type: String,
    pub required: bool,
    pub passed: bool,
    pub min_score_pct: i32,
    pub start_time: i64,
    pub end_time: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TabType {
    Task,
    Text,
    Video,
    Unknown(String),
}

impl TabType {
    pub fn parse(value: &str) -> Self {
        match value {
            "task" => Self::Task,
            "text" => Self::Text,
            "video" => Self::Video,
            other => Self::Unknown(other.to_string()),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct HomeCourse {
    pub name: String,
    pub course_id: String,

    pub class_id: String,

    pub curricula_id: String,

    pub group_label: String,
}
