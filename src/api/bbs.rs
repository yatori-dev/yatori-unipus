use crate::api::session::Session;
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};

const UCLOUD: &str = "https://ucloud.unipus.cn";

fn topic_page_url() -> String {
    format!("{}/api/bbs/utopic/page", UCLOUD)
}

fn topic_add_url() -> String {
    format!("{}/api/bbs/utopic/add", UCLOUD)
}

fn reply_page_url() -> String {
    format!("{}/api/bbs/ureply/top/page", UCLOUD)
}

fn reply_add_url() -> String {
    format!("{}/api/bbs/ureply/add", UCLOUD)
}

#[derive(Debug, Clone)]
pub struct Topic {
    pub topic_id: i64,
    pub owner_status: bool,
}

fn bbs_scope(session: &Session) -> Result<(String, String)> {
    let cfg = session.cfg().course;
    if cfg.class_id.is_empty() {
        bail!("讨论题需要配置 class_id（页面 URL 的 cid），当前为空");
    }
    if cfg.curricula_id.is_empty() {
        bail!("讨论题需要配置 curricula_id（页面 URL 的 cloudCurriculaId），当前为空");
    }
    Ok((cfg.class_id.clone(), cfg.curricula_id.clone()))
}

fn parse_bbs_response(body: &str, allow_duplicate: bool) -> Result<Value> {
    let v: Value = serde_json::from_str(body)
        .with_context(|| format!("BBS 响应不是 JSON: {}", crate::text::truncate(body, 200)))?;
    let success = v.get("success").and_then(|s| s.as_bool()).unwrap_or(false);
    let code = v.get("code").and_then(|c| c.as_i64());
    if success || code == Some(1) {
        return Ok(v);
    }
    if allow_duplicate && matches!(code, Some(10001 | 10002)) {
        return Ok(v);
    }
    let msg = v.get("msg").and_then(|m| m.as_str()).unwrap_or("");
    let message = v.get("message").and_then(|m| m.as_str()).unwrap_or("");
    if code.is_none() && !success && msg.is_empty() && message.is_empty() {
        bail!("BBS 接口返回异常: {}", crate::text::truncate(body, 200));
    }
    bail!(
        "BBS 接口错误 code={:?} msg={} message={}",
        code,
        msg,
        message
    )
}

async fn post_bbs(
    session: &Session,
    url: &str,
    payload: &Value,
    allow_duplicate: bool,
) -> Result<Value> {
    let body = serde_json::to_string(payload).context("序列化 BBS 请求失败")?;
    if session.jwt().is_empty() {
        session.refresh_auth().await?;
    }
    let generation = session.identity_generation();
    let jwt = session.jwt();
    if jwt.is_empty() {
        bail!("无可用 JWT：请向宿主配置 username/password 或有效 refresh_token 后登录");
    }
    let (mut status, mut text) = session.post_raw_with_auth(url, &body, Some(&jwt)).await?;
    if status == reqwest::StatusCode::UNAUTHORIZED {
        anyhow::ensure!(
            session.identity_generation() == generation,
            "账号已切换，未重试旧账号讨论请求"
        );
        if session.refresh_rejected_auth(&jwt).await? {
            anyhow::ensure!(
                session.identity_generation() == generation,
                "账号已切换，未重试旧账号讨论请求"
            );
            let current = session.jwt();
            (status, text) = session
                .post_raw_with_auth(url, &body, Some(&current))
                .await?;
        }
    }
    if status.is_success() {
        return parse_bbs_response(&text, allow_duplicate);
    }
    if status == reqwest::StatusCode::UNAUTHORIZED {
        bail!(
            "BBS 接口 401：JWT 过期或无权限，请更新账号凭证后登录。响应: {}",
            crate::text::truncate(&text, 150)
        );
    }
    bail!(
        "BBS 接口 HTTP {}: {}",
        status,
        crate::text::truncate(&text, 200)
    )
}

