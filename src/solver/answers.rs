use crate::model::{ChildQ, Module, OptionItem};

pub(super) fn parse_dropdown_answers(ans: &str, expected: usize) -> Vec<String> {
    let mut parts = split_numbered(ans);
    if parts.len() < expected {
        let tokens: Vec<String> = ans
            .split(|ch: char| ch.is_whitespace() || matches!(ch, ',' | '，' | ';' | '；' | '、'))
            .map(|s| {
                s.trim()
                    .trim_matches(|ch: char| matches!(ch, '.' | ')' | ']' | '、'))
                    .to_string()
            })
            .filter(|s| !s.is_empty())
            .collect();
        if tokens.len() >= expected {
            parts = tokens;
        }
    }
    parts
}

pub(super) fn canonicalize_dropdown(parts: &[String], m: &Module) -> Vec<String> {
    let mut out = Vec::with_capacity(m.children.len());
    for (i, c) in m.children.iter().enumerate() {
        let raw = parts.get(i).map(|s| s.trim()).unwrap_or("");
        match match_option_label(raw, c) {
            Some(label) => out.push(label),
            None => {
                log::warn!(
                    "下拉选择第 {} 空答案无法匹配选项（{:?}），随机兜底",
                    i + 1,
                    raw
                );
                out.push(random_choice(c));
            }
        }
    }
    out
}

pub(super) fn match_option_label(raw: &str, c: &ChildQ) -> Option<String> {
    if raw.is_empty() {
        return None;
    }
    let cleaned = raw.trim().trim_matches(|ch: char| {
        matches!(
            ch,
            '.' | ',' | '，' | '。' | '、' | ';' | '；' | '"' | '\'' | '“' | '”'
        )
    });
    if cleaned.is_empty() {
        return None;
    }
    let lower = cleaned.to_lowercase();
    for o in &c.options {
        let label = o.label().to_string();
        if !label.is_empty() && label.to_lowercase() == lower {
            return Some(label);
        }
    }
    for o in &c.options {
        let text = if o.text.is_empty() { &o.value } else { &o.text };
        if !text.is_empty() && text.trim().to_lowercase() == lower {
            let label = o.label().to_string();
            if !label.is_empty() {
                return Some(label);
            }
        }
    }
    let letter = cleaned
        .chars()
        .find(|ch| ch.is_ascii_alphabetic())
        .map(|ch| ch.to_ascii_uppercase().to_string());
    if let Some(letter) = letter {
        for o in &c.options {
            let label = o.label().to_string();
            if !label.is_empty() && label.to_uppercase() == letter {
                return Some(label);
            }
        }
    }
    None
}

pub(super) fn clean_discussion_text(s: &str) -> String {
    let mut t = s.trim().to_string();
    if t.starts_with("```") {
        if let Some(pos) = t.find('\n') {
            t = t[pos + 1..].to_string();
        }
        if let Some(end) = t.rfind("```") {
            t.truncate(end);
        }
        t = t.trim().to_string();
    }
    let quotes: &[char] = &['"', '\'', '“', '”', '‘', '’'];
    if t.chars().count() >= 2 && t.starts_with(quotes) && t.ends_with(quotes) {
        t = t.trim_matches(quotes).trim().to_string();
    }
    t
}

pub(super) fn word_bank(module: &Module) -> Vec<String> {
    if !module.word_bank.is_empty() {
        return module
            .word_bank
            .iter()
            .map(|option| option.bank_word())
            .filter(|word| !word.is_empty())
            .map(str::to_string)
            .collect();
    }
    let mut words: Vec<String> = Vec::new();
    for option in module.children.iter().flat_map(|child| &child.options) {
        let word = option.bank_word();
        if !word.is_empty()
            && !words
                .iter()
                .any(|existing| existing.eq_ignore_ascii_case(word))
        {
            words.push(word.to_string());
        }
    }
    words
}

pub(super) fn shuffled_words(words: &[String], count: usize) -> Vec<String> {
    use rand::seq::SliceRandom;
    let mut pool = words.to_vec();
    pool.shuffle(&mut rand::rng());
    (0..count)
        .map(|i| pool.get(i).cloned().unwrap_or_default())
        .collect()
}

pub(super) fn canonicalize_banked(parts: &[String], words: &[String], count: usize) -> Vec<String> {
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let raw = parts.get(i).map(|s| s.as_str()).unwrap_or("");
        let clean = raw
            .trim()
            .trim_end_matches(['.', ',', '，', '。', '、', ';', '；'])
            .trim();
        let lower = clean.to_lowercase();
        let word = words
            .iter()
            .find(|w| w.to_lowercase() == lower)
            .cloned()
            .unwrap_or_else(|| clean.to_string());
        out.push(word);
    }
    out
}

