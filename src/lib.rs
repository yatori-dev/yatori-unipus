#![doc = include_str!("../README.md")]

mod api;
pub mod archive;
mod client;
pub mod config;
pub mod course;
mod execution;
mod llm;
pub mod model;
mod reporter;
mod serde_util;
mod solver;
mod text;
mod time_util;
mod transcribe;

pub use archive::{Archive, ArchiveOptions, ArchiveSummary, Preview, TaskKey};
pub use client::Client;
pub use config::{
    AuthConfig, Config, CourseConfig, ExecutionConfig, LlmConfig, TranscriptionConfig,
};
pub use course::{CourseProgress, GroupTask, HomeCourse, TabType, UnitProgress};
pub use execution::{
    RunOptions, RunSummary,
    planner::{Plan, UnitPlan},
};
pub use reporter::{ArchiveEventKind, FnReporter, ReportEvent, Reporter};
pub use tokio_util::sync::CancellationToken;