pub fn topics_from_value(v: &Value) -> Vec<Topic> {
    v.pointer("/value/utopics")
        .and_then(|t| t.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|t| {
                    let topic_id = t.get("topicId").and_then(|x| x.as_i64())?;
                    Some(Topic {
                        topic_id,
                        owner_status: t
                            .get("ownerStatus")
                            .and_then(|x| x.as_bool())
                            .unwrap_or(false),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

pub fn has_own_reply_in(v: &Value) -> bool {
    v.pointer("/value/replyContents")
        .and_then(|r| r.as_array())
        .map(|arr| {
            arr.iter().any(|r| {
                r.get("ownerStatus")
                    .and_then(|x| x.as_bool())
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false)
}

pub async fn fetch_topics(session: &Session, group_id: &str) -> Result<Vec<Topic>> {
    let (class_id, curricula_id) = bbs_scope(session)?;
    let payload = json!({
        "order": "desc",
        "pageNum": 1,
        "pageSize": 10,
        "courseId": session.course_id(),
        "groupId": group_id,
        "type": 2,
        "classId": class_id,
        "curriculaId": curricula_id,
    });
    let v = post_bbs(session, &topic_page_url(), &payload, false).await?;
    Ok(topics_from_value(&v))
}

pub async fn create_topic(
    session: &Session,
    group_id: &str,
    title: &str,
    content: &str,
) -> Result<()> {
    let (class_id, curricula_id) = bbs_scope(session)?;
    let payload = json!({
        "title": title,
        "content": content,
        "courseId": session.course_id(),
        "groupId": group_id,
        "type": 2,
        "classId": class_id,
        "curriculaId": curricula_id,
    });
    post_bbs(session, &topic_add_url(), &payload, true)
        .await
        .map(|_| ())
        .with_context(|| format!("创建讨论主题失败 (groupId={})", group_id))
}

pub async fn has_own_reply(session: &Session, topic_id: i64) -> Result<bool> {
    let payload = json!({
        "topicId": topic_id,
        "parentId": "",
        "order": "desc",
        "pageNum": 1,
        "pageSize": 10,
    });
    let v = post_bbs(session, &reply_page_url(), &payload, false).await?;
    Ok(has_own_reply_in(&v))
}

pub async fn post_reply(session: &Session, topic_id: i64, content: &str) -> Result<()> {
    let payload = json!({
        "type": 2,
        "content": content,
        "topicId": topic_id,
        "parentId": "",
        "topReplyId": "",
        "contentType": "text",
    });
    post_bbs(session, &reply_add_url(), &payload, true)
        .await
        .map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn topics_parse_with_results() {
        let json = r#"{"code":1,"msg":"SUCCESS","value":{"count":1,"utopics":[{"topicId":10190684,"title":"t","content":"c","ownerStatus":true,"replyCount":0}]},"success":true}"#;
        let v: Value = serde_json::from_str(json).unwrap();
        let topics = topics_from_value(&v);
        assert_eq!(topics.len(), 1);
        assert_eq!(topics[0].topic_id, 10190684);
        assert!(topics[0].owner_status);
    }

    #[test]
    fn topics_parse_empty_null() {
        let json =
            r#"{"code":1,"msg":"SUCCESS","value":{"count":0,"utopics":null},"success":true}"#;
        let v: Value = serde_json::from_str(json).unwrap();
        assert!(topics_from_value(&v).is_empty());
    }

    #[test]
    fn reply_owner_status() {
        let json = r#"{"code":1,"msg":"SUCCESS","value":{"count":1,"replyTotalCount":1,"replyContents":[{"id":41572409,"content":"1","ownerStatus":true}],"uaiBbs":true},"success":true}"#;
        let v: Value = serde_json::from_str(json).unwrap();
        assert!(has_own_reply_in(&v));
        let empty = r#"{"code":1,"msg":"SUCCESS","value":{"count":0,"replyTotalCount":0,"replyContents":[],"uaiBbs":true},"success":true}"#;
        let v2: Value = serde_json::from_str(empty).unwrap();
        assert!(!has_own_reply_in(&v2));
    }

    #[test]
    fn error_response_rejected() {
        let json = r#"{"code":100,"msg":"Ai版课程Id不能为空","value":null,"success":false}"#;
        let err = parse_bbs_response(json, false).unwrap_err().to_string();
        assert!(err.contains("Ai版课程Id不能为空"));

        let dup = r#"{"code":10002,"msg":"重复提交","success":false}"#;
        assert!(parse_bbs_response(dup, true).is_ok());
        assert!(parse_bbs_response(dup, false).is_err());
    }
}