fn valid_labels(options: &[OptionItem]) -> Vec<String> {
    options
        .iter()
        .map(|option| option.label().to_string())
        .collect()
}

pub(super) fn parse_single(ans: &str, options: &[OptionItem]) -> String {
    let labels = valid_labels(options);
    let upper = ans.to_uppercase();
    for lab in &labels {
        if lab.len() == 1 && upper.contains(lab.as_str()) {
            return lab.clone();
        }
    }
    upper
        .chars()
        .find(|c| c.is_ascii_uppercase())
        .map(|c| c.to_string())
        .unwrap_or_else(|| "A".to_string())
}

pub(super) fn parse_multi(ans: &str, options: &[OptionItem]) -> String {
    let labels = valid_labels(options);
    let upper: String = ans.to_uppercase().chars().collect();
    let mut picked: Vec<String> = Vec::new();
    for lab in &labels {
        if lab.len() == 1 && upper.contains(lab.as_str()) {
            picked.push(lab.clone());
        }
    }
    if picked.is_empty() {
        return "A".to_string();
    }
    picked.join(",")
}

pub(super) fn parse_banked(ans: &str, expected: usize) -> Vec<String> {
    let mut result = vec![String::new(); expected];
    let parts = split_numbered(ans);
    for (i, content) in parts.into_iter().enumerate() {
        if i < expected {
            result[i] = content;
        }
    }
    if result.iter().all(|r| r.is_empty()) {
        let lines: Vec<String> = ans
            .lines()
            .map(str::trim)
            .map(str::to_string)
            .filter(|l| !l.is_empty())
            .collect();
        for (i, line) in lines.into_iter().take(expected).enumerate() {
            result[i] = line;
        }
    }
    result
}

pub(super) fn split_numbered(ans: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes = ans.as_bytes();
    let n = bytes.len();
    let mut i = 0;
    while i < n {
        if bytes[i].is_ascii_digit() {
            while i < n && bytes[i].is_ascii_digit() {
                i += 1;
            }
            if i < n
                && (bytes[i] == b'.' || bytes[i] == b')' || bytes[i] == b']' || bytes[i] == b'>')
            {
                i += 1;
                while i < n && (bytes[i] == b' ' || bytes[i] == b'\t') {
                    i += 1;
                }
                let val_start = i;
                while i < n {
                    if bytes[i].is_ascii_digit() {
                        let j = i;
                        let mut j2 = j;
                        while j2 < n && bytes[j2].is_ascii_digit() {
                            j2 += 1;
                        }
                        if j2 < n && (bytes[j2] == b'.' || bytes[j2] == b')' || bytes[j2] == b']') {
                            break;
                        }
                    }
                    i += 1;
                }
                let val = ans[val_start..i].trim().to_string();
                if !val.is_empty() {
                    out.push(val);
                }
                continue;
            }
            i += 1;
            continue;
        }
        i += 1;
    }
    out
}

