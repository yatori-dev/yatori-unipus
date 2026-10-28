use crate::model::{ChildQ, Module, OptionItem, ParsedGroup, VocabWord};
use crate::text::truncate;
use anyhow::Result;
use serde_json::Value;

fn module_values(value: &Value) -> std::slice::Iter<'_, Value> {
    value
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_else(|| std::slice::from_ref(value))
        .iter()
}

fn module_content(module: &Value) -> Option<Value> {
    match module.get("content")? {
        Value::String(text) => serde_json::from_str(text).ok(),
        Value::Object(_) => module.get("content").cloned(),
        _ => None,
    }
}

fn parse_options(value: Option<&Value>) -> Vec<OptionItem> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|option| OptionItem {
            name: get_str(option, &["name", "value"]).unwrap_or_default(),
            value: get_str(option, &["value", "name"]).unwrap_or_default(),
            text: get_str(option, &["text"])
                .map(|text| strip_html(&text))
                .unwrap_or_default(),
        })
        .collect()
}

fn get_str(v: &Value, keys: &[&str]) -> Option<String> {
    for k in keys {
        if let Some(s) = v.get(k).and_then(|x| x.as_str()) {
            return Some(s.to_string());
        }
    }
    None
}

pub fn strip_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for ch in s.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out.lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn strip_html_short(s: &str) -> String {
    let cleaned = strip_html(s);
    let mut lines: Vec<&str> = cleaned.lines().collect();
    lines.truncate(30);
    lines.join("\n")
}

fn direction_text(v: &Value) -> String {
    let dir = v.get("direction");
    if let Some(d) = dir {
        if let Some(s) = d.get("text").and_then(|x| x.as_str()) {
            return strip_html(s);
        }
        if let Some(s) = d.get("pcText").and_then(|x| x.as_str()) {
            return strip_html(s);
        }
    }
    String::new()
}

fn is_media_url(p: &str) -> bool {
    let lower = p.to_ascii_lowercase();
    lower.contains(".mp3")
        || lower.contains(".mp4")
        || lower.contains(".m4a")
        || lower.contains(".wav")
        || lower.contains(".aac")
        || lower.contains(".ogg")
        || lower.contains(".flac")
        || lower.contains("audio/")
        || lower.contains("video/")
}

pub fn clean_url(p: &str) -> String {
    p.split('#').next().unwrap_or(p).trim().to_string()
}

fn collect_material(content: &Value) -> (String, Vec<String>, String) {
    let mut parts: Vec<String> = Vec::new();
    let mut media: Vec<String> = Vec::new();
    let mut transcript: Vec<String> = Vec::new();
    if let Some(arr) = content.get("contents").and_then(|c| c.as_array()) {
        for item in arr {
            let text = item.get("text").and_then(|x| x.as_str()).unwrap_or("");
            let clean = strip_html(text);
            if !clean.is_empty() {
                if text.trim_start().starts_with("WEBVTT") {
                    transcript.push(clean);
                } else {
                    parts.push(clean);
                }
            }
            if let Some(p) = item.get("path").and_then(|x| x.as_str())
                && is_media_url(p)
            {
                media.push(clean_url(p));
            }

            if let Some(subs) = item.get("subtitles").and_then(|s| s.as_array()) {
                for s in subs {
                    if let Some(p) = s.get("path").and_then(|x| x.as_str())
                        && !p.is_empty()
                    {
                        media.push(clean_url(p));
                    }
                }
            }
        }
    }

    let mut seen = std::collections::HashSet::new();
    media.retain(|m| seen.insert(m.clone()));
    (parts.join("\n\n"), media, transcript.join("\n\n"))
}

