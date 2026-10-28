use crate::api::session::{Session, submit_url};
use crate::model::Module;
use anyhow::{Context, Result};
use serde::Serialize;
use serde_json::{Value, json};

const SUBMITTED_CONTEXT: &str = r#"{"state":"submitted"}"#;

#[derive(Debug)]
pub struct RateLimited {
    pub code: String,
    pub msg: String,
}

impl std::fmt::Display for RateLimited {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "限频提交被拒 (code={} msg={})", self.code, self.msg)
    }
}

impl std::error::Error for RateLimited {}

pub fn is_rate_limited(v: &Value) -> bool {
    let msg = v
        .get("msg")
        .and_then(|m| m.as_str())
        .unwrap_or("")
        .to_string();
    let msg_limited = msg.contains("操作过于频繁");
    let code_limited = v
        .get("code")
        .and_then(|c| c.as_i64())
        .is_some_and(|c| matches!(c, 600001 | 600002));
    msg_limited || code_limited
}

#[derive(Debug, Clone)]
pub struct ChildAnswer {
    pub question_type: String,
    pub reply_type: String,
    pub value: String,
}

#[derive(Debug, Clone)]
pub struct ModuleAnswer {
    pub instance_id: String,
    pub children: Vec<ChildAnswer>,
}

fn build_payload_json(
    sessions: &Session,
    group_id: &str,
    ques_datas: Vec<SubmitQues>,
    is_completed: Vec<bool>,
    third_party_judges: String,
    submit_type: u32,
    hide_loading: bool,
) -> Result<String> {
    let obj = json!({
        "quesDatas": ques_datas,
        "groupId": group_id,
        "isCompleted": is_completed,
        "thirdPartyJudges": third_party_judges,
        "submitType": submit_type,
        "hideLoading": hide_loading,
        "associationGroupId": "",
        "courseId": sessions.course_id(),
        "openId": sessions.open_id(),
        "version": "default",
    });
    serde_json::to_string(&obj).context("序列化提交载荷失败")
}

#[derive(Debug, Clone, Serialize)]
struct SubmitQues {
    #[serde(rename = "instanceId")]
    instance_id: String,
    answer: String,
    context: String,
    #[serde(rename = "contextVersion")]
    context_version: u32,
    #[serde(rename = "answerVersion")]
    answer_version: u32,
}

fn versions(session: &Session) -> Value {
    let course = session
        .cfg()
        .course
        .publish_version
        .parse::<i64>()
        .unwrap_or(1);
    json!({
        "course": course,
        "group": 1,
        "template": 1,
        "answer": 3,
        "content": 0,
    })
}

fn build_answer_json(children: &[ChildAnswer]) -> String {
    let children_json: Vec<Value> = children
        .iter()
        .map(|c| {
            let v: Value = if c.value.is_empty() {
                json!([])
            } else if c.reply_type == "multichoice" {
                let parts: Vec<&str> = c.value.split(',').map(|s| s.trim()).collect();
                json!(parts)
            } else {
                json!([c.value])
            };
            json!({ "value": v, "isDone": !c.value.is_empty() })
        })
        .collect();
    json!({
        "value": [],
        "children": children_json,
        "progress": {},
        "record": {"url": ""}
    })
    .to_string()
}

fn make_judges(session: &Session, modules: &[ModuleAnswer]) -> String {
    let versions = versions(session);
    let mut arr: Vec<Value> = Vec::new();
    for m in modules {
        for c in &m.children {
            arr.push(json!({
                "value": c.value,
                "question_type": c.question_type,
                "reply_type": c.reply_type,
                "versions": versions,
                "payloads": [],
            }));
        }
    }
    Value::Array(arr).to_string()
}

pub fn build_answer_payload(
    session: &Session,
    group_id: &str,
    modules: &[ModuleAnswer],
) -> Result<String> {
    let questions_datas: Vec<SubmitQues> = modules
        .iter()
        .map(|m| SubmitQues {
            instance_id: m.instance_id.clone(),
            answer: build_answer_json(&m.children),
            context: SUBMITTED_CONTEXT.to_string(),
            context_version: 1,
            answer_version: 1,
        })
        .collect();

    let is_completed: Vec<bool> = modules
        .iter()
        .flat_map(|m| m.children.iter().map(|c| !c.value.is_empty()))
        .collect();

    let judges = make_judges(session, modules);

    build_payload_json(
        session,
        group_id,
        questions_datas,
        is_completed,
        judges,
        1,
        false,
    )
}