pub(super) fn random_choice(c: &ChildQ) -> String {
    use rand::RngExt;

    if !c.options.is_empty() {
        let idx = rand::rng().random_range(0..c.options.len());
        let o = &c.options[idx];
        let label = o.label().to_string();
        if !label.is_empty() {
            return label;
        }
        if !o.text.is_empty() {
            return o.text.clone();
        }
    }
    match c.reply_type.as_str() {
        "singlechoice" | "multichoice" if c.option_count > 0 => {
            let idx = rand::rng().random_range(0..c.option_count);
            ((b'A' + idx as u8) as char).to_string()
        }
        _ => "answer".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_single_works() {
        let opts = vec![
            OptionItem {
                name: "A".into(),
                value: "A".into(),
                text: "x".into(),
            },
            OptionItem {
                name: "B".into(),
                value: "B".into(),
                text: "y".into(),
            },
        ];
        assert_eq!(parse_single("1. B 正确答案是B", &opts), "B");
        assert_eq!(parse_multi("答案是 A、B、D", &opts), "A,B");
    }

    #[test]
    fn parse_banked_order() {
        assert_eq!(
            split_numbered("1. apple 2. banana"),
            vec!["apple", "banana"]
        );
        assert_eq!(parse_banked("1. one 2. two", 2), vec!["one", "two"]);
        assert_eq!(
            parse_banked("first answer\nsecond answer", 2),
            vec!["first answer", "second answer"]
        );
    }

    fn dropdown_module() -> Module {
        let opts = || {
            vec![
                OptionItem {
                    name: "A".into(),
                    value: "A".into(),
                    text: "do".into(),
                },
                OptionItem {
                    name: "B".into(),
                    value: "B".into(),
                    text: "make".into(),
                },
            ]
        };
        Module {
            instance_id: "m".into(),
            module_type: "basic-scoop-content".into(),
            direction: "Choose to fill in the blanks with either do or make.".into(),
            material: "I ____1____ my own decisions. He ____2____ an arrangement.".into(),
            media_sources: Vec::new(),
            transcript: String::new(),
            reply_type: "dropdownSelection".into(),
            word_bank: Vec::new(),
            children: (0..2)
                .map(|_| ChildQ {
                    question_type: "basic-scoop-content".into(),
                    reply_type: "dropdownSelection".into(),
                    question_text: String::new(),
                    options: opts(),
                    option_count: 2,
                })
                .collect(),
        }
    }

    #[test]
    fn dropdown_answers_normalized_to_labels() {
        let m = dropdown_module();
        let parts = parse_dropdown_answers("1.B 2.A", 2);
        assert_eq!(canonicalize_dropdown(&parts, &m), vec!["B", "A"]);

        let parts = parse_dropdown_answers("B A", 2);
        assert_eq!(canonicalize_dropdown(&parts, &m), vec!["B", "A"]);

        let parts = parse_dropdown_answers("1. make 2. do", 2);
        assert_eq!(canonicalize_dropdown(&parts, &m), vec!["B", "A"]);

        let parts = parse_dropdown_answers("B,B", 2);
        assert_eq!(canonicalize_dropdown(&parts, &m), vec!["B", "B"]);
    }

    #[test]
    fn dropdown_unknown_answer_falls_back_to_valid_label() {
        let m = dropdown_module();
        let parts = parse_dropdown_answers("1.Z", 2);
        let out = canonicalize_dropdown(&parts, &m);
        assert_eq!(out.len(), 2);
        assert!(out.iter().all(|v| v == "A" || v == "B"), "{:?}", out);
    }

    #[test]
    fn random_choice_uses_valid_labels() {
        let m = dropdown_module();
        for _ in 0..20 {
            let v = random_choice(&m.children[0]);
            assert!(v == "A" || v == "B", "unexpected: {}", v);
        }
    }

    #[test]
    fn discussion_text_cleanup() {
        assert_eq!(clean_discussion_text("  hello world  "), "hello world");
        assert_eq!(clean_discussion_text("\"quoted text\""), "quoted text");
        assert_eq!(
            clean_discussion_text("```\nMy travel story.\n```"),
            "My travel story."
        );
        assert_eq!(clean_discussion_text("“中文引号”"), "中文引号");
    }

    #[test]
    fn word_bank_dedupes_options() {
        let opts = |w: &str| OptionItem {
            name: w.into(),
            value: w.into(),
            text: String::new(),
        };
        let m = Module {
            instance_id: "m".into(),
            module_type: "material-banked-cloze".into(),
            direction: String::new(),
            material: String::new(),
            media_sources: Vec::new(),
            transcript: String::new(),
            reply_type: "bankedcloze".into(),
            word_bank: Vec::new(),
            children: vec![
                ChildQ {
                    question_type: "material-banked-cloze".into(),
                    reply_type: "bankedcloze".into(),
                    question_text: String::new(),
                    options: vec![opts("unique"), opts("strong"), opts("high")],
                    option_count: 3,
                },
                ChildQ {
                    question_type: "material-banked-cloze".into(),
                    reply_type: "bankedcloze".into(),
                    question_text: String::new(),
                    options: vec![opts("Unique"), opts("collective"), opts("strong")],
                    option_count: 3,
                },
            ],
        };
        assert_eq!(
            word_bank(&m),
            vec!["unique", "strong", "high", "collective"]
        );
    }

    #[test]
    fn banked_canonicalize_matches_bank() {
        let words = vec![
            "unique".to_string(),
            "collective".to_string(),
            "high".to_string(),
        ];
        let parts = split_numbered("1. Unique 2. Collective 3. HIGH");
        assert_eq!(
            canonicalize_banked(&parts, &words, 3),
            vec!["unique", "collective", "high"]
        );
        let partial = split_numbered("1. unique");
        assert_eq!(
            canonicalize_banked(&partial, &words, 3),
            vec!["unique", "", ""]
        );
    }

    #[test]
    fn shuffled_words_use_each_once() {
        let words = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let mut all = shuffled_words(&words, 3);
        all.sort();
        assert_eq!(all, vec!["a", "b", "c"]);
        let padded = shuffled_words(&words, 5);
        assert_eq!(padded.iter().filter(|s| s.is_empty()).count(), 2);
    }
}
