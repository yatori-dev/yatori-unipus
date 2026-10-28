use crate::model::{ChildQ, Module, OptionItem};
use crate::text::truncate;

fn context_lines(context: Option<&str>) -> Vec<String> {
    context
        .map(|text| vec![text.to_string(), String::new()])
        .unwrap_or_default()
}

pub(super) fn choice(context: Option<&str>, c: &ChildQ, is_multi: bool) -> String {
    let mut lines = context_lines(context);
    let qtext = if c.question_text.is_empty() {
        "（题目见上方材料）".to_string()
    } else {
        c.question_text.clone()
    };
    lines.push(format!(
        "【{}】{}",
        if is_multi { "多选题" } else { "单选题" },
        qtext
    ));
    for opt in &c.options {
        let label = opt.label();
        let txt = opt.display_text();
        lines.push(format!("{}: {}", label, txt));
    }
    lines.push(String::new());
    if is_multi {
        lines.push("请只回答选项字母，多个用逗号分隔（如 A,B,C）。".to_string());
    } else {
        lines.push("请只回答一个选项字母（如 A）。".to_string());
    }
    lines.join("\n")
}

pub(super) fn batch(context: Option<&str>, m: &Module) -> String {
    let count = m.children.len();
    let mut lines = context_lines(context);
    lines.push(format!(
        "【{}题】共 {} 小题，请依次作答：",
        if m.reply_type == "fillblank" {
            "填空"
        } else {
            "简答"
        },
        count
    ));
    for (i, c) in m.children.iter().enumerate() {
        let q = if c.question_text.is_empty() {
            String::new()
        } else {
            c.question_text.clone()
        };
        lines.push(format!(
            "{}. {}",
            i + 1,
            if q.is_empty() {
                "(见上方材料)".to_string()
            } else {
                q
            }
        ));
    }
    lines.push(String::new());
    lines.push("请按题号回答，格式：1.答案 2.答案 ...".to_string());
    lines.push("如果不是翻译题，请只用英文回答。".to_string());
    lines.join("\n")
}

pub(super) fn banked_cloze(context: Option<&str>, count: usize, words: &[String]) -> String {
    let mut lines = context_lines(context);
    lines.push(
        "【选词填空】短文中共有 {} 个编号空格（____1____、____2____…），词库如下："
            .replace("{}", &count.to_string()),
    );
    lines.push(words.join("、"));
    lines.push(String::new());
    lines.push("请为每个编号空格选择最合适的单词，每个词限用一次。".to_string());
    lines.push("请按题号回答，格式：1.单词 2.单词 3.单词 ...（只输出单词本身）".to_string());
    lines.join("\n")
}

pub(super) fn dropdown(context: Option<&str>, m: &Module) -> String {
    let count = m.children.len();
    let mut lines = context_lines(context);
    lines.push(format!(
        "【下拉选择】材料中共有 {} 个编号空格（____1____、____2____…），请为每个空格从对应选项中选出最合适的一项：",
        count
    ));
    for (i, c) in m.children.iter().enumerate() {
        let opts: Vec<String> = c
            .options
            .iter()
            .map(option_label_text)
            .filter(|s| !s.is_empty())
            .collect();
        lines.push(format!(
            "第{}空（____{}____）选项: {}",
            i + 1,
            i + 1,
            opts.join("，")
        ));
    }
    lines.push(String::new());
    lines.push("请按题号回答，格式：1.A 2.B 3.A ...（只输出选项字母）".to_string());
    lines.join("\n")
}

fn option_label_text(o: &OptionItem) -> String {
    let label = o.label();
    let text = o.display_text();
    match (label.is_empty(), text.is_empty()) {
        (false, false) => format!("{}={}", label, text),
        (false, true) => label.to_string(),
        (true, false) => text.to_string(),
        (true, true) => String::new(),
    }
}

pub(super) fn discussion(context: Option<&str>, m: &Module) -> String {
    let mut lines: Vec<String> = Vec::new();
    if !m.direction.is_empty() {
        lines.push(format!("【答题说明】{}", m.direction));
        lines.push(String::new());
    }
    if !m.material.is_empty() {
        lines.push(format!("【讨论题目】\n{}", truncate(&m.material, 3000)));
    } else if let Some(context) = context {
        lines.push(context.to_string());
    }
    lines.push(String::new());
    lines.push(
        "请根据以上讨论题目写一段英文讨论发言（约 100-150 词），需回应全部问题，\
         观点明确、语言自然。只输出发言正文，不要编号、标题或引号。"
            .to_string(),
    );
    lines.join("\n")
}
