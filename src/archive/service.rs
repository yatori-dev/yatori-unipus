use super::TaskKey;
use super::format::group_type_name;
use super::rebuild::Rebuild;
use super::store::{self, MediaEntry, TaskInput, TaskStatus};
use crate::api::content::{ContentDocument, fetch_document};
use crate::api::course::{
    GroupTask, build_tasks, course_display_name, fetch_course_units, fetch_unit,
};
use crate::api::parser::{extract_group_label, extract_vocabulary};
use crate::api::session::Session;
use crate::model::ParsedGroup;
use crate::reporter::{ArchiveEventKind, ReportEvent, Reporter};
use crate::transcribe::Transcriber;
use anyhow::{Context, Result};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct ArchiveOptions {
    pub force: bool,
    pub with_names: bool,
    pub unit_ids: Vec<String>,
    pub group_ids: Vec<String>,
}

#[derive(Debug, Default, Clone, Serialize)]
pub struct ArchiveSummary {
    pub units: usize,
    pub generated: usize,
    pub skipped: usize,
    pub updated: usize,
    pub total_tasks: usize,
    pub modules: usize,
    pub questions: usize,
    pub media: usize,
    pub media_chars: usize,
}

pub(crate) async fn archive_course(
    session: &Session,
    options: &ArchiveOptions,
    reporter: &dyn Reporter,
    cancel: &CancellationToken,
) -> Result<ArchiveSummary> {
    if session.open_id().trim().is_empty() {
        anyhow::bail!("open_id 不能为空，无法隔离账号归档");
    }
    let course_id = session.course_id();
    let target = store::archive_path(&session.data_dir(), &session.open_id());
    let full_rebuild = options.force && options.unit_ids.is_empty() && options.group_ids.is_empty();
    // 先创建守卫，保证出错退出时先关闭 SQLite，再清理临时文件。
    let rebuild = full_rebuild.then(|| Rebuild::new(&target));
    let working_path = rebuild.as_ref().map_or(target.as_path(), Rebuild::path);
    let connection = store::open_path(working_path)?;
    let config = session.cfg();
    let mut job = ArchiveJob {
        session,
        options,
        reporter,
        cancel,
        course_id,
        target,
        connection,
        transcriber: Transcriber::new(session.http_client(), config.transcription, config.data_dir),
        full_rebuild,
        summary: ArchiveSummary::default(),
    };
    job.write_metadata().await?;
    job.write_units().await?;
    job.summary.total_tasks = store::task_count(&job.connection, &job.course_id)?;
    if rebuild.is_some() && cancel.is_cancelled() {
        anyhow::bail!("全量重建已取消，保留原归档");
    }
    let ArchiveJob {
        connection,
        summary,
        course_id,
        ..
    } = job;
    // 关闭连接后再移动守卫，关闭失败时也保持此析构顺序。
    connection
        .close()
        .map_err(|(_, error)| error)
        .context("关闭归档连接失败")?;
    if let Some(rebuild) = rebuild {
        rebuild.commit(&course_id)?;
    }
    Ok(summary)
}

struct ArchiveJob<'a> {
    session: &'a Session,
    options: &'a ArchiveOptions,
    reporter: &'a dyn Reporter,
    cancel: &'a CancellationToken,
    course_id: String,
    target: PathBuf,
    connection: Connection,
    transcriber: Transcriber,
    full_rebuild: bool,
    summary: ArchiveSummary,
}

