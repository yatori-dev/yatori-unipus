use super::{discussion::post_discussion_comments, submission::submit_with_rate_retry};
use crate::api::submit::{ModuleAnswer, build_answer_payload, build_mark_seen_payload};
use crate::api::{content::fetch_document, session::Session};
use crate::course::{GroupTask, TabType};
use crate::reporter::{ReportEvent, Reporter};
use crate::solver::Solver;
use crate::transcribe::Transcriber;
use anyhow::{Result, bail};
use tokio_util::sync::CancellationToken;

pub(super) async fn process_group(
    session: &Session,
    task: &GroupTask,
    reporter: &dyn Reporter,
    cancel: &CancellationToken,
) -> Result<()> {
    super::ensure_running(cancel)?;
    reporter.report(ReportEvent::TaskStart {
        group_id: task.group_id.clone(),
        tab_type: task.tab_type.clone(),
    });
    process_group_inner(session, task, reporter, cancel).await?;

    reporter.report(ReportEvent::TaskSubmitted {
        group_id: task.group_id.clone(),
        tab_type: task.tab_type.clone(),
    });
    Ok(())
}

async fn process_group_inner(
    session: &Session,
    task: &GroupTask,
    reporter: &dyn Reporter,
    cancel: &CancellationToken,
) -> Result<()> {
    let config = session.cfg();
    let http = session.http_client();
    let solver = Solver::new(
        http.clone(),
        config.llm,
        Transcriber::new(http, config.transcription, config.data_dir),
    );
    match TabType::parse(&task.tab_type) {
        TabType::Text | TabType::Video => {
            if let Some(group) = fetch_document(session, &task.group_id)
                .await
                .ok()
                .and_then(|document| document.group)
            {
                post_discussion_comments(session, &solver, &task.group_id, &group, cancel).await?;
            }
            let payload = build_mark_seen_payload(session, &task.group_id)?;
            submit_with_rate_retry(session, &payload, reporter, cancel).await
        }
        TabType::Task => {
            let document = fetch_document(session, &task.group_id).await?;
            let group = match document.group {
                Some(group) => group,
                None => {
                    log::warn!(
                        "任务组 {} 内容为空/非题目模块，按“浏览即完成”标记已看",
                        task.group_id
                    );
                    let payload = build_mark_seen_payload(session, &task.group_id)?;
                    return submit_with_rate_retry(session, &payload, reporter, cancel).await;
                }
            };

            post_discussion_comments(session, &solver, &task.group_id, &group, cancel).await?;
            let mut modules = Vec::new();
            for module in group
                .modules
                .iter()
                .filter(|module| !module.is_discussion())
            {
                super::ensure_running(cancel)?;
                let values = solver.solve(module).await?;
                let mut answer = ModuleAnswer::from(module);
                for (child, value) in answer.children.iter_mut().zip(values) {
                    child.value = value;
                }
                modules.push(answer);
            }

            if !group.has_answerable_modules() {
                let payload = build_mark_seen_payload(session, &task.group_id)?;
                return submit_with_rate_retry(session, &payload, reporter, cancel).await;
            }

            let payload = build_answer_payload(session, &task.group_id, &modules)?;
            submit_with_rate_retry(session, &payload, reporter, cancel).await
        }
        TabType::Unknown(other) => bail!("未知 tab_type: {}", other),
    }
}
