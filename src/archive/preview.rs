use super::{ArchivedModule, ArchivedTask};
use crate::api::content::fetch_document;
use crate::api::parser::extract_vocabulary;
use crate::api::session::Session;
use crate::model::{ChildQ, Module, OptionItem, ParsedGroup, VocabWord};
use anyhow::Result;

#[derive(Debug, Clone, serde::Serialize)]
pub struct Preview {
    pub json_pretty: Option<String>,

    pub plain: String,

    pub group: Option<ParsedGroup>,

    pub vocab: Vec<VocabWord>,
}

pub(crate) async fn load_preview(session: &Session, group_id: &str) -> Result<Preview> {
    let document = fetch_document(session, group_id).await?;
    let vocab = document
        .json
        .as_ref()
        .map(extract_vocabulary)
        .unwrap_or_default();
    Ok(Preview {
        json_pretty: document
            .json
            .as_ref()
            .map(serde_json::to_string_pretty)
            .transpose()?,
        plain: document.plain,
        group: document.group,
        vocab,
    })
}

pub(super) fn from_stored(st: ArchivedTask) -> Preview {
    let group = (st.kind == "task").then(|| rebuild_group(&st));
    let vocab = st
        .vocab
        .into_iter()
        .map(|(name, sound)| VocabWord { name, sound })
        .collect();
    Preview {
        json_pretty: st.raw_json,
        plain: st.raw_content.unwrap_or_default(),
        group,
        vocab,
    }
}

fn parse_options(json: &str) -> Vec<OptionItem> {
    serde_json::from_str(json).unwrap_or_default()
}

fn rebuild_group(st: &ArchivedTask) -> ParsedGroup {
    ParsedGroup {
        modules: st.modules.iter().map(rebuild_module).collect(),
    }
}

fn rebuild_module(m: &ArchivedModule) -> Module {
    Module {
        instance_id: m.instance_id.clone(),
        module_type: m.module_type.clone(),
        direction: m.direction.clone(),
        material: m.material.clone(),
        media_sources: m.media_urls.clone(),
        transcript: m.transcript.clone(),
        reply_type: m.reply_type.clone(),
        word_bank: parse_options(&m.word_bank_json),
        children: m
            .questions
            .iter()
            .map(|q| {
                let options = parse_options(&q.options_json);
                ChildQ {
                    question_type: q.question_type.clone(),
                    reply_type: q.reply_type.clone(),
                    question_text: q.question_text.clone(),
                    option_count: options.len(),
                    options,
                }
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::archive::store::{self, MediaEntry, TaskInput, TaskKey};

    #[test]
    fn rebuild_from_stored_task() {
        let mut conn = rusqlite::Connection::open_in_memory().unwrap();
        store::init(&conn).unwrap();
        let module = Module {
            instance_id: "i1".into(),
            module_type: "basic".into(),
            direction: "说明".into(),
            material: "材料".into(),
            media_sources: vec!["http://a/1.mp3".into()],
            transcript: "字幕".into(),
            reply_type: "singlechoice".into(),
            word_bank: vec![OptionItem {
                name: "A".into(),
                value: "a".into(),
                text: "选项A".into(),
            }],
            children: vec![ChildQ {
                question_type: "basic".into(),
                reply_type: "singlechoice".into(),
                question_text: "题干".into(),
                options: vec![
                    OptionItem {
                        name: "A".into(),
                        value: "a".into(),
                        text: "选项A".into(),
                    },
                    OptionItem {
                        name: "B".into(),
                        value: "b".into(),
                        text: "选项B".into(),
                    },
                ],
                option_count: 2,
            }],
        };
        let group = ParsedGroup {
            modules: vec![module],
        };
        let media = vec![MediaEntry {
            module_idx: 0,
            url_idx: 0,
            url: "http://a/1.mp3".into(),
            text: None,
            error: Some("x".into()),
        }];
        store::save_task(
            &mut conn,
            &TaskInput {
                unit_index: 1,
                key: TaskKey::new("course-x", "u1", "g1"),
                unit_label: None,
                tab_type: "task",
                group_type: "singlechoice",
                kind: "task",
                required: true,
                passed: false,
                raw_content: None,
                raw_json: Some("{}"),
                group: Some(&group),
                vocab: &[],
                media: &media,
            },
        )
        .unwrap();

        let st = store::load_stored(&conn, TaskKey::new("course-x", "u1", "g1"))
            .unwrap()
            .unwrap();
        let rebuilt = rebuild_group(&st);
        assert_eq!(rebuilt.modules.len(), 1);
        let m = &rebuilt.modules[0];
        assert_eq!(m.direction, "说明");
        assert_eq!(m.media_sources, vec!["http://a/1.mp3".to_string()]);
        assert_eq!(m.word_bank.len(), 1);
        assert_eq!(m.children.len(), 1);
        assert_eq!(m.children[0].option_count, 2);
        assert_eq!(m.children[0].options[1].text, "选项B");
    }
}
