use super::{Session, fetch_home_courses, fetch_unit};
use anyhow::{Context, Result};

pub(crate) async fn unit_label(session: &Session, unit_id: &str) -> Result<Option<String>> {
    let scope = session.cache_scope(&session.course_id());
    if let Some(label) = session.cached_unit_label(&scope, unit_id) {
        return Ok(Some(label));
    }
    let progress = fetch_unit(session, unit_id).await?;
    let group_id = progress
        .leafs
        .iter()
        .find(|(_, leaf)| leaf.tab_type == "task")
        .map(|(id, _)| id)
        .or_else(|| progress.leafs.keys().next());
    let Some(group_id) = group_id else {
        return Ok(None);
    };
    let document = crate::api::content::fetch_document(session, group_id).await?;
    let json = document.json.context("单元内容不是 JSON，无法提取名称")?;
    let label = crate::api::parser::extract_group_label(&json);
    if label.is_empty() {
        return Ok(None);
    }
    session.cache_unit_label(&scope, unit_id, &label);
    Ok(Some(label))
}

pub async fn course_display_name(session: &Session, course_id: &str) -> String {
    let scope = session.cache_scope(course_id);
    if let Some(name) = session.cached_course_name(&scope) {
        return name;
    }
    let name = match course_display_name_lookup(session, course_id).await {
        Some(n) if !n.is_empty() => n,
        _ => course_display_name_fallback(course_id),
    };
    session.cache_course_name(&scope, &name);
    name
}

async fn course_display_name_lookup(session: &Session, course_id: &str) -> Option<String> {
    match fetch_home_courses(session).await {
        Ok(courses) => courses
            .into_iter()
            .find(|course| course.course_id == course_id)
            .map(|course| course.name)
            .filter(|name| !name.is_empty()),
        Err(e) => {
            log::warn!("获取课程名称失败: {:#}", e);
            None
        }
    }
}

pub(super) fn course_display_name_fallback(course_id: &str) -> String {
    let code = course_id.split('+').nth(1).unwrap_or(course_id);

    let (base, book) = match code.rfind('_') {
        Some(i)
            if !code[i + 1..].is_empty() && code[i + 1..].chars().all(|c| c.is_ascii_digit()) =>
        {
            (&code[..i], &code[i + 1..])
        }
        _ => (code, ""),
    };
    let mut name = String::new();
    let mut parts = base.split('_');
    let (course, version, kind) = (
        parts.next().unwrap_or_default(),
        parts.next().unwrap_or_default(),
        parts.next().unwrap_or_default(),
    );
    match course {
        "nhce" => name.push_str("新视野大学英语"),
        _ => name.push_str(&format!("未知书本({})", course)),
    }
    if let Some(version) = version.strip_prefix("v") {
        name.push_str(&format!("(第{}版)", version));
    } else {
        name.push_str(&format!("未知版本({})", version));
    }
    match kind {
        "rw" => name.push_str("读写教程"),
        "vls" => name.push_str("视听说教程"),
        _ => name.push_str(&format!("未知课程({})", kind)),
    };
    if book.is_empty() {
        name.to_string()
    } else {
        format!("{} {}", name, book)
    }
}