pub fn build_mark_seen_payload(session: &Session, group_id: &str) -> Result<String> {
    build_payload_json(
        session,
        group_id,
        Vec::new(),
        Vec::new(),
        "[]".to_string(),
        2,
        true,
    )
}

impl From<&Module> for ModuleAnswer {
    fn from(module: &Module) -> Self {
        Self {
            instance_id: module.instance_id.clone(),
            children: module
                .children
                .iter()
                .map(|child| ChildAnswer {
                    question_type: child.question_type.clone(),
                    reply_type: child.reply_type.clone(),
                    value: String::new(),
                })
                .collect(),
        }
    }
}

pub async fn submit_raw(session: &Session, payload: &str) -> Result<Value> {
    let (status, body) = session.post_raw(submit_url(), payload).await?;
    if !status.is_success() {
        anyhow::bail!("submit HTTP {}", status);
    }
    let v: Value = serde_json::from_str(&body)?;
    let code = v.get("code").and_then(|c| c.as_i64());
    if code != Some(0) {
        let code_s = v.get("code").map(|c| c.to_string()).unwrap_or_default();
        let msg = v.get("msg").and_then(|m| m.as_str()).unwrap_or("");
        let message = v.get("message").and_then(|m| m.as_str()).unwrap_or("");
        if is_rate_limited(&v) {
            return Err(RateLimited {
                code: code_s,
                msg: msg.to_string(),
            }
            .into());
        }
        anyhow::bail!("submit code={} msg={} message={}", code_s, msg, message);
    }
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn submission_envelope_preserves_platform_field_names_and_versions() {
        let mut config = crate::config::Config::default();
        config.auth.open_id = "student".into();
        config.course.course_id = "course".into();
        config.course.publish_version = "123".into();
        let session = Session::from_config(config).unwrap();
        let modules = vec![ModuleAnswer {
            instance_id: "module-1".into(),
            children: vec![ChildAnswer {
                question_type: "basic".into(),
                reply_type: "singlechoice".into(),
                value: "B".into(),
            }],
        }];
        let payload: Value =
            serde_json::from_str(&build_answer_payload(&session, "group", &modules).unwrap())
                .unwrap();
        assert_eq!(payload["courseId"], "course");
        assert_eq!(payload["openId"], "student");
        assert_eq!(payload["groupId"], "group");
        assert_eq!(payload["submitType"], 1);
        assert_eq!(payload["isCompleted"], serde_json::json!([true]));
        let question = &payload["quesDatas"][0];
        assert_eq!(question["instanceId"], "module-1");
        assert_eq!(question["contextVersion"], 1);
        assert_eq!(question["answerVersion"], 1);
        assert_eq!(question["context"], r#"{"state":"submitted"}"#);
        assert!(question.get("instance_id").is_none());
        let judges: Value =
            serde_json::from_str(payload["thirdPartyJudges"].as_str().unwrap()).unwrap();
        assert_eq!(judges[0]["versions"]["course"], 123);
        assert_eq!(judges[0]["value"], "B");

        let seen: Value =
            serde_json::from_str(&build_mark_seen_payload(&session, "group").unwrap()).unwrap();
        assert_eq!(seen["submitType"], 2);
        assert_eq!(seen["hideLoading"], true);
        assert_eq!(seen["quesDatas"], serde_json::json!([]));
    }

    #[test]
    fn multichoice_answer_split() {
        let c = ChildAnswer {
            question_type: "basic".into(),
            reply_type: "multichoice".into(),
            value: "A,B,C".into(),
        };
        let s = build_answer_json(&[c]);
        let v: Value = serde_json::from_str(&s).unwrap();
        assert_eq!(
            v["children"][0]["value"],
            serde_json::json!(["A", "B", "C"])
        );
    }

    #[test]
    fn singlechoice_answer() {
        let c = ChildAnswer {
            question_type: "basic".into(),
            reply_type: "singlechoice".into(),
            value: "B".into(),
        };
        let s = build_answer_json(&[c]);
        let v: Value = serde_json::from_str(&s).unwrap();
        assert_eq!(v["children"][0]["value"], serde_json::json!(["B"]));
    }
}
