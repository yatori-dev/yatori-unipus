use super::annotator_token_ok;
use crate::api::{course, session::Session};
use anyhow::Result;

/// Fill remote identity fields without replacing unrelated settings or refreshed credentials.
pub(crate) async fn ensure_profile(session: &Session) -> Result<bool> {
    let mut changed = false;
    if !session.open_id().is_empty()
        && !annotator_token_ok(&session.cfg().auth.x_annotator_auth_token)
    {
        changed = session.refresh_annotator().await?;
    }
    let generation = session.identity_generation();
    let baseline = session.cfg();
    let mut profile = baseline.clone();
    if !profile.course.course_id.is_empty() {
        match course::fetch_home_courses(session).await {
            Ok(list) => match list
                .iter()
                .find(|course| course.course_id == profile.course.course_id)
            {
                Some(course) => {
                    if !course.class_id.is_empty() {
                        profile.course.class_id = course.class_id.clone();
                    }
                    if !course.curricula_id.is_empty() {
                        profile.course.curricula_id = course.curricula_id.clone();
                    }
                }
                None => log::warn!(
                    "课程列表未找到当前 course_id（{}），保留现有课程身份",
                    profile.course.course_id
                ),
            },
            Err(error) => log::warn!("刷新课程信息失败（保留原值）: {error:#}"),
        }
    }
    match course::fetch_user_school(session).await {
        Ok(Some(school)) => profile.auth.u_school = school,
        Ok(None) => {}
        Err(error) => log::warn!("刷新学校编号失败（保留原值）: {error:#}"),
    }
    if !profile.course.course_id.is_empty() {
        match course::fetch_course_progress(session).await {
            Ok(progress) if !progress.publish_version.is_empty() => {
                profile.course.publish_version = progress.publish_version
            }
            Ok(_) => {}
            Err(error) => log::warn!("刷新 publish_version 失败（保留原值）: {error:#}"),
        }
    }
    changed |= session.merge_profile(&baseline, &profile, generation)?;
    Ok(changed)
}