impl ArchiveJob<'_> {
    async fn write_metadata(&mut self) -> Result<()> {
        if self.options.group_ids.is_empty() || self.options.with_names {
            let name = course_display_name(self.session, &self.course_id).await;
            if self.options.group_ids.is_empty() {
                store::save_meta(&self.connection, "course_id", &self.course_id)?;
                store::save_meta(&self.connection, "course_name", &name)?;
                store::save_meta(
                    &self.connection,
                    &format!("course_name:{}", self.course_id),
                    &name,
                )?;
            }
            if self.options.with_names {
                self.reporter.report(ReportEvent::CourseInfo {
                    course_id: self.course_id.clone(),
                    name,
                });
            }
        }
        Ok(())
    }

    async fn write_units(&mut self) -> Result<()> {
        let all_units = fetch_course_units(self.session).await?;
        let units = if self.options.unit_ids.is_empty() {
            all_units.clone()
        } else {
            self.options.unit_ids.clone()
        };
        self.summary.units = units.len();
        for (index, unit_id) in units.iter().enumerate() {
            if self.cancel.is_cancelled() {
                break;
            }
            let unit_index = all_units
                .iter()
                .position(|unit| unit == unit_id)
                .unwrap_or(index)
                + 1;
            self.write_unit(unit_id, unit_index).await?;
        }
        Ok(())
    }

    async fn write_unit(&mut self, unit_id: &str, unit_index: usize) -> Result<()> {
        let progress = fetch_unit(self.session, unit_id).await?;
        let mut label = String::new();
        let mut announced = false;
        for task in build_tasks(unit_id, &progress) {
            if self.cancel.is_cancelled() {
                break;
            }
            if !self.options.group_ids.is_empty()
                && !self.options.group_ids.contains(&task.group_id)
            {
                continue;
            }
            if self.refresh_existing(&task, unit_index, &label)? {
                continue;
            }
            let document = match fetch_document(self.session, &task.group_id).await {
                Ok(document) => document,
                Err(error) if self.full_rebuild => {
                    return Err(error)
                        .with_context(|| format!("全量重建任务 {} 获取失败", task.group_id));
                }
                Err(_) => continue,
            };
            if label.is_empty()
                && document.group.is_some()
                && let Some(json) = &document.json
            {
                label = extract_group_label(json);
            }
            if self.options.with_names && !announced {
                self.reporter.report(ReportEvent::UnitInfo {
                    unit_id: unit_id.to_string(),
                    label: if label.is_empty() {
                        format!("Unit {unit_index}")
                    } else {
                        label.clone()
                    },
                });
                announced = true;
            }
            self.save_document(&task, unit_index, &label, document)
                .await?;
        }
        Ok(())
    }

    fn refresh_existing(
        &mut self,
        task: &GroupTask,
        unit_index: usize,
        label: &str,
    ) -> Result<bool> {
        if self.options.force || !self.options.group_ids.is_empty() {
            return Ok(false);
        }
        let key = TaskKey::new(&self.course_id, &task.unit_id, &task.group_id);
        let Some(old) = store::task_status(&self.connection, key)? else {
            return Ok(false);
        };
        if old.needs_refresh {
            return Ok(false);
        }
        self.summary.skipped += 1;
        if old.required != task.required || old.passed != task.passed {
            let status = TaskStatus {
                required: task.required,
                passed: task.passed,
                needs_refresh: false,
            };
            if store::update_status(
                &self.connection,
                key,
                unit_index,
                nonempty(label),
                &task.tab_type,
                status,
            )? {
                self.summary.updated += 1;
                self.report(task, ArchiveEventKind::Refreshed, None);
            }
        }
        Ok(true)
    }

    async fn save_document(
        &mut self,
        task: &GroupTask,
        unit_index: usize,
        label: &str,
        document: ContentDocument,
    ) -> Result<()> {
        let parsed = document.json.as_ref().zip(document.group.as_ref());
        let mut media = Vec::new();
        let mut vocab = Vec::new();
        let mut raw_json = None;
        let mut group_type = "view-only".to_string();
        let mut group = None;
        if let Some((json, parsed_group)) = parsed {
            media = self.transcribe(parsed_group).await;
            vocab = extract_vocabulary(json);
            raw_json = Some(serde_json::to_string_pretty(json)?);
            group_type = group_type_name(parsed_group);
            group = Some(parsed_group);
        }
        store::save_task(
            &mut self.connection,
            &TaskInput {
                key: TaskKey::new(&self.course_id, &task.unit_id, &task.group_id),
                unit_index,
                unit_label: nonempty(label),
                tab_type: &task.tab_type,
                group_type: &group_type,
                kind: if group.is_some() { "task" } else { "view-only" },
                required: task.required,
                passed: task.passed,
                raw_content: if group.is_none() {
                    nonempty(document.plain.trim())
                } else {
                    None
                },
                raw_json: raw_json.as_deref(),
                group,
                vocab: &vocab,
                media: &media,
            },
        )?;
        self.summary.generated += 1;
        self.report(
            task,
            if group.is_some() {
                ArchiveEventKind::Generated
            } else {
                ArchiveEventKind::ViewOnly
            },
            group,
        );
        Ok(())
    }

    async fn transcribe(&mut self, group: &ParsedGroup) -> Vec<MediaEntry> {
        let mut media = Vec::new();
        for (module_idx, module) in group.modules.iter().enumerate() {
            self.summary.modules += 1;
            self.summary.questions += module.children.len();
            for (url_idx, url) in module.media_sources.iter().enumerate() {
                self.summary.media += 1;
                let (text, error) = match self.transcriber.transcribe(url).await {
                    Ok(text) => {
                        self.summary.media_chars += text.chars().count();
                        (Some(text), None)
                    }
                    Err(error) => (None, Some(format!("{error:#}"))),
                };
                media.push(MediaEntry {
                    module_idx,
                    url_idx,
                    url: url.clone(),
                    text,
                    error,
                });
            }
        }
        media
    }

    fn report(&self, task: &GroupTask, kind: ArchiveEventKind, group: Option<&ParsedGroup>) {
        self.reporter.report(ReportEvent::Archive {
            unit_id: task.unit_id.clone(),
            group_id: task.group_id.clone(),
            kind,
            path: self.target.display().to_string(),
            modules: group.map_or(0, |group| group.modules.len()),
            questions: group.map_or(0, ParsedGroup::question_count),
            media: group.map_or(0, |group| {
                group
                    .modules
                    .iter()
                    .map(|module| module.media_sources.len())
                    .sum()
            }),
        });
    }
}

fn nonempty(value: &str) -> Option<&str> {
    (!value.is_empty()).then_some(value)
}
