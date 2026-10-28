use crate::model::{Module, ReplyType};
use crate::text::truncate;
use crate::transcribe::Transcriber;

pub(super) async fn prepare(
    transcriber: &Transcriber,
    module: &Module,
    reply_type: &ReplyType,
    enabled: bool,
) -> Option<String> {
    if !enabled || matches!(reply_type, ReplyType::Vocabulary) {
        return None;
    }
    if matches!(reply_type, ReplyType::Discussion) {
        return if module.material.is_empty() {
            media_context(transcriber, module).await
        } else {
            None
        };
    }
    if matches!(reply_type, ReplyType::BankedCloze) && super::answers::word_bank(module).is_empty()
    {
        return None;
    }
    match material_context(module) {
        Some(context) => Some(context),
        None => media_context(transcriber, module).await,
    }
}

fn material_context(m: &Module) -> Option<String> {
    let mut parts = Vec::new();
    if !m.material.is_empty() {
        parts.push(truncate(&m.material, 4000));
    }
    if !m.transcript.is_empty() {
        parts.push(format!(
            "【视频/音频字幕】\n{}",
            truncate(&m.transcript, 4000)
        ));
    }
    if !m.direction.is_empty() {
        parts.push(format!("【答题说明】{}", m.direction));
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("\n\n"))
    }
}

async fn media_context(transcriber: &Transcriber, m: &Module) -> Option<String> {
    if m.media_sources.is_empty() {
        return None;
    }
    if !m.material.is_empty() || !m.transcript.is_empty() {
        return None;
    }
    let mut texts: Vec<String> = Vec::new();
    for url in &m.media_sources {
        if let Ok(t) = transcriber.transcribe(url).await
            && !t.is_empty()
        {
            texts.push(t);
        }
    }
    if texts.is_empty() {
        return None;
    }
    let joined = texts.join("\n\n");
    log::info!(
        "转录媒体 {} 条，文本 {} 字符",
        m.media_sources.len(),
        joined.chars().count()
    );
    Some(format!(
        "【音频/视频转写内容】\n{}",
        truncate(&joined, 5000)
    ))
}
