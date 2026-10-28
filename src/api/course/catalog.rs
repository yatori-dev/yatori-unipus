use super::Session;
use crate::config::CourseConfig;
use crate::course::HomeCourse;
use anyhow::Result;

const HOME_COURSE_LIST_URL: &str =
    "https://uai.unipus.cn/api/cmgt/course/getHomeCourseListByStudent";

pub fn apply_course_selection(cfg: &mut CourseConfig, course: &HomeCourse) -> bool {
    let had_curricula_id = !cfg.curricula_id.is_empty();
    cfg.course_id = course.course_id.clone();
    cfg.class_id = course.class_id.clone();
    cfg.curricula_id = course.curricula_id.clone();
    had_curricula_id && cfg.curricula_id.is_empty()
}

pub(super) fn parse_home_courses(v: &serde_json::Value) -> Vec<HomeCourse> {
    let mut out = Vec::new();
    let Some(courses) = v.pointer("/value/courseList").and_then(|c| c.as_array()) else {
        return out;
    };
    for c in courses {
        let class_id = c
            .get("classId")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();
        let curricula_id = c
            .get("id")
            .map(|x| match x {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            })
            .unwrap_or_default();
        let fallback_name = c
            .get("name")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();
        let Some(res_list) = c.get("courseResourceList").and_then(|r| r.as_array()) else {
            continue;
        };
        for res in res_list {
            let course_id = res
                .get("instanceId")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            if course_id.is_empty() {
                continue;
            }
            let name = res
                .get("name")
                .and_then(|x| x.as_str())
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| fallback_name.clone());
            out.push(HomeCourse {
                name,
                course_id,
                class_id: class_id.clone(),
                curricula_id: curricula_id.clone(),
                group_label: String::new(),
            });
        }
    }
    out
}

pub async fn fetch_home_courses(session: &Session) -> Result<Vec<HomeCourse>> {
    let body = session
        .get_bytes(HOME_COURSE_LIST_URL)
        .await
        .map(|b| String::from_utf8_lossy(&b).into_owned())
        .map_err(|e| anyhow::anyhow!("获取课程列表失败: {:#}", e))?;
    let v: serde_json::Value =
        serde_json::from_str(&body).map_err(|e| anyhow::anyhow!("课程列表响应非 JSON: {:#}", e))?;
    let ok = v.get("code").and_then(|c| c.as_i64()) == Some(1)
        || v.get("success").and_then(|s| s.as_bool()) == Some(true);
    if !ok {
        anyhow::bail!(
            "课程列表接口返回异常: {}",
            crate::text::truncate(&body, 200)
        );
    }
    log::debug!("课程列表原始响应: {}", crate::text::truncate(&body, 4000));
    let list = parse_home_courses(&v);
    if !list.is_empty() {
        return Ok(list);
    }

    let bookshelf = fetch_bookshelf_courses(session).await?;
    if bookshelf.is_empty() {
        log::warn!("首页课程列表与我的教材均为空（账号可能未加入班级/激活教材）");
    } else {
        log::info!(
            "首页课程列表为空，改用我的教材列表（{} 门）",
            bookshelf.len()
        );
    }
    Ok(bookshelf)
}

const BOOKSHELF_URL: &str = "https://uai.unipus.cn/api/cmgt/course/my/bookshelf";

async fn fetch_bookshelf_courses(session: &Session) -> Result<Vec<HomeCourse>> {
    let body = session
        .get_bytes(BOOKSHELF_URL)
        .await
        .map(|b| String::from_utf8_lossy(&b).into_owned())
        .map_err(|e| anyhow::anyhow!("获取我的教材失败: {:#}", e))?;
    let v: serde_json::Value =
        serde_json::from_str(&body).map_err(|e| anyhow::anyhow!("我的教材响应非 JSON: {:#}", e))?;
    let ok = v.get("code").and_then(|c| c.as_i64()) == Some(1)
        || v.get("success").and_then(|s| s.as_bool()) == Some(true);
    if !ok {
        anyhow::bail!(
            "我的教材接口返回异常: {}",
            crate::text::truncate(&body, 200)
        );
    }
    log::debug!("我的教材原始响应: {}", crate::text::truncate(&body, 4000));
    Ok(parse_bookshelf_courses(&v))
}

pub(super) fn parse_bookshelf_courses(v: &serde_json::Value) -> Vec<HomeCourse> {
    let mut out: Vec<HomeCourse> = Vec::new();
    let Some(groups) = v.pointer("/value/groups").and_then(|g| g.as_object()) else {
        return out;
    };

    for (key, label) in [("CLASS_COURSE", "班级课程"), ("PERSONAL", "个人学习")] {
        let Some(items) = groups.get(key).and_then(|x| x.as_array()) else {
            continue;
        };
        for item in items {
            let course_id = item
                .get("instanceId")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            if course_id.is_empty() {
                continue;
            }
            let name = item
                .get("name")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();

            let (class_id, curricula_id) = item
                .get("classCourses")
                .and_then(|cc| match cc {
                    serde_json::Value::Array(arr) => arr.first().cloned(),
                    other => Some(other.clone()),
                })
                .map(|cc| {
                    (
                        cc.get("classId")
                            .map(|x| match x {
                                serde_json::Value::String(s) => s.clone(),
                                other => other.to_string(),
                            })
                            .unwrap_or_default(),
                        cc.get("courseId")
                            .map(|x| match x {
                                serde_json::Value::String(s) => s.clone(),
                                other => other.to_string(),
                            })
                            .unwrap_or_default(),
                    )
                })
                .unwrap_or_default();
            if out.iter().any(|c| c.course_id == course_id) {
                continue;
            }
            out.push(HomeCourse {
                name,
                course_id,
                class_id,
                curricula_id,
                group_label: label.to_string(),
            });
        }
    }
    out
}

const ACCOUNT_USER_INFO_URL: &str = "https://uai.unipus.cn/api/account/user/info";

pub async fn fetch_user_school(session: &Session) -> Result<Option<String>> {
    let body = session
        .get_bytes(ACCOUNT_USER_INFO_URL)
        .await
        .map(|b| String::from_utf8_lossy(&b).into_owned())
        .map_err(|e| anyhow::anyhow!("获取账号信息失败: {:#}", e))?;
    let v: serde_json::Value =
        serde_json::from_str(&body).map_err(|e| anyhow::anyhow!("账号信息响应非 JSON: {:#}", e))?;
    let ok = v.get("code").and_then(|c| c.as_i64()) == Some(1)
        || v.get("success").and_then(|s| s.as_bool()) == Some(true);
    if !ok {
        anyhow::bail!(
            "账号信息接口返回异常: {}",
            crate::text::truncate(&body, 200)
        );
    }
    Ok(v.pointer("/value/userInfo/school")
        .map(|x| match x {
            serde_json::Value::String(s) => s.clone(),
            other => other.to_string(),
        })
        .filter(|s| !s.is_empty()))
}