pub fn parse_group(decrypted: &Value) -> Result<ParsedGroup> {
    let mut out = ParsedGroup::default();

    for module in module_values(decrypted) {
        let instance_id = get_str(module, &["id"])
            .or_else(|| {
                module
                    .get("id")
                    .and_then(|x| x.as_i64())
                    .map(|x| x.to_string())
            })
            .unwrap_or_default();
        if instance_id.is_empty() {
            continue;
        }
        let Some(content) = module_content(module) else {
            continue;
        };
        let module_type = get_str(&content, &["type"]).unwrap_or_default();
        let reply_type = get_str(&content, &["replyType"]).unwrap_or_default();
        let direction = direction_text(&content);
        let (material, media_sources, transcript) = collect_material(&content);
        let word_bank = parse_options(content.get("options"));
        let children = content
            .get("children")
            .and_then(|c| c.as_array())
            .cloned()
            .unwrap_or_default();

        let mut child_qs = Vec::new();
        for child in children {
            let question_type = get_str(&child, &["type"]).unwrap_or_else(|| module_type.clone());
            let reply_type =
                get_str(&child, &["replyType"]).unwrap_or_else(|| "text-area".to_string());
            let question_text = get_str(&child, &["quesText", "text"])
                .map(|s| strip_html_short(&s))
                .unwrap_or_default();
            let options = parse_options(child.get("options"));
            let options_len = options.len();
            child_qs.push(ChildQ {
                question_type,
                reply_type,
                question_text,
                options,
                option_count: options_len,
            });
        }

        out.modules.push(Module {
            instance_id,
            module_type,
            direction,
            material,
            media_sources,
            transcript,
            reply_type,
            word_bank,
            children: child_qs,
        });
    }

    if out.modules.is_empty() {
        return Err(anyhow::anyhow!("未解析到任何模块，content 结构未知"));
    }
    Ok(out)
}

pub fn extract_group_label(decrypted: &Value) -> String {
    let mut direction = String::new();
    for module in module_values(decrypted) {
        let Some(content) = module_content(module) else {
            continue;
        };
        if let Some(arr) = content.get("contents").and_then(|c| c.as_array()) {
            for item in arr {
                if let Some(name) = item.get("name").and_then(|n| n.as_str()) {
                    let name = name.trim();
                    if !name.is_empty() && !looks_like_uuid(name) {
                        return strip_media_ext(name).to_string();
                    }
                }
            }
        }
        if direction.is_empty() {
            direction = direction_text(&content);
        }
    }
    if !direction.is_empty() {
        return truncate(&direction, 40);
    }
    String::new()
}

pub fn extract_vocabulary(decrypted: &Value) -> Vec<VocabWord> {
    let mut out = Vec::new();
    for module in module_values(decrypted) {
        let Some(content) = module_content(module) else {
            continue;
        };
        if get_str(&content, &["type"]).as_deref() != Some("vocabulary") {
            continue;
        }
        if let Some(arr) = content.get("contents").and_then(|c| c.as_array()) {
            for item in arr {
                let name = get_str(item, &["name"])
                    .map(|s| strip_html(&s))
                    .unwrap_or_default();
                let sound = get_str(item, &["sound"])
                    .map(|s| clean_url(&s))
                    .unwrap_or_default();
                if !name.is_empty() || !sound.is_empty() {
                    out.push(VocabWord { name, sound });
                }
            }
        }
    }
    out
}

fn looks_like_uuid(s: &str) -> bool {
    s.contains('-') && s.chars().all(|c| c.is_alphanumeric() || c == '-')
}

