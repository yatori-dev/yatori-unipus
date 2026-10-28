//! 课程任务规划与执行。平台协议位于 api，执行策略位于本模块。
mod discussion;
pub mod planner;
mod submission;
mod task;

use crate::api::course::{build_tasks, fetch_course_progress, fetch_unit, select_tasks};
use crate::api::session::Session;
use crate::reporter::{ReportEvent, Reporter};
use anyhow::Result;
use log::info;
use serde::Serialize;
use task::process_group;
use tokio_util::sync::CancellationToken;

fn ensure_running(cancel: &CancellationToken) -> Result<()> {
    anyhow::ensure!(!cancel.is_cancelled(), "已取消");
    Ok(())
}

pub(crate) async fn run_course(
    session: &mut Session,
    with_names: bool,
    reporter: &dyn Reporter,
    cancel: &CancellationToken,
) -> Result<RunSummary> {
    let course = fetch_course_progress(session).await?;
    let version = course.publish_version.clone();
    if !version.is_empty() {
        session.set_publish_version(&version)?;
    }
    let units: Vec<String> = course.units.into_keys().collect();
    run_course_units(session, &units, with_names, reporter, cancel).await
}

pub(crate) async fn run_course_units(
    session: &mut Session,
    unit_ids: &[String],
    with_names: bool,
    reporter: &dyn Reporter,
    cancel: &CancellationToken,
) -> Result<RunSummary> {
    let compulsory_only = session.cfg().execution.compulsory_only();
    let mut summary = RunSummary::default();
    if with_names {
        info!("课程: {}", {
            let course_id = session.course_id();
            crate::api::course::course_display_name(session, &course_id).await
        });
    }
    'units: for (ui, unit_id) in unit_ids.iter().enumerate() {
        if cancel.is_cancelled() {
            break;
        }
        let rt = fetch_unit(session, unit_id).await?;
        let tasks = select_tasks(&build_tasks(unit_id, &rt), compulsory_only);
        let label = if with_names {
            Some(
                crate::api::course::unit_label(session, unit_id)
                    .await?
                    .unwrap_or_else(|| format!("Unit {}", ui + 1)),
            )
        } else {
            None
        };
        reporter.report(ReportEvent::UnitStart {
            unit_id: unit_id.clone(),
            label,
            total: tasks.len(),
            required_only: compulsory_only,
        });
        for task in &tasks {
            if cancel.is_cancelled() {
                break 'units;
            }
            if task.passed {
                summary.skipped += 1;

                crate::archive::sync_task_status(session, task, true);
                reporter.report(ReportEvent::TaskSkipped {
                    group_id: task.group_id.clone(),
                    reason: "已通过".to_string(),
                });
                continue;
            }
            match process_group(session, task, reporter, cancel).await {
                Ok(_) => {
                    summary.submitted += 1;
                }
                Err(e) => {
                    if cancel.is_cancelled() {
                        break 'units;
                    }
                    summary.failed += 1;
                    reporter.report(ReportEvent::TaskFailed {
                        group_id: task.group_id.clone(),
                        tab_type: task.tab_type.clone(),
                        error: format!("{:#}", e),
                    });
                }
            }
            tokio::select! {
                _ = cancel.cancelled() => break 'units,
                _ = tokio::time::sleep(std::time::Duration::from_millis(session.cfg().execution.interval_ms)) => {}
            }
        }
    }

    reporter.report(ReportEvent::RunFinished {
        submitted: summary.submitted,
        skipped: summary.skipped,
        failed: summary.failed,
    });
    Ok(summary)
}

/// 选择待运行单元；空 unit_ids 表示当前课程的全部单元。
#[derive(Debug, Clone, Default)]
pub struct RunOptions {
    pub unit_ids: Vec<String>,
    pub with_names: bool,
}

/// submitted 仅统计平台接受的请求，不代表远端成绩已通过。
#[derive(Debug, Default, Serialize)]
pub struct RunSummary {
    pub skipped: u32,
    pub submitted: u32,
    pub failed: u32,
}
