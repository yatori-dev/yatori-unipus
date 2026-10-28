use crate::api::{bbs, session::Session};
use crate::model::{Module, ParsedGroup};
use crate::solver::Solver;
use anyhow::{Context, Result, bail};
use log::info;
use tokio_util::sync::CancellationToken;

pub(super) async fn post_discussion_comments(
    session: &Session,
    solver: &Solver,
    group_id: &str,
    group: &ParsedGroup,
    cancel: &CancellationToken,
) -> Result<()> {
    for m in group.modules.iter().filter(|m| m.is_discussion()) {
        super::ensure_running(cancel)?;
        let values = solver.solve(m).await?;
        super::ensure_running(cancel)?;
        let comment = values
            .into_iter()
            .next()
            .unwrap_or_default()
            .trim()
            .to_string();
        if comment.is_empty() {
            bail!("讨论题 {} 生成内容为空，无法发帖", m.instance_id);
        }
        let title = discussion_title(m);
        let topic_id = ensure_topic(session, group_id, &title, &comment, cancel).await?;
        if bbs::has_own_reply(session, topic_id).await? {
            info!(
                "讨论组 {} 主题 {} 已有本人回复，跳过发帖",
                group_id, topic_id
            );
            continue;
        }
        super::ensure_running(cancel)?;
        bbs::post_reply(session, topic_id, &comment).await?;
        info!("讨论组 {} 已发表评论 (topic {})", group_id, topic_id);
    }
    Ok(())
}

async fn ensure_topic(
    session: &Session,
    group_id: &str,
    title: &str,
    content: &str,
    cancel: &CancellationToken,
) -> Result<i64> {
    let pick = |topics: &[bbs::Topic]| {
        topics
            .iter()
            .find(|topic| topic.owner_status)
            .or_else(|| topics.first())
            .map(|topic| topic.topic_id)
    };
    let topics = bbs::fetch_topics(session, group_id).await?;
    if let Some(id) = pick(&topics) {
        return Ok(id);
    }
    super::ensure_running(cancel)?;
    bbs::create_topic(session, group_id, title, content).await?;
    let topics = bbs::fetch_topics(session, group_id).await?;
    pick(&topics).with_context(|| {
        format!("讨论组 {group_id} 创建主题后仍未查询到 topicId，请检查 class_id/curricula_id 配置")
    })
}

fn discussion_title(m: &Module) -> String {
    let src = if !m.direction.is_empty() {
        m.direction.as_str()
    } else {
        m.material.as_str()
    };
    let line = src
        .lines()
        .map(|l| l.trim())
        .find(|l| !l.is_empty())
        .unwrap_or("Discussion");
    crate::text::truncate(line, 60)
}
