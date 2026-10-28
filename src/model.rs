//! 课程内容的领域类型，与 HTTP、存储和求解策略无关。

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct OptionItem {
    pub name: String,
    pub value: String,
    pub text: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ChildQ {
    pub question_type: String,

    pub reply_type: String,

    pub question_text: String,

    pub options: Vec<OptionItem>,

    pub option_count: usize,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Module {
    pub instance_id: String,

    pub module_type: String,

    pub direction: String,

    pub material: String,

    pub reply_type: String,

    pub media_sources: Vec<String>,

    pub transcript: String,

    pub word_bank: Vec<OptionItem>,
    pub children: Vec<ChildQ>,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct ParsedGroup {
    pub modules: Vec<Module>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ReplyType {
    SingleChoice,
    MultiChoice,
    FillBlank,
    TextArea,
    BankedCloze,
    DropdownSelection,
    Discussion,
    Vocabulary,
    Unknown(String),
}

impl ReplyType {
    pub fn parse(value: &str) -> Self {
        match value {
            "singlechoice" => Self::SingleChoice,
            "multichoice" => Self::MultiChoice,
            "fillblank" => Self::FillBlank,
            "text-area" => Self::TextArea,
            "bankedcloze" => Self::BankedCloze,
            "dropdownSelection" => Self::DropdownSelection,
            "discussion" => Self::Discussion,
            "vocabulary" => Self::Vocabulary,
            other => Self::Unknown(other.to_string()),
        }
    }
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct VocabWord {
    pub name: String,
    pub sound: String,
}

impl OptionItem {
    pub fn label(&self) -> &str {
        if self.name.is_empty() {
            &self.value
        } else {
            &self.name
        }
    }

    pub fn display_text(&self) -> &str {
        if self.text.is_empty() {
            &self.value
        } else {
            &self.text
        }
    }

    pub fn bank_word(&self) -> &str {
        if !self.value.is_empty() {
            &self.value
        } else if !self.name.is_empty() {
            &self.name
        } else {
            &self.text
        }
    }
}

impl Module {
    pub fn reply_type(&self) -> ReplyType {
        ReplyType::parse(&self.reply_type)
    }

    pub fn is_discussion(&self) -> bool {
        self.reply_type == "discussion"
    }
}

impl ParsedGroup {
    pub fn question_count(&self) -> usize {
        self.modules
            .iter()
            .map(|module| module.children.len())
            .sum()
    }

    pub fn has_answerable_modules(&self) -> bool {
        self.modules
            .iter()
            .any(|module| !module.children.is_empty() && !module.is_discussion())
    }
}

#[cfg(test)]
mod tests {
    use crate::model::{ChildQ, Module, ParsedGroup};

    fn module(reply_type: &str, children: usize) -> Module {
        Module {
            instance_id: "m".into(),
            module_type: reply_type.into(),
            direction: String::new(),
            material: String::new(),
            media_sources: Vec::new(),
            transcript: String::new(),
            reply_type: reply_type.into(),
            word_bank: Vec::new(),
            children: (0..children)
                .map(|i| ChildQ {
                    question_type: "basic".into(),
                    reply_type: reply_type.into(),
                    question_text: format!("q{}", i),
                    options: Vec::new(),
                    option_count: 0,
                })
                .collect(),
        }
    }

    fn group(modules: Vec<Module>) -> ParsedGroup {
        ParsedGroup { modules }
    }

    #[test]
    fn vocabulary_group_is_not_answerable() {
        assert!(!group(vec![module("vocabulary", 0)]).has_answerable_modules());
        assert!(!group(vec![module("", 0)]).has_answerable_modules());
        assert!(!group(vec![module("discussion", 1)]).has_answerable_modules());
    }

    #[test]
    fn choice_group_is_answerable() {
        assert!(group(vec![module("singlechoice", 1)]).has_answerable_modules());

        assert!(
            group(vec![module("discussion", 1), module("fillblank", 1),]).has_answerable_modules()
        );
    }
}