fn strip_media_ext(s: &str) -> &str {
    for ext in [".mp3", ".mp4", ".m4a", ".wav", ".aac", ".ogg", ".flac"] {
        if let Some(rest) = s.strip_suffix(ext) {
            return rest.trim_end();
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_both_platform_content_encodings() {
        let content = serde_json::json!({
            "type": "basic", "replyType": "singlechoice",
            "direction": {"text": "<p>Choose one.</p>"},
            "options": [{"value": "word", "text": "<b>Word</b>"}],
            "children": [{"quesText": "<p>A question</p>", "options": [{"name": "A", "text": "<b>First</b>"}]}]
        });
        for encoded in [content.clone(), Value::String(content.to_string())] {
            let group = parse_group(&serde_json::json!([{"id": 7, "content": encoded}])).unwrap();
            assert_eq!(group.modules.len(), 1);
            let module = &group.modules[0];
            assert_eq!(module.instance_id, "7");
            assert_eq!(module.direction, "Choose one.");
            assert_eq!(module.word_bank[0].name, "word");
            assert_eq!(module.word_bank[0].text, "Word");
            let question = &module.children[0];
            assert_eq!(question.question_type, "basic");
            assert_eq!(question.reply_type, "text-area");
            assert_eq!(question.question_text, "A question");
            assert_eq!(question.option_count, 1);
            assert_eq!(question.options[0].value, "A");
            assert_eq!(question.options[0].text, "First");
        }
    }

    #[test]
    fn media_url_detect() {
        assert!(is_media_url("https://cdn/x/audio.mp3"));
        assert!(is_media_url("https://cdn/x/a.m4a"));
        assert!(is_media_url("https://cdn/x/v.mp4#duration=10"));
        assert!(!is_media_url("https://cdn/x/a.jpg"));
    }

    #[test]
    fn clean_url_drops_fragment() {
        assert_eq!(
            clean_url("https://cdn/x/v.mp4#duration=100&size=1"),
            "https://cdn/x/v.mp4"
        );
    }

    #[test]
    fn strip_html_no_truncation() {
        let input: String = (0..50).map(|i| format!("line{}\n", i)).collect();
        let out = strip_html(&input);
        assert_eq!(out.lines().count(), 50);
        let short = strip_html_short(&input);
        assert_eq!(short.lines().count(), 30);
    }

    #[test]
    fn group_label_prefers_media_name() {
        let json = r#"[{"id":1,"content":"{\"type\":\"x\",\"contents\":[{\"id\":\"uuid-1\",\"text\":\"q\"},{\"path\":\"http://c/a.mp3\",\"name\":\"U1 Pre-reading activities.mp3\"}]}"}]"#;
        let v: Value = serde_json::from_str(json).unwrap();
        assert_eq!(extract_group_label(&v), "U1 Pre-reading activities");
    }

    #[test]
    fn group_label_falls_back_to_direction() {
        let json = r#"[{"id":1,"content":"{\"type\":\"x\",\"direction\":{\"text\":\"Listen and fill in the blanks.\"},\"contents\":[{\"id\":\"uuid-1\",\"text\":\"q\"}]}"}]"#;
        let v: Value = serde_json::from_str(json).unwrap();
        assert_eq!(extract_group_label(&v), "Listen and fill in the blanks.");
    }

    #[test]
    fn group_label_empty_when_no_hint() {
        let json = r#"[{"id":1,"content":"{\"type\":\"x\",\"contents\":[{\"id\":\"uuid-1\",\"text\":\"q\"}]}"}]"#;
        let v: Value = serde_json::from_str(json).unwrap();
        assert_eq!(extract_group_label(&v), "");
    }

    #[test]
    fn vocabulary_extract() {
        let json = r#"[{"id":1,"content":"{\"type\":\"vocabulary\",\"contents\":[{\"name\":\"<p>abolish</p>\",\"sound\":\"https://cdn/a_96k.mp3#duration=1.8&size=29&name=U4TA-01 abolish.mp3\"},{\"name\":\"<p>degrade</p>\",\"sound\":\"https://cdn/d.mp3\"}],\"children\":[]}"}]"#;
        let v: Value = serde_json::from_str(json).unwrap();
        let words = extract_vocabulary(&v);
        assert_eq!(words.len(), 2);
        assert_eq!(words[0].name, "abolish");
        assert_eq!(words[0].sound, "https://cdn/a_96k.mp3");
        assert_eq!(words[1].name, "degrade");
        assert_eq!(words[1].sound, "https://cdn/d.mp3");
    }

    #[test]
    fn vocabulary_ignores_other_types() {
        let json = r#"[{"id":1,"content":"{\"type\":\"basic\",\"contents\":[{\"name\":\"x\",\"sound\":\"https://cdn/a.mp3\"}]}"}]"#;
        let v: Value = serde_json::from_str(json).unwrap();
        assert!(extract_vocabulary(&v).is_empty());
    }
}
